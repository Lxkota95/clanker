//! Slash commands. Add a command: write it in its own module, then list it in `all()`.

mod ping;
mod rank;

use crate::{Data, Error};

/// Every command the bot registers with Discord.
pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![ping::ping(), rank::rank()]
}
