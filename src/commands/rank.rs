use crate::rank::source::{LookupError, Platform, Player, RankSource};
use crate::rank::{self, RankTier};
use crate::{Context, Error};
use anyhow::Context as _;
use poise::serenity_prelude as serenity;

/// Set your Rainbow Six Siege rank: clanker gives you that rank's role (and removes your old one)
// Named /r6rank, not /rank: other bots (e.g. Dyno) commonly own /rank, and Discord lists every
// bot's command of the same name side by side.
//
// No `required_bot_permissions = "MANAGE_ROLES"`: poise checks it against the bot's permissions in
// the *channel*, where an @everyone "Manage Permissions" deny (same permission bit) is common and
// blocks the command, yet assigning server roles only depends on *server-level* Manage Roles plus
// role order. Discord's own answer is authoritative; `apply` explains a 403.
#[poise::command(slash_command, rename = "r6rank", guild_only)]
pub async fn rank(
    ctx: Context<'_>,
    #[description = "Your current Ranked tier"] tier: RankTier,
) -> Result<(), Error> {
    let text = set_rank_role(ctx, tier, "clanker /r6rank").await?;
    reply(ctx, text).await
}

/// Look up your Rainbow Six Siege rank and get the matching role
// Only registered when a rank source is configured (see `commands::all`). It assigns the role to
// whoever runs it, for the name they give: it checks the rank, not that the account is theirs.
// The 10-minute cooldown protects Arenyze's free quota (2,000 calls/month) from one person spamming
#[poise::command(slash_command, rename = "r6verify", guild_only, user_cooldown = 600)]
pub async fn verify(
    ctx: Context<'_>,
    #[description = "Your Ubisoft account name"] name: String,
    #[description = "Where you play ranked"] platform: Platform,
) -> Result<(), Error> {
    let Some(source) = ctx.data().rank_source.as_deref() else {
        return reply(
            ctx,
            "Rank lookups aren't set up on this bot. Use /r6rank.".to_owned(),
        )
        .await;
    };

    // Discord drops an interaction that isn't answered within 3 seconds. A lookup can take
    // longer, so acknowledge now ("clanker is thinking...", visible only to the caller) and
    // send the real answer when it's ready.
    ctx.defer_ephemeral().await?;

    let player = Player { name, platform };
    let lookup = match source.lookup(&player).await {
        Ok(lookup) => lookup,
        Err(e) => return reply(ctx, lookup_failed(source, &e)).await,
    };

    let points = lookup
        .rank_points
        .map(|rp| format!(" ({rp} RP)"))
        .unwrap_or_default();
    let reason = format!("clanker /r6verify via {}", source.name());
    let text = set_rank_role(ctx, lookup.tier, &reason).await?;
    reply(
        ctx,
        format!(
            "{} is **{}**{points} according to {}.\n{text}",
            player.name,
            lookup.tier.role_name(),
            source.name()
        ),
    )
    .await
}

/// Reply text for a failed lookup; roles are left unchanged.
fn lookup_failed(source: &dyn RankSource, error: &LookupError) -> String {
    let hint = match error {
        LookupError::PlayerNotFound(_) => "Check the spelling and the platform.",
        LookupError::NoRankedData(_) => "You can set your rank yourself with /r6rank.",
        LookupError::Unavailable(_) => "Try again later, or set your rank with /r6rank.",
    };
    if let LookupError::Unavailable(cause) = error {
        tracing::warn!(source = source.name(), "rank lookup failed: {cause:#}");
    }
    format!(
        "{} lookup: {error}. {hint} Your roles weren't changed.",
        source.name()
    )
}

/// Gives the command's author `tier`'s rank role and removes their other rank roles; returns the
/// reply text. Shared by /r6rank and /r6verify, whatever produced the tier.
async fn set_rank_role(ctx: Context<'_>, tier: RankTier, reason: &str) -> Result<String, Error> {
    // guild_only guarantees both of these for slash commands
    let guild_id = ctx.guild_id().context("used outside a server")?;
    let member = ctx
        .author_member()
        .await
        .context("could not read your server membership")?;

    // Each `.await` is a request to Discord's API; tokio runs other work (other users' commands,
    // the gateway heartbeat) while this one waits for the response
    let server_roles = guild_id.roles(ctx.http()).await?;
    let names: Vec<_> = server_roles
        .iter()
        .map(|(id, role)| (*id, role.name.as_str()))
        .collect();
    let plan = rank::plan(tier, &names, &member.roles);

    if plan.add.is_none() && plan.missing.contains(&tier.role_name()) {
        return Ok(format!(
            "This server has no **{}** role yet, so I can't give it to you. Ask an admin to \
             create it.{}",
            tier.role_name(),
            missing_note(&plan.missing)
        ));
    }

    let http = ctx.http();
    for old in &plan.remove {
        apply(http.remove_member_role(guild_id, member.user.id, *old, Some(reason))).await?;
    }
    if let Some(new) = plan.add {
        apply(http.add_member_role(guild_id, member.user.id, new, Some(reason))).await?;
    }

    let text = if plan.add.is_none() && plan.remove.is_empty() {
        format!("You already have the **{}** role.", tier.role_name())
    } else {
        format!("You're now **{}**.", tier.role_name())
    };
    Ok(format!("{text}{}", missing_note(&plan.missing)))
}

/// Turns Discord's "Missing Permissions" into an explanation of the two causes: the bot needs
/// server-level Manage Roles, and its role must sit above the rank roles in Server Settings → Roles.
async fn apply(request: impl Future<Output = serenity::Result<()>>) -> Result<(), Error> {
    request.await.map_err(|e| match &e {
        serenity::Error::Http(serenity::HttpError::UnsuccessfulRequest(response))
            if response.status_code == serenity::StatusCode::FORBIDDEN =>
        {
            anyhow::Error::new(e).context(
                "Discord refused the role change (403). clanker needs Manage Roles, and its role must \
                 be above the rank roles in Server Settings → Roles.",
            )
        }
        _ => anyhow::Error::new(e),
    })
}

/// Appended to replies when some rank roles don't exist yet, so admins know what to create.
fn missing_note(missing: &[&str]) -> String {
    if missing.is_empty() {
        String::new()
    } else {
        format!(
            "\n-# Rank roles not on this server yet: {}",
            missing.join(", ")
        )
    }
}

/// Replies so only the person who ran the command sees it. After `defer_ephemeral`, poise sends
/// this as the follow-up to the deferred response.
async fn reply(ctx: Context<'_>, content: String) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default()
            .content(content)
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rank::source::tests::FixedSource;

    #[test]
    fn failed_lookups_explain_and_point_to_r6rank() {
        let source = FixedSource(vec![]);
        let text = lookup_failed(&source, &LookupError::NoRankedData("ash".to_owned()));
        assert!(text.contains("no current ranked data"), "{text}");
        assert!(text.contains("/r6rank"), "{text}");
        assert!(text.contains("weren't changed"), "{text}");
    }
}
