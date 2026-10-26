//! [Arenyze](https://r6.arenyze.com) as a [`RankSource`]: Ubisoft's Siege stats through a free,
//! key-gated API (`GET /r6/api/v2/profile`, key in an `api-key` header).
//!
//! The free plan allows 2,000 calls a month, and the stats endpoints 40/minute, 200/hour and
//! 400/day (`X-RateLimit-*` headers). So a successful lookup is cached per player for
//! [`CACHE_TTL`], `/r6verify` has a per-user cooldown, and the remaining daily quota is logged.
//! When Ubisoft throttles Arenyze it answers `503` with `code: STATS_UPSTREAM_THROTTLED` and a
//! `retryAfter`; that becomes [`LookupError::Unavailable`] and changes no roles.

use super::RankTier;
use super::source::{LookupError, Platform, Player, RankLookup, RankSource};
use anyhow::anyhow;
use async_trait::async_trait;
use reqwest::{StatusCode, header::HeaderMap};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

const BASE_URL: &str = "https://public-api.arenyze.com/r6/api";
/// How long a looked-up rank is reused before asking Arenyze again.
const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Warn in the logs when fewer daily calls than this remain.
const LOW_DAILY_QUOTA: u32 = 25;

type CacheKey = (String, Platform);

pub struct Arenyze {
    http: reqwest::Client,
    api_key: String,
    /// `std::sync::Mutex`, not tokio's: it's only held for a quick map lookup/insert, never across
    /// an `.await`, which is when tokio's async mutex would be needed.
    cache: Mutex<HashMap<CacheKey, (Instant, RankLookup)>>,
}

impl Arenyze {
    pub fn new(api_key: String) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent("clanker (https://github.com/Lxkota95/clanker)")
            .build()?;
        Ok(Self {
            http,
            api_key,
            cache: Mutex::default(),
        })
    }

    fn cache_key(player: &Player) -> CacheKey {
        // Ubisoft names aren't case-sensitive for lookups
        (player.name.to_lowercase(), player.platform)
    }

    fn cached(&self, key: &CacheKey, now: Instant) -> Option<RankLookup> {
        // A poisoned lock only means another thread panicked mid-update of a cache; the map is
        // still usable, so carry on rather than panic too
        let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache
            .get(key)
            .filter(|(at, _)| now.duration_since(*at) < CACHE_TTL)
            .map(|(_, lookup)| *lookup)
    }

    fn remember(&self, key: CacheKey, lookup: RankLookup, now: Instant) {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache.retain(|_, (at, _)| now.duration_since(*at) < CACHE_TTL);
        cache.insert(key, (now, lookup));
    }
}

#[async_trait]
impl RankSource for Arenyze {
    fn name(&self) -> &'static str {
        "Arenyze"
    }

    async fn lookup(&self, player: &Player) -> Result<RankLookup, LookupError> {
        let key = Self::cache_key(player);
        if let Some(lookup) = self.cached(&key, Instant::now()) {
            tracing::debug!(player = %player.name, "rank from cache");
            return Ok(lookup);
        }

        let unavailable = |e: reqwest::Error| {
            LookupError::Unavailable(anyhow::Error::new(e).context("request to Arenyze failed"))
        };
        let response = self
            .http
            .get(format!("{BASE_URL}/v2/profile"))
            .header("api-key", &self.api_key)
            .query(&[
                ("nameOnPlatform", player.name.as_str()),
                ("platformType", player.platform.ubisoft_platform_type()),
            ])
            .send()
            .await
            .map_err(unavailable)?;
        log_quota(response.headers());
        let status = response.status();
        let body = response.text().await.map_err(unavailable)?;

        let lookup = parse_response(status, &body, &player.name)?;
        self.remember(key, lookup, Instant::now());
        Ok(lookup)
    }
}

/// Logs the remaining daily quota from Arenyze's rate-limit headers.
fn log_quota(headers: &HeaderMap) {
    let remaining = headers
        .get("x-ratelimit-day-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u32>().ok());
    match remaining {
        Some(left) if left < LOW_DAILY_QUOTA => {
            tracing::warn!(left, "Arenyze daily quota is nearly used up");
        }
        Some(left) => tracing::debug!(left, "Arenyze daily quota remaining"),
        None => {}
    }
}

/// Turns an Arenyze response into a lookup result. Separate from the HTTP call so every case can
/// be tested with recorded responses.
fn parse_response(status: StatusCode, body: &str, name: &str) -> Result<RankLookup, LookupError> {
    if status.is_success() {
        let profile: ProfileResponse = serde_json::from_str(body).map_err(|e| {
            LookupError::Unavailable(
                anyhow::Error::new(e).context("Arenyze's profile response has an unexpected shape"),
            )
        })?;
        return current_ranked(&profile).ok_or_else(|| LookupError::NoRankedData(name.to_owned()));
    }

    let error: ApiError = serde_json::from_str(body).unwrap_or_default();
    let code = error.code.as_deref().unwrap_or("");
    if status == StatusCode::NOT_FOUND || code.contains("NOT_FOUND") {
        return Err(LookupError::PlayerNotFound(name.to_owned()));
    }
    let retry = error
        .retry_after
        .map(|s| format!(", retry after {s}s"))
        .unwrap_or_default();
    Err(LookupError::Unavailable(anyhow!(
        "Arenyze returned {status} {code}: {}{retry}",
        error.message.as_deref().unwrap_or("no message")
    )))
}

/// The current season's ranked standing: the `ranked` board's most recent season, across the
/// player's platform families.
fn current_ranked(profile: &ProfileResponse) -> Option<RankLookup> {
    profile
        .stats
        .platform_families_full_profiles
        .iter()
        .flat_map(|family| &family.board_ids_full_profiles)
        .filter(|board| board.board_id == "ranked")
        .flat_map(|board| &board.full_profiles)
        .max_by_key(|season| season.season_id)
        .map(|season| RankLookup {
            tier: RankTier::from_rank_points(season.profile.rank_points),
            rank_points: Some(season.profile.rank_points),
        })
}

// The parts of Arenyze's responses clanker reads; serde ignores every other field.

#[derive(Deserialize)]
struct ProfileResponse {
    stats: Stats,
}

#[derive(Deserialize)]
struct Stats {
    platform_families_full_profiles: Vec<PlatformFamily>,
}

#[derive(Deserialize)]
struct PlatformFamily {
    board_ids_full_profiles: Vec<Board>,
}

#[derive(Deserialize)]
struct Board {
    board_id: String,
    full_profiles: Vec<SeasonProfile>,
}

#[derive(Deserialize)]
struct SeasonProfile {
    season_id: u32,
    profile: RankedProfile,
}

#[derive(Deserialize)]
struct RankedProfile {
    rank_points: u32,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ApiError {
    message: Option<String>,
    code: Option<String>,
    retry_after: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A profile in the documented shape, with a `ranked` board for two seasons and another board.
    fn profile(seasons: &[(u32, u32)]) -> String {
        let full_profiles: Vec<_> = seasons
            .iter()
            .map(|(season_id, rank_points)| {
                serde_json::json!({
                    "season_id": season_id,
                    "profile": { "rank": 0, "rank_points": rank_points, "max_rank": 0 },
                    "season_statistics": {}
                })
            })
            .collect();
        serde_json::json!({
            "player": { "nameOnPlatform": "ash", "platformType": "uplay" },
            "stats": { "platform_families_full_profiles": [{
                "profile_id": "x",
                "board_ids_full_profiles": [
                    { "board_id": "casual", "full_profiles": [] },
                    { "board_id": "ranked", "full_profiles": full_profiles }
                ]
            }]},
            "meta": { "partial": false }
        })
        .to_string()
    }

    #[test]
    fn uses_the_latest_ranked_season() {
        let lookup = parse_response(StatusCode::OK, &profile(&[(40, 1200), (41, 3650)]), "ash");
        assert_eq!(
            lookup.unwrap(),
            RankLookup {
                tier: RankTier::Emerald,
                rank_points: Some(3650)
            }
        );
    }

    #[test]
    fn no_ranked_seasons_is_no_ranked_data() {
        let err = parse_response(StatusCode::OK, &profile(&[]), "ash").unwrap_err();
        assert!(matches!(err, LookupError::NoRankedData(name) if name == "ash"));
    }

    #[test]
    fn throttling_is_unavailable() {
        // Recorded from Arenyze on 2026-10-25
        let body = r#"{"error":"Service temporarily unavailable","message":"The stats service is temporarily overloaded. Please retry later.","retryAfter":34,"code":"STATS_UPSTREAM_THROTTLED"}"#;
        let err = parse_response(StatusCode::SERVICE_UNAVAILABLE, body, "ash").unwrap_err();
        let LookupError::Unavailable(cause) = err else {
            panic!("expected Unavailable, got {err:?}");
        };
        let cause = cause.to_string();
        assert!(cause.contains("STATS_UPSTREAM_THROTTLED"), "{cause}");
        assert!(cause.contains("retry after 34s"), "{cause}");
    }

    #[test]
    fn not_found_is_player_not_found() {
        let err = parse_response(StatusCode::NOT_FOUND, "{}", "ash").unwrap_err();
        assert!(matches!(err, LookupError::PlayerNotFound(_)));
    }

    #[test]
    fn unexpected_success_body_is_unavailable_not_a_panic() {
        let err = parse_response(StatusCode::OK, "<html>maintenance</html>", "ash").unwrap_err();
        assert!(matches!(err, LookupError::Unavailable(_)));
    }

    #[test]
    fn cache_expires_after_the_ttl() {
        let arenyze = Arenyze::new("test-key".to_owned()).unwrap();
        let key = ("ash".to_owned(), Platform::Pc);
        let gold = RankLookup {
            tier: RankTier::Gold,
            rank_points: Some(2600),
        };
        let start = Instant::now();
        arenyze.remember(key.clone(), gold, start);

        assert_eq!(
            arenyze.cached(&key, start + Duration::from_secs(60)),
            Some(gold)
        );
        assert_eq!(arenyze.cached(&key, start + CACHE_TTL), None);
    }
}
