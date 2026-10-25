//! Slash commands. Add a command: write it in its own module, then list it in `all()`.

mod ping;
mod rank;

use crate::{Data, Error};

/// Every command the bot registers with Discord. `/r6verify` only when a rank source is
/// configured, so members never see a command that can't work.
pub fn all(has_rank_source: bool) -> Vec<poise::Command<Data, Error>> {
    let mut commands = vec![ping::ping(), rank::rank()];
    if has_rank_source {
        commands.push(rank::verify());
    }
    commands
}
