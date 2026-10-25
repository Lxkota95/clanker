//! Where a player's rank comes from, when it's looked up rather than chosen by the member.
//!
//! Every Siege rank provider sits on Ubisoft's unofficial API and can break or disappear (the
//! service the original Python bot used did), so each one lives behind [`RankSource`]: swapping
//! providers means adding an implementation, and nothing that assigns roles changes. `/r6rank`
//! (the member picks their tier) needs no source and keeps working whatever happens to them.

use super::RankTier;
use async_trait::async_trait;

/// The platform a player's Ubisoft account plays ranked on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum Platform {
    #[name = "PC (Ubisoft Connect)"]
    Pc,
    #[name = "PlayStation"]
    PlayStation,
    #[name = "Xbox"]
    Xbox,
}

impl Platform {
    /// Ubisoft's `platformType` value, which provider APIs take as-is.
    pub fn ubisoft_platform_type(self) -> &'static str {
        match self {
            Self::Pc => "uplay",
            Self::PlayStation => "psn",
            Self::Xbox => "xbl",
        }
    }
}

/// Who to look up: a Ubisoft account name on a platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    pub name: String,
    pub platform: Platform,
}

/// A player's current ranked standing, as reported by a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RankLookup {
    pub tier: RankTier,
    /// Ranked 3.0 rank points, when the source reports them.
    pub rank_points: Option<u32>,
}

/// Why a lookup failed. A typed enum (unlike the app-wide `anyhow`), because the reply differs per
/// case: a typo, a player with nothing to show, or a provider that's down.
#[derive(Debug, thiserror::Error)]
pub enum LookupError {
    #[error("no player named `{0}` on that platform")]
    PlayerNotFound(String),

    #[error("`{0}` has no current ranked data (no ranked games this season, or a private profile)")]
    NoRankedData(String),

    #[error("the rank service is unavailable right now")]
    Unavailable(#[source] anyhow::Error),
}

/// A provider of players' current ranks.
///
/// `#[async_trait]` because clanker holds the configured source as `Box<dyn RankSource>` (chosen
/// at startup), and Rust's built-in `async fn` in traits doesn't support `dyn` yet. `Send + Sync`
/// because tokio may run commands that use it on any thread.
#[async_trait]
pub trait RankSource: Send + Sync {
    /// The provider's name, shown in replies (e.g. "Verified via Arenyze").
    fn name(&self) -> &'static str;

    /// Looks up `player`'s current ranked tier.
    async fn lookup(&self, player: &Player) -> Result<RankLookup, LookupError>;
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A source with canned answers, for tests: players it knows get their rank, everyone else
    /// is `PlayerNotFound`.
    pub struct FixedSource(pub Vec<(&'static str, RankLookup)>);

    #[async_trait]
    impl RankSource for FixedSource {
        fn name(&self) -> &'static str {
            "fixed"
        }

        async fn lookup(&self, player: &Player) -> Result<RankLookup, LookupError> {
            self.0
                .iter()
                .find(|(name, _)| *name == player.name)
                .map(|(_, rank)| *rank)
                .ok_or_else(|| LookupError::PlayerNotFound(player.name.clone()))
        }
    }

    fn player(name: &str) -> Player {
        Player {
            name: name.to_owned(),
            platform: Platform::Pc,
        }
    }

    #[tokio::test]
    async fn works_as_a_trait_object() {
        let source: Box<dyn RankSource> = Box::new(FixedSource(vec![(
            "ash",
            RankLookup {
                tier: RankTier::Gold,
                rank_points: Some(3100),
            },
        )]));

        let found = source.lookup(&player("ash")).await.unwrap();
        assert_eq!(found.tier, RankTier::Gold);

        let missing = source.lookup(&player("thermite")).await.unwrap_err();
        assert!(matches!(missing, LookupError::PlayerNotFound(name) if name == "thermite"));
    }

    #[test]
    fn platform_maps_to_ubisoft_platform_type() {
        assert_eq!(Platform::Pc.ubisoft_platform_type(), "uplay");
        assert_eq!(Platform::PlayStation.ubisoft_platform_type(), "psn");
        assert_eq!(Platform::Xbox.ubisoft_platform_type(), "xbl");
    }
}
