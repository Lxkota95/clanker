//! Slash commands. Add a command: write it in its own module, then list it in `all()`.

mod ping;

use crate::{Data, Error};

/// Every command the bot registers with Discord.
pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![ping::ping()]
}
