//! clanker: a Discord bot built on poise (commands) and serenity (Discord API), running on tokio.

mod commands;
mod rank;

use anyhow::Context as _;
use poise::serenity_prelude as serenity;
use tracing::{error, info};

/// State shared by every command invocation.
pub struct Data {
    /// Where `/r6verify` looks ranks up; `None` until a provider is configured (then `/r6verify`
    /// isn't registered). `Box<dyn …>` so the provider is chosen at startup, not compile time.
    pub rank_source: Option<Box<dyn rank::source::RankSource>>,
}

/// Applications can use `anyhow` for errors (libraries like the `ansible` crate define their own).
pub type Error = anyhow::Error;
pub type Context<'a> = poise::Context<'a, Data, Error>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Log level from RUST_LOG (e.g. RUST_LOG=clanker=debug), defaulting to info
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let token = std::env::var("DISCORD_TOKEN")
        .context("DISCORD_TOKEN is not set (see README: Running locally)")?;
    // Guild commands appear instantly; global ones can take up to an hour to show up
    let dev_guild = match std::env::var("DEV_GUILD_ID") {
        Ok(id) => Some(serenity::GuildId::new(
            id.parse()
                .context("DEV_GUILD_ID must be a numeric server ID")?,
        )),
        Err(_) => None,
    };

    // Rank lookups (/r6verify) are optional: only with an Arenyze API key
    let rank_source: Option<Box<dyn rank::source::RankSource>> =
        match std::env::var("ARENYZE_API_KEY") {
            Ok(key) if !key.is_empty() => {
                info!("rank lookups enabled (Arenyze)");
                Some(Box::new(rank::arenyze::Arenyze::new(key)?))
            }
            _ => {
                info!("rank lookups disabled: ARENYZE_API_KEY not set; /r6verify not registered");
                None
            }
        };

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands::all(rank_source.is_some()),
            on_error: |error| Box::pin(on_error(error)),
            ..Default::default()
        })
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                let commands = &framework.options().commands;
                if let Some(guild) = dev_guild {
                    poise::builtins::register_in_guild(ctx, commands, guild).await?;
                    info!(%guild, "registered {} commands in dev guild", commands.len());
                } else {
                    poise::builtins::register_globally(ctx, commands).await?;
                    info!("registered {} commands globally", commands.len());
                }
                info!("logged in as {}", ready.user.name);
                Ok(Data { rank_source })
            })
        })
        .build();

    // Slash commands need no privileged intents (prefix commands would need MESSAGE_CONTENT)
    let mut client =
        serenity::ClientBuilder::new(token, serenity::GatewayIntents::non_privileged())
            .framework(framework)
            .await
            .context("failed to build the Discord client")?;

    // Shut down cleanly on Ctrl-C / SIGTERM: a second task waits for the signal while the client runs
    let shard_manager = client.shard_manager.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        info!("shutting down");
        shard_manager.shutdown_all().await;
    });

    client
        .start()
        .await
        .context("Discord client stopped with an error")
}

/// Resolves on Ctrl-C (SIGINT) or SIGTERM (what systemd and container runtimes send).
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            error!("failed to listen for Ctrl-C: {e}");
        }
    };
    let mut sigterm = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        Ok(signal) => signal,
        Err(e) => {
            error!("failed to listen for SIGTERM: {e}");
            ctrl_c.await;
            return;
        }
    };
    // Whichever happens first wins; the other future is dropped
    tokio::select! {
        () = ctrl_c => {}
        _ = sigterm.recv() => {}
    }
}

/// Logs command errors, then lets poise's default handler reply to the user where it makes sense.
async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    if let poise::FrameworkError::Command { ctx, error, .. } = &error {
        error!(command = %ctx.command().qualified_name, "command failed: {error:#}");
    }
    if let Err(e) = poise::builtins::on_error(error).await {
        error!("error while handling an error: {e}");
    }
}
