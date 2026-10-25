# clanker
A Discord bot, written to learn [tokio](https://tokio.rs) and async Rust. Built on
[poise](https://github.com/serenity-rs/poise) (commands) and
[serenity](https://github.com/serenity-rs/serenity) (Discord API).

## Commands
| Command | What it does |
|---|---|
| `/ping` | Replies with the bot's gateway latency |

## Setting up the Discord application (once)
1. In the [Discord Developer Portal](https://discord.com/developers/applications), create an
   application named `clanker`.
2. **Bot** page: **Reset Token** and copy it. No privileged intents are needed (slash commands only).
3. Store the token in the GNOME keyring (paste it when prompted), and keep a copy in Bitwarden:
   ```
   secret-tool store --label='clanker Discord bot token' service discord app clanker
   ```
4. **OAuth2 → URL Generator**: scopes `bot` and `applications.commands`, bot permission
   `Send Messages`. Open the generated URL to invite clanker to your server.

## Running locally
```
direnv allow   # once: loads DISCORD_TOKEN from the keyring via .envrc
cargo run
```
Set `DEV_GUILD_ID` in `.envrc` to your test server's ID so commands appear there instantly;
without it, commands register globally, which can take up to an hour to show up. Stop with Ctrl-C
(clanker shuts down its gateway connection cleanly).

## Development
- `cargo test`, `cargo clippy --all-targets`, `cargo fmt` (CI runs all three, warnings as errors)
- New command: add `src/commands/<name>.rs`, then list it in `src/commands/mod.rs`
- Logging: `RUST_LOG=clanker=debug cargo run`

## License
MIT
