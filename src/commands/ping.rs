use crate::{Context, Error};
use std::time::Duration;

/// Check that the bot is alive, and its connection latency to Discord
#[poise::command(slash_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    // `ping()` reads the gateway heartbeat latency; `.await` yields to tokio while it locks the
    // shard manager instead of blocking the thread
    let latency = ctx.ping().await;
    ctx.say(reply(latency)).await?;
    Ok(())
}

/// The reply text, kept separate from the command so it can be unit tested without Discord.
fn reply(latency: Duration) -> String {
    if latency.is_zero() {
        // Right after connecting, no heartbeat has completed yet
        "Pong! (latency not measured yet, try again in a minute)".to_owned()
    } else {
        format!("Pong! {} ms", latency.as_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_shows_milliseconds() {
        assert_eq!(reply(Duration::from_millis(42)), "Pong! 42 ms");
    }

    #[test]
    fn reply_explains_missing_latency() {
        assert!(reply(Duration::ZERO).contains("not measured yet"));
    }
}
