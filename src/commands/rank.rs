use crate::rank::{self, RankTier};
use crate::{Context, Error};
use anyhow::Context as _;
use poise::serenity_prelude as serenity;

/// Reason shown in the server's audit log for every role change clanker makes here.
const AUDIT_REASON: &str = "clanker /rank";

/// Set your Rainbow Six Siege rank: clanker gives you that rank's role (and removes your old one)
#[poise::command(slash_command, guild_only, required_bot_permissions = "MANAGE_ROLES")]
pub async fn rank(
    ctx: Context<'_>,
    #[description = "Your current Ranked tier"] tier: RankTier,
) -> Result<(), Error> {
    // guild_only guarantees both of these for slash commands
    let guild_id = ctx.guild_id().context("/rank used outside a server")?;
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
        reply(
            ctx,
            format!(
                "This server has no **{}** role yet, so I can't give it to you. Ask an admin to \
                 create it.{}",
                tier.role_name(),
                missing_note(&plan.missing)
            ),
        )
        .await?;
        return Ok(());
    }

    let http = ctx.http();
    for old in &plan.remove {
        apply(http.remove_member_role(guild_id, member.user.id, *old, Some(AUDIT_REASON))).await?;
    }
    if let Some(new) = plan.add {
        apply(http.add_member_role(guild_id, member.user.id, new, Some(AUDIT_REASON))).await?;
    }

    let text = if plan.add.is_none() && plan.remove.is_empty() {
        format!("You already have the **{}** role.", tier.role_name())
    } else {
        format!("You're now **{}**.", tier.role_name())
    };
    reply(ctx, format!("{text}{}", missing_note(&plan.missing))).await?;
    Ok(())
}

/// Turns Discord's "Missing Permissions" into an explanation of the usual cause: the bot's role
/// must sit above the rank roles in Server Settings → Roles, even with Manage Roles.
async fn apply(request: impl Future<Output = serenity::Result<()>>) -> Result<(), Error> {
    request.await.map_err(|e| match &e {
        serenity::Error::Http(serenity::HttpError::UnsuccessfulRequest(response))
            if response.status_code == serenity::StatusCode::FORBIDDEN =>
        {
            anyhow::Error::new(e).context(
                "Discord refused the role change (403). clanker's role must be above the rank \
                 roles in Server Settings → Roles.",
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

/// Replies so only the person who ran the command sees it.
async fn reply(ctx: Context<'_>, content: String) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default()
            .content(content)
            .ephemeral(true),
    )
    .await?;
    Ok(())
}
