//! Fetch Copilot quota from `copilot_internal/user`.

use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::cache::{Cache, MAX_STALE, acquire_lock_async};
use crate::error::{AppError, Result};
use crate::usage::CopilotSnapshot;

use super::types::CopilotUser;

pub const BASE_URL: &str = "https://api.github.com";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);
/// The endpoint is the editors' own; it answers a recognised editor build and
/// is inconsistent about unknown ones.
const EDITOR_VERSION: &str = "vscode/1.104.1";
/// Not decoration: api.github.com answers a request without a User-Agent with
/// `403 Request forbidden by administrative rules`, which the error mapper then
/// reports as rejected credentials. The shared client sets none.
const USER_AGENT: &str = "GitHubCopilotChat/0.26.7";

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub user: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            user: format!("{BASE_URL}/copilot_internal/user"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub snapshot: CopilotSnapshot,
    pub stale: bool,
    pub last_error: Option<(u16, String)>,
    pub cache_age: Option<Duration>,
}

pub async fn fetch_snapshot(
    client: &reqwest::Client,
    token: &str,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
) -> Result<FetchOutcome> {
    cache.ensure_dir()?;
    let _lock = acquire_lock_async(&cache.lock_path(), LOCK_TIMEOUT).await?;

    if let Some(bytes) = cache.fresh_payload(cache_ttl)?
        && let Ok(outcome) = reuse_cache(bytes, cache, false)
    {
        return Ok(outcome);
    }

    match fetch_live(client, &endpoints.user, token).await {
        Ok(snap) => {
            let bytes = serde_json::to_vec(&snap_to_json(&snap))?;
            cache.write_payload(&bytes)?;
            Ok(FetchOutcome {
                snapshot: snap,
                stale: false,
                last_error: None,
                cache_age: Some(Duration::ZERO),
            })
        }
        Err(e) if e.is_transient() => fallback_silent(cache),
        Err(AppError::Http { status, body }) => {
            cache.mark_stale();
            let last_error = Some(cache.write_last_error(status, &body));
            fallback_with_error(cache, last_error)
        }
        Err(e) => {
            cache.mark_stale();
            let last_error = Some(cache.write_last_error(0, &e.to_string()));
            fallback_with_error(cache, last_error)
        }
    }
}

fn fallback_silent(cache: &Cache) -> Result<FetchOutcome> {
    let Some(bytes) = cache.fallback_payload(MAX_STALE)? else {
        return Err(AppError::Transport(
            "copilot: no cache and network unreachable".into(),
        ));
    };
    reuse_cache(bytes, cache, true)
}

fn fallback_with_error(cache: &Cache, last_error: Option<(u16, String)>) -> Result<FetchOutcome> {
    let Some(bytes) = cache.fallback_payload(MAX_STALE)? else {
        return Err(AppError::Other("copilot: no usable cache".into()));
    };
    let mut outcome = reuse_cache(bytes, cache, true)?;
    outcome.last_error = last_error;
    Ok(outcome)
}

fn reuse_cache(bytes: Vec<u8>, cache: &Cache, stale: bool) -> Result<FetchOutcome> {
    Ok(FetchOutcome {
        snapshot: parse_cache(&bytes)?,
        stale,
        last_error: cache.read_last_error(),
        cache_age: cache.payload_age(),
    })
}

/// A half-written payload must be refetched, not rendered as a full balance.
fn parse_cache(bytes: &[u8]) -> Result<CopilotSnapshot> {
    let v: serde_json::Value = serde_json::from_slice(bytes)?;
    let missing = |name: &str| AppError::Schema(format!("copilot cache missing '{name}'"));
    let number = |name: &str| -> Result<f64> {
        v[name]
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| missing(name))
    };
    let reset_at = match v["reset_at"].as_str() {
        Some(text) => Some(
            DateTime::parse_from_rfc3339(text)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|_| AppError::Schema("copilot cache 'reset_at' is not RFC 3339".into()))?,
        ),
        None => None,
    };

    Ok(CopilotSnapshot {
        plan: v["plan"]
            .as_str()
            .ok_or_else(|| missing("plan"))?
            .to_string(),
        account: v["account"].as_str().unwrap_or_default().to_string(),
        premium_pct: v["premium_pct"]
            .as_i64()
            .filter(|p| (0..=100).contains(p))
            .ok_or_else(|| missing("premium_pct"))? as i32,
        entitlement: number("entitlement")?,
        used: number("used")?,
        remaining: number("remaining")?,
        unlimited: v["unlimited"]
            .as_bool()
            .ok_or_else(|| missing("unlimited"))?,
        overage_count: number("overage_count")?,
        overage_permitted: v["overage_permitted"]
            .as_bool()
            .ok_or_else(|| missing("overage_permitted"))?,
        reset_at,
    })
}

fn snap_to_json(snap: &CopilotSnapshot) -> serde_json::Value {
    serde_json::json!({
        "plan": snap.plan,
        "account": snap.account,
        "premium_pct": snap.premium_pct,
        "entitlement": snap.entitlement,
        "used": snap.used,
        "remaining": snap.remaining,
        "unlimited": snap.unlimited,
        "overage_count": snap.overage_count,
        "overage_permitted": snap.overage_permitted,
        "reset_at": snap.reset_at.map(|dt| dt.to_rfc3339()),
    })
}

async fn fetch_live(client: &reqwest::Client, url: &str, token: &str) -> Result<CopilotSnapshot> {
    let resp = tokio::time::timeout(
        HTTP_TIMEOUT,
        client
            .get(url)
            .header("Authorization", format!("token {token}"))
            .header("Accept", "application/json")
            .header("Editor-Version", EDITOR_VERSION)
            .header("User-Agent", USER_AGENT)
            .send(),
    )
    .await
    .map_err(|_| AppError::Transport(format!("copilot timeout: {url}")))??;

    let status = resp.status();
    let bytes = crate::vendor::read_body_capped(resp, crate::vendor::MAX_BODY_BYTES).await?;

    if !status.is_success() {
        let body = String::from_utf8_lossy(&bytes).chars().take(200).collect();
        return Err(AppError::Http {
            status: status.as_u16(),
            body,
        });
    }

    let user: CopilotUser = serde_json::from_slice(&bytes).map_err(|_| {
        AppError::Schema("copilot user response does not match the expected schema".into())
    })?;
    super::types::to_snapshot(user)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const LIVE_BODY: &str = r#"{
        "login": "octocat",
        "copilot_plan": "individual",
        "quota_reset_date_utc": "2026-10-01T00:00:00.000Z",
        "quota_snapshots": {
            "premium_interactions": {
                "entitlement": 200, "quota_remaining": 198.6, "remaining": 198,
                "percent_remaining": 99.3, "unlimited": false,
                "overage_count": 0, "overage_permitted": false
            }
        }
    }"#;

    fn cache_fixture() -> (TempDir, Cache) {
        let td = TempDir::new().unwrap();
        let cache = Cache::at(td.path().join("copilot"));
        cache.ensure_dir().unwrap();
        (td, cache)
    }

    fn endpoints(server: &mockito::Server) -> Endpoints {
        Endpoints {
            user: format!("{}/copilot_internal/user", server.url()),
        }
    }

    #[tokio::test]
    async fn live_200_returns_snapshot() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/copilot_internal/user")
            .match_header("authorization", "token gho_test")
            // Without a User-Agent api.github.com answers 403, which surfaces
            // as "credentials rejected" and sends you chasing the wrong bug.
            .match_header("user-agent", USER_AGENT)
            .match_header("editor-version", EDITOR_VERSION)
            .with_status(200)
            .with_body(LIVE_BODY)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let out = fetch_snapshot(
            &reqwest::Client::new(),
            "gho_test",
            &cache,
            &endpoints(&server),
            Duration::from_secs(0),
        )
        .await
        .unwrap();

        assert_eq!(out.snapshot.premium_pct, 1);
        assert_eq!(out.snapshot.entitlement, 200.0);
        assert_eq!(out.snapshot.plan, "Copilot Pro");
        assert!(!out.stale);
    }

    #[tokio::test]
    async fn a_cached_snapshot_round_trips_through_disk() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/copilot_internal/user")
            .with_status(200)
            .with_body(LIVE_BODY)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let endpoints = endpoints(&server);
        let live = fetch_snapshot(
            &client,
            "gho_test",
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        // A long TTL must be served from the cache written above.
        let cached = fetch_snapshot(
            &client,
            "gho_test",
            &cache,
            &endpoints,
            Duration::from_secs(600),
        )
        .await
        .unwrap();

        assert_eq!(cached.snapshot, live.snapshot);
        assert!(!cached.stale);
    }

    #[tokio::test]
    async fn a_401_body_never_reaches_the_outcome() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/copilot_internal/user")
            .with_status(401)
            .with_body("bad credentials gho_leaked_value")
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let warm = serde_json::json!({
            "plan": "Copilot Pro", "account": "octocat", "premium_pct": 5,
            "entitlement": 200.0, "used": 10.0, "remaining": 190.0,
            "unlimited": false, "overage_count": 0.0, "overage_permitted": false,
            "reset_at": "2026-10-01T00:00:00+00:00"
        });
        cache.write_payload(warm.to_string().as_bytes()).unwrap();

        let out = fetch_snapshot(
            &reqwest::Client::new(),
            "bad-token",
            &cache,
            &endpoints(&server),
            Duration::from_secs(0),
        )
        .await
        .unwrap();

        let (code, msg) = out.last_error.expect("the 401 must still be reported");
        assert_eq!(code, 401);
        assert_eq!(msg, crate::error::AUTH_FAILURE_MESSAGE);
        assert!(!msg.contains("gho_leaked_value"), "{msg}");
        assert!(out.stale);
        assert_eq!(out.snapshot.premium_pct, 5);
    }

    #[tokio::test]
    async fn a_malformed_success_does_not_echo_the_response() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/copilot_internal/user")
            .with_status(200)
            .with_body(
                r#"{"quota_snapshots":{"premium_interactions":{"entitlement":"sensitive-value"}}}"#,
            )
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let error = fetch_snapshot(
            &reqwest::Client::new(),
            "gho_test",
            &cache,
            &endpoints(&server),
            Duration::from_secs(0),
        )
        .await
        .unwrap_err()
        .to_string();

        assert!(!error.contains("sensitive-value"), "{error}");
    }

    #[test]
    fn a_truncated_cache_is_rejected_rather_than_rendered() {
        assert!(parse_cache(br#"{"plan":"Copilot Pro"}"#).is_err());
        assert!(parse_cache(br#"{"plan":"x","premium_pct":140}"#).is_err());
    }
}
