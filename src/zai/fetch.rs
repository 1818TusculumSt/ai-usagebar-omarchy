//! Z.AI / BigModel fetch. Note the auth-header quirk — the API key is passed
//! as `Authorization: <KEY>` WITHOUT the `Bearer` prefix. Sending `Bearer …`
//! returns 401.
//!
//! Three request shapes share one response schema (see `types.rs`):
//! - **personal** — plain quota GET on the key's site (default `api.z.ai`);
//! - **team** — same path with `?type=2` plus the lowercase
//!   `bigmodel-organization` / `bigmodel-project` headers, exactly the
//!   combination cc-switch ships (its request-shape contract test pins it);
//!   team plans only exist on the BigModel CN site (`open.bigmodel.cn`);
//! - **usage-only** — a pay-as-you-go key has no subscription, so the quota
//!   endpoint answers "not subscribed" for it; the 7-day aggregate from the
//!   model-usage endpoint is all there is to show.

use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};

use crate::cache::{Cache, MAX_STALE, acquire_lock_async};
use crate::config::{ResolvedZaiAccount, ZaiAccountType, ZaiSite};
use crate::error::{AppError, Result};
use crate::usage::ZaiSnapshot;

use super::types::Envelope;

pub const QUOTA_URL: &str = "https://api.z.ai/api/monitor/usage/quota/limit";
const GLOBAL_BASE: &str = "https://api.z.ai";
const CN_BASE: &str = "https://open.bigmodel.cn";
const QUOTA_PATH: &str = "/api/monitor/usage/quota/limit";
const USAGE_STATS_PATH: &str = "/api/monitor/usage/model-usage";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);
/// The model-usage aggregate window for usage-only keys, matching the old
/// GNOME panel extension's supplementary stats line.
const STATS_WINDOW: chrono::Duration = chrono::Duration::days(7);

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub quota: String,
    /// 7-day aggregate stats endpoint (usage-only accounts).
    pub usage_stats: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self::for_site(ZaiSite::Global)
    }
}

impl Endpoints {
    pub fn for_site(site: ZaiSite) -> Self {
        let base = match site {
            ZaiSite::Global => GLOBAL_BASE,
            ZaiSite::Cn => CN_BASE,
        };
        Self {
            quota: format!("{base}{QUOTA_PATH}"),
            usage_stats: format!("{base}{USAGE_STATS_PATH}"),
        }
    }
}

/// One validated fetch plan: which key, billed how, on which site. Built by
/// [`crate::config::ZaiConfig::resolve_account`]; kept as its own struct so
/// the fetch layer never re-reads config itself.
pub type FetchPlan = ResolvedZaiAccount;

#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub snapshot: ZaiSnapshot,
    pub stale: bool,
    pub last_error: Option<(u16, String)>,
    pub cache_age: Option<Duration>,
}

/// Historical single-key entry point: a personal plan on the default site.
pub async fn fetch_snapshot(
    client: &reqwest::Client,
    api_key: &str,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
    config_plan_tier: Option<&str>,
) -> Result<FetchOutcome> {
    let plan = FetchPlan {
        api_key: api_key.to_string(),
        account_type: ZaiAccountType::Personal,
        site: ZaiSite::Global,
        organization_id: None,
        project_id: None,
        plan_tier: config_plan_tier.map(str::to_string),
    };
    fetch_snapshot_plan(client, &plan, cache, endpoints, cache_ttl, Utc::now()).await
}

/// Fetch one account's usage according to its plan. Cache-first for every
/// account type; the payload bytes are shared across types because the wire
/// schema is.
pub async fn fetch_snapshot_plan(
    client: &reqwest::Client,
    plan: &FetchPlan,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
    now: DateTime<Utc>,
) -> Result<FetchOutcome> {
    // Validate the team scope before touching cache or network: a
    // half-configured team entry is a configuration error, not a fetch
    // failure, and must not be swallowed by the stale-cache fallback below.
    if plan.account_type == ZaiAccountType::Team
        && (plan.organization_id.as_deref().unwrap_or("").is_empty()
            || plan.project_id.as_deref().unwrap_or("").is_empty())
    {
        return Err(AppError::Credentials(
            "zai: team plan needs both the organization id and the project id".into(),
        ));
    }
    cache.ensure_dir()?;
    let _lock = acquire_lock_async(&cache.lock_path(), LOCK_TIMEOUT).await?;

    if let Some(bytes) = cache.fresh_payload(cache_ttl)?
        && let Ok(outcome) = reuse(bytes, cache, false, plan)
    {
        return Ok(outcome);
    }
    // Corrupt fresh cache: fall through to live fetch rather than return a
    // fabricated "GLM Coding Unknown" snapshot with empty windows.

    let live = match plan.account_type {
        ZaiAccountType::Personal => {
            match fetch_quota(client, endpoints, plan, None).await {
                Ok(ok) => Ok(ok),
                // A key the gateway answers "no coding plan" for is either a
                // pay-as-you-go key (the old GNOME panel degraded those to
                // the 7-day usage stats) or a team key queried without its
                // org scope. Try the stats degrade; if the monitor gateway
                // rejects those too, the key carries no personal plan at
                // all and the remedy says what to configure.
                Err(e) if is_no_coding_plan(&e) => {
                    match fetch_usage_stats(client, endpoints, plan, now).await {
                        Ok(ok) => Ok(ok),
                        Err(stats_err) if is_no_coding_plan(&stats_err) => {
                            // Compose from the API's own message rather than
                            // the formatted error — nesting `e` here would
                            // double the "schema mismatch:" prefix on Display.
                            let api = match &e {
                                AppError::Schema(m) => m.clone(),
                                other => other.to_string(),
                            };
                            Err(AppError::Schema(format!(
                                "{api}. Every monitor endpoint denies this key a personal \
                                 coding plan, so it is either a pay-as-you-go key with no \
                                 usage to report, or a team/enterprise key: under [zai] (or a \
                                 [[zai.accounts]] entry) set account_type = \"team\" with \
                                 organization_id and project_id (bigmodel.cn console → F12 → \
                                 Application → Local Storage → Bigmodel-Organization / \
                                 Bigmodel-Project)."
                            )))
                        }
                        Err(_) => Err(e),
                    }
                }
                Err(e) => Err(e),
            }
        }
        ZaiAccountType::Team => {
            // Unreachable for config-sourced plans (validated above and in
            // `ZaiConfig::resolve_account`); kept for hand-built fetch plans.
            let (org, project) = (
                plan.organization_id.as_deref().unwrap_or_default(),
                plan.project_id.as_deref().unwrap_or_default(),
            );
            fetch_quota(client, endpoints, plan, Some((org, project))).await
        }
        ZaiAccountType::Usage => fetch_usage_stats(client, endpoints, plan, now).await,
    };

    match live {
        Ok((bytes, snapshot)) => {
            // Only a validated payload reaches the cache, so a 200 carrying
            // `success: false` can never overwrite the last good payload nor
            // clear the recorded error.
            cache.write_payload(&bytes)?;
            Ok(FetchOutcome {
                snapshot,
                stale: false,
                last_error: None,
                cache_age: Some(Duration::ZERO),
            })
        }
        Err(e) if e.is_transient() => fallback_silent(cache, plan, e),
        Err(AppError::Http { status, body }) => {
            cache.mark_stale();
            let last_error = Some(cache.write_last_error(status, &body));
            fallback_with_error(
                cache,
                last_error,
                plan,
                AppError::Http { status, body },
            )
        }
        Err(e) => {
            cache.mark_stale();
            let last_error = Some(cache.write_last_error(0, &e.to_string()));
            fallback_with_error(cache, last_error, plan, e)
        }
    }
}

/// The gateway's "this key has no coding plan" marker, in the in-band
/// failure message. Observed live (2026-08-29): `当前用户不存在coding plan`.
/// Matched on the substring so the English variant ("no coding plan
/// subscribed") and future phrasings ride along; auth failures never carry it.
fn is_no_coding_plan(e: &AppError) -> bool {
    let msg = match e {
        AppError::Schema(m) => m,
        AppError::Http { body, .. } => body,
        _ => return false,
    };
    msg.to_lowercase().contains("coding plan")
}

fn reuse(bytes: Vec<u8>, cache: &Cache, stale: bool, plan: &FetchPlan) -> Result<FetchOutcome> {
    let env: Envelope = serde_json::from_slice(&bytes)?;
    // A cached failure envelope is not usage data, even if it parses.
    env.check_ok()?;
    Ok(FetchOutcome {
        snapshot: env.into_snapshot_for(plan),
        stale,
        last_error: cache.read_last_error(),
        cache_age: cache.payload_age(),
    })
}

fn fallback_silent(cache: &Cache, plan: &FetchPlan, original: AppError) -> Result<FetchOutcome> {
    let Some(bytes) = cache.fallback_payload(MAX_STALE)? else {
        return Err(original);
    };
    reuse(bytes, cache, true, plan)
}

/// Serve the last good cache flagged stale with the live error recorded.
/// With nothing cached, the ORIGINAL error surfaces — "no usable cache"
/// swallowed the actual API message (the live `当前用户不存在coding plan`
/// was only ever visible in `.last_error`), which is how a config problem
/// masqueraded as a cache bug. Mirrors kimi's fallback semantics.
fn fallback_with_error(
    cache: &Cache,
    last_error: Option<(u16, String)>,
    plan: &FetchPlan,
    original: AppError,
) -> Result<FetchOutcome> {
    let Some(bytes) = cache.fallback_payload(MAX_STALE)? else {
        return Err(original);
    };
    let mut out = reuse(bytes, cache, true, plan)?;
    out.last_error = last_error;
    Ok(out)
}

/// Quota fetch (personal + team). `team` carries the `(organization, project)`
/// pair: with it the request goes out as `?type=2` with the org headers, the
/// shape cc-switch verified against the BigModel gateway (Bearer +
/// capitalized headers returned an empty `data` object).
async fn fetch_quota(
    client: &reqwest::Client,
    endpoints: &Endpoints,
    plan: &FetchPlan,
    team: Option<(&str, &str)>,
) -> Result<(Vec<u8>, ZaiSnapshot)> {
    let url = match team {
        Some(_) => format!("{}?type=2", endpoints.quota),
        None => endpoints.quota.clone(),
    };
    let mut request = client
        .get(&url)
        // NO `Bearer ` prefix — on either site.
        .header("Authorization", &plan.api_key)
        .header("Accept-Language", "en-US,en")
        .header("Content-Type", "application/json");
    if let Some((organization, project)) = team {
        // Lowercase spellings, exactly as cc-switch sends them and its
        // contract test asserts.
        request = request
            .header("bigmodel-organization", organization)
            .header("bigmodel-project", project);
    }
    let (bytes, env) = send_validated(request, &url).await?;
    let snapshot = env.into_snapshot_for(plan);
    Ok((bytes, snapshot))
}

/// 7-day aggregate stats for usage-only keys. The endpoint wants local-time
/// `YYYY-MM-DD HH:mm:ss` boundaries (the format the bigmodel web console's
/// trackers send).
async fn fetch_usage_stats(
    client: &reqwest::Client,
    endpoints: &Endpoints,
    plan: &FetchPlan,
    now: DateTime<Utc>,
) -> Result<(Vec<u8>, ZaiSnapshot)> {
    let start = now - STATS_WINDOW;
    let url = format!(
        "{}?startTime={}&endTime={}",
        endpoints.usage_stats,
        chrono::Local
            .from_utc_datetime(&start.naive_utc())
            .format("%Y-%m-%d %H:%M:%S"),
        chrono::Local
            .from_utc_datetime(&now.naive_utc())
            .format("%Y-%m-%d %H:%M:%S"),
    );
    let request = client
        .get(&url)
        .header("Authorization", &plan.api_key)
        .header("Accept-Language", "en-US,en")
        .header("Content-Type", "application/json");
    let (bytes, env) = send_validated(request, &url).await?;
    // For a usage-only key the stats ARE the data — an envelope without them
    // is drift, not "no usage", and must not be cached as authoritative.
    if env.data.as_ref().and_then(|d| d.total_usage).is_none() {
        return Err(AppError::Schema(
            "zai: model-usage response carried no totalUsage".into(),
        ));
    }
    let snapshot = env.usage_stats_into_snapshot(plan);
    Ok((bytes, snapshot))
}

/// Shared request tail: bounded timeout, capped body, HTTP-status and
/// in-band `success: false` checks. Returns the raw bytes (for the cache)
/// alongside the *validated* envelope, so the caller cannot accidentally
/// cache a body it never checked.
async fn send_validated(
    request: reqwest::RequestBuilder,
    url: &str,
) -> Result<(Vec<u8>, Envelope)> {
    let resp = tokio::time::timeout(HTTP_TIMEOUT, request.send())
        .await
        .map_err(|_| AppError::Transport(format!("zai timeout: {url}")))??;

    let status = resp.status();
    let bytes = crate::vendor::read_body_capped(resp, crate::vendor::MAX_BODY_BYTES).await?;

    if !status.is_success() {
        let body = String::from_utf8_lossy(&bytes).chars().take(200).collect();
        return Err(AppError::Http {
            status: status.as_u16(),
            body,
        });
    }

    // Schema drift surfaces here — and so does Z.AI's in-band failure shape,
    // which arrives as HTTP 200 with `success: false`. Both are errors, so the
    // caller's fallback path runs and the good cache survives.
    let env: Envelope = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Schema(format!("zai quota response: {e}")))?;
    env.check_ok()?;
    Ok((bytes, env))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ZaiAccount, ZaiConfig};
    use tempfile::TempDir;

    fn cache_fixture() -> (TempDir, Cache) {
        let td = TempDir::new().unwrap();
        let cache = Cache::at(td.path().join("zai"));
        cache.ensure_dir().unwrap();
        (td, cache)
    }

    fn endpoints_at(server: &mockito::Server) -> Endpoints {
        let url = server.url();
        Endpoints {
            quota: format!("{url}/api/monitor/usage/quota/limit"),
            usage_stats: format!("{url}/api/monitor/usage/model-usage"),
        }
    }

    fn personal_plan() -> FetchPlan {
        FetchPlan {
            api_key: "k".into(),
            account_type: ZaiAccountType::Personal,
            site: ZaiSite::Global,
            organization_id: None,
            project_id: None,
            plan_tier: None,
        }
    }

    const GOOD_BODY: &str = r#"{"code":200,"msg":"Operation successful","data":{
        "limits":[{"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":42}],
        "level":"pro"},"success":true}"#;

    async fn team_request_shape(server: &mut mockito::Server) {
        server
            .mock("GET", "/api/monitor/usage/quota/limit")
            .match_query(mockito::Matcher::Exact("type=2".into()))
            .match_header("authorization", "k")
            .match_header("bigmodel-organization", "org-1")
            .match_header("bigmodel-project", "proj-1")
            .with_status(200)
            .with_body(GOOD_BODY)
            .create_async()
            .await;
    }

    #[tokio::test]
    async fn team_request_carries_type2_and_org_project_headers() {
        // The contract cc-switch's own test pins: ?type=2, bare token, both
        // lowercase org headers. Bearer + capitalized headers verifiably
        // returned an empty data object against the live gateway.
        let mut server = mockito::Server::new_async().await;
        team_request_shape(&mut server).await;

        let (_td, cache) = cache_fixture();
        let plan = FetchPlan {
            account_type: ZaiAccountType::Team,
            site: ZaiSite::Cn,
            organization_id: Some("org-1".into()),
            project_id: Some("proj-1".into()),
            ..personal_plan()
        };
        let out = fetch_snapshot_plan(
            &reqwest::Client::new(),
            &plan,
            &cache,
            &endpoints_at(&server),
            Duration::from_secs(0),
            Utc::now(),
        )
        .await
        .unwrap();
        assert_eq!(out.snapshot.session.as_ref().unwrap().utilization_pct, 42);
    }

    #[tokio::test]
    async fn a_half_configured_team_plan_fails_loudly() {
        // Never silently degrade to a personal query — that answers "not
        // subscribed" for a team key and looks like an auth error.
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", mockito::Matcher::Any)
            .expect(0)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let plan = FetchPlan {
            account_type: ZaiAccountType::Team,
            organization_id: None,
            project_id: Some("proj-1".into()),
            ..personal_plan()
        };
        let err = fetch_snapshot_plan(
            &reqwest::Client::new(),
            &plan,
            &cache,
            &endpoints_at(&server),
            Duration::from_secs(0),
            Utc::now(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("team plan"), "{err}");
    }

    #[tokio::test]
    async fn usage_only_account_reads_seven_day_stats_and_skips_quota() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/monitor/usage/quota/limit")
            .expect(0)
            .create_async()
            .await;
        let stats = server
            .mock("GET", "/api/monitor/usage/model-usage")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(
                r#"{"code":200,"msg":"ok","data":{"totalUsage":{
                    "totalModelCallCount":12,"totalTokensUsage":3456789}},"success":true}"#,
            )
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let plan = FetchPlan {
            account_type: ZaiAccountType::Usage,
            site: ZaiSite::Cn,
            ..personal_plan()
        };
        let out = fetch_snapshot_plan(
            &reqwest::Client::new(),
            &plan,
            &cache,
            &endpoints_at(&server),
            Duration::from_secs(0),
            Utc::now(),
        )
        .await
        .unwrap();
        assert!(out.snapshot.session.is_none());
        assert!(out.snapshot.weekly.is_none());
        let stats_snapshot = out.snapshot.usage_stats.unwrap();
        assert_eq!(stats_snapshot.prompts, 12);
        assert_eq!(stats_snapshot.tokens, 3_456_789);
        stats.assert_async().await;
    }

    /// The live scenario that motivated the degrade: a real pay-as-you-go
    /// key under a bare `[zai]` config answered the personal quota query
    /// with `当前用户不存在coding plan` and the widget showed a useless
    /// "no usable cache". Now it shows the 7-day stats.
    #[tokio::test]
    async fn personal_key_without_coding_plan_degrades_to_seven_day_stats() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/monitor/usage/quota/limit")
            .with_status(200)
            .with_body(
                r#"{"code":500,"msg":"当前用户不存在coding plan","data":null,"success":false}"#,
            )
            .create_async()
            .await;
        server
            .mock("GET", "/api/monitor/usage/model-usage")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(
                r#"{"code":200,"msg":"ok","data":{"totalUsage":{
                    "totalModelCallCount":7,"totalTokensUsage":424242}},"success":true}"#,
            )
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let out = fetch_snapshot_plan(
            &reqwest::Client::new(),
            &personal_plan(),
            &cache,
            &endpoints_at(&server),
            Duration::from_secs(0),
            Utc::now(),
        )
        .await
        .unwrap();
        assert!(
            out.snapshot.usage_stats.is_some(),
            "expected degraded stats, got {:?}",
            out.snapshot
        );
        assert_eq!(out.snapshot.usage_stats.unwrap().prompts, 7);
        assert!(out.snapshot.session.is_none());
    }

    /// An auth failure must NOT degrade — a bad key reading "usage stats"
    /// would look like a working account.
    #[tokio::test]
    async fn auth_failure_does_not_degrade_to_usage_stats() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/monitor/usage/quota/limit")
            .with_status(200)
            .with_body(r#"{"code":401,"msg":"Unauthorized","data":null,"success":false}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/monitor/usage/model-usage")
            .expect(0)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let err = fetch_snapshot_plan(
            &reqwest::Client::new(),
            &personal_plan(),
            &cache,
            &endpoints_at(&server),
            Duration::from_secs(0),
            Utc::now(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("Unauthorized"), "{err}");
    }

    /// If the stats query also fails, the quota error is the one that
    /// matters — it names the actual account problem.
    #[tokio::test]
    async fn degrade_failure_keeps_the_quota_error() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/monitor/usage/quota/limit")
            .with_status(200)
            .with_body(
                r#"{"code":500,"msg":"当前用户不存在coding plan","data":null,"success":false}"#,
            )
            .create_async()
            .await;
        server
            .mock("GET", "/api/monitor/usage/model-usage")
            .match_query(mockito::Matcher::Any)
            .with_status(500)
            .with_body("boom")
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let err = fetch_snapshot_plan(
            &reqwest::Client::new(),
            &personal_plan(),
            &cache,
            &endpoints_at(&server),
            Duration::from_secs(0),
            Utc::now(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("coding plan"), "{err}");
    }

    /// When BOTH endpoints answer "no coding plan" — the live signature of a
    /// team key or a PAAS key with no plan — the error keeps the API message
    /// and appends the team-config remedy.
    #[tokio::test]
    async fn no_plan_on_both_endpoints_appends_the_team_remedy() {
        let mut server = mockito::Server::new_async().await;
        for path in ["/api/monitor/usage/quota/limit", "/api/monitor/usage/model-usage"] {
            server
                .mock("GET", path)
                .match_query(mockito::Matcher::Any)
                .with_status(200)
                .with_body(
                    r#"{"code":500,"msg":"当前用户不存在coding plan","data":null,"success":false}"#,
                )
                .create_async()
                .await;
        }

        let (_td, cache) = cache_fixture();
        let err = fetch_snapshot_plan(
            &reqwest::Client::new(),
            &personal_plan(),
            &cache,
            &endpoints_at(&server),
            Duration::from_secs(0),
            Utc::now(),
        )
        .await
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("当前用户不存在coding plan"), "{message}");
        assert!(message.contains("account_type = \"team\""), "{message}");
        assert!(message.contains("Bigmodel-Organization"), "{message}");
    }

    /// With no cache to fall back on, the API's own error surfaces — the
    /// old "zai: no usable cache" masked the real message.
    #[tokio::test]
    async fn http_failure_without_cache_surfaces_the_api_error_not_the_cache_state() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/monitor/usage/quota/limit")
            .with_status(403)
            .with_body(r#"{"code":403,"msg":"forbidden"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let err = fetch_snapshot_plan(
            &reqwest::Client::new(),
            &personal_plan(),
            &cache,
            &endpoints_at(&server),
            Duration::from_secs(0),
            Utc::now(),
        )
        .await
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("403") || message.contains("forbidden"), "{message}");
        assert!(!message.contains("no usable cache"), "{message}");
    }

    /// The degrade must survive the cache: the stats payload replays as
    /// stats even though the config still says `personal`.
    #[tokio::test]
    async fn degraded_stats_payload_replays_as_stats_from_cache() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/monitor/usage/quota/limit")
            .with_status(200)
            .with_body(
                r#"{"code":500,"msg":"当前用户不存在coding plan","data":null,"success":false}"#,
            )
            .create_async()
            .await;
        server
            .mock("GET", "/api/monitor/usage/model-usage")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_body(
                r#"{"code":200,"msg":"ok","data":{"totalUsage":{
                    "totalModelCallCount":9,"totalTokensUsage":999}},"success":true}"#,
            )
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let endpoints = endpoints_at(&server);
        let first = fetch_snapshot_plan(
            &reqwest::Client::new(),
            &personal_plan(),
            &cache,
            &endpoints,
            Duration::from_secs(0),
            Utc::now(),
        )
        .await
        .unwrap();
        assert!(first.snapshot.usage_stats.is_some());

        // Fresh-cache replay: same projection, no network.
        let replayed = fetch_snapshot_plan(
            &reqwest::Client::new(),
            &personal_plan(),
            &cache,
            &endpoints,
            Duration::from_secs(3600),
            Utc::now(),
        )
        .await
        .unwrap();
        assert!(
            replayed.snapshot.usage_stats.is_some(),
            "cache replay lost the stats projection: {:?}",
            replayed.snapshot
        );
        assert_eq!(replayed.snapshot.usage_stats.unwrap().prompts, 9);
    }

    #[test]
    fn config_resolves_team_accounts_with_the_cn_site_default() {
        let cfg = ZaiConfig {
            accounts: vec![ZaiAccount {
                label: "team".into(),
                api_key: Some("k".into()),
                api_key_env: None,
                plan_tier: None,
                account_type: ZaiAccountType::Team,
                organization_id: Some("org".into()),
                project_id: Some("proj".into()),
                site: None,
            }],
            ..ZaiConfig::default()
        };
        let resolved = cfg.resolve_account(Some("team")).unwrap();
        assert_eq!(resolved.site, ZaiSite::Cn);
        assert_eq!(resolved.account_type, ZaiAccountType::Team);

        // Personal keys keep the historical global host when unset, and an
        // inline key keeps this test off any ambient environment variable.
        let personal = ZaiConfig {
            api_key: Some("k".into()),
            ..ZaiConfig::default()
        };
        assert_eq!(
            personal.resolve_account(None).unwrap().site,
            ZaiSite::Global
        );
    }

    #[test]
    fn config_rejects_a_team_account_without_both_ids() {
        let cfg = ZaiConfig {
            api_key: Some("k".into()),
            account_type: ZaiAccountType::Team,
            organization_id: Some("org".into()),
            ..ZaiConfig::default()
        };
        let err = cfg.resolve_account(None).unwrap_err();
        assert!(err.to_string().contains("organization_id"), "{err}");
    }

    #[test]
    fn endpoints_pick_the_host_by_site() {
        assert_eq!(
            Endpoints::for_site(ZaiSite::Cn).quota,
            "https://open.bigmodel.cn/api/monitor/usage/quota/limit"
        );
        assert_eq!(
            Endpoints::for_site(ZaiSite::Global).quota,
            "https://api.z.ai/api/monitor/usage/quota/limit"
        );
        assert!(
            Endpoints::for_site(ZaiSite::Cn)
                .usage_stats
                .ends_with("/api/monitor/usage/model-usage")
        );
    }

    #[tokio::test]
    async fn in_band_failure_on_200_is_rejected_and_keeps_the_good_cache() {
        // The regression this guards: a 200 carrying `success:false` used to be
        // written to cache, clearing the recorded error, and rendered as an
        // unknown plan with empty windows — visually identical to "no usage".
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/monitor/usage/quota/limit")
            .with_status(200)
            .with_body(r#"{"code":401,"msg":"Unauthorized","data":null,"success":false}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        cache.write_payload(GOOD_BODY.as_bytes()).unwrap();

        let client = reqwest::Client::new();
        let out = fetch_snapshot(
            &client,
            "k",
            &cache,
            &endpoints_at(&server),
            Duration::from_secs(0),
            None,
        )
        .await
        .unwrap();

        // The last good figure is still shown, flagged stale...
        assert!(out.stale);
        assert_eq!(out.snapshot.plan, "GLM Coding Pro");
        // ...and the good payload was NOT overwritten by the failure envelope.
        let cached = String::from_utf8(cache.maybe_payload().unwrap().unwrap()).unwrap();
        assert!(cached.contains("\"success\":true"), "cache was clobbered");
    }

    #[tokio::test]
    async fn in_band_failure_with_no_cache_surfaces_the_error() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/monitor/usage/quota/limit")
            .with_status(200)
            .with_body(r#"{"code":500,"msg":"boom","data":null,"success":false}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let out = fetch_snapshot(
            &client,
            "k",
            &cache,
            &endpoints_at(&server),
            Duration::from_secs(0),
            None,
        )
        .await;
        assert!(out.is_err(), "expected an error, got {out:?}");
    }

    #[tokio::test]
    async fn corrupt_fresh_cache_refetches_instead_of_showing_unknown_plan() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/monitor/usage/quota/limit")
            .with_status(200)
            .with_body(GOOD_BODY)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        cache.write_payload(b"{ truncated").unwrap();

        let client = reqwest::Client::new();
        // Long TTL: the payload IS fresh, it is just unusable.
        let out = fetch_snapshot(
            &client,
            "k",
            &cache,
            &endpoints_at(&server),
            Duration::from_secs(3600),
            None,
        )
        .await
        .unwrap();
        assert_eq!(out.snapshot.plan, "GLM Coding Pro");
        assert!(!out.stale);
    }

    #[tokio::test]
    async fn live_200_parses_real_shape() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/monitor/usage/quota/limit")
            .with_status(200)
            .with_body(
                r#"{"code":200,"msg":"Operation successful","data":{
                    "limits":[
                        {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":42},
                        {"type":"TOKENS_LIMIT","unit":6,"number":1,"percentage":15,"nextResetTime":1779792169974}
                    ],"level":"pro"
                },"success":true}"#,
            )
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let out = fetch_snapshot(
            &client,
            "fake-key",
            &cache,
            &endpoints_at(&server),
            Duration::from_secs(0),
            None,
        )
        .await
        .unwrap();
        assert_eq!(out.snapshot.plan, "GLM Coding Pro");
        assert_eq!(out.snapshot.session.as_ref().unwrap().utilization_pct, 42);
        assert_eq!(out.snapshot.weekly.as_ref().unwrap().utilization_pct, 15);
    }

    #[tokio::test]
    async fn http_401_falls_back_to_cache_when_present() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/monitor/usage/quota/limit")
            .with_status(401)
            .with_body(r#"{"code":401,"msg":"Unauthorized"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let seed = r#"{"code":200,"data":{"limits":[
            {"type":"TOKENS_LIMIT","unit":3,"percentage":10}
        ],"level":"lite"},"success":true}"#;
        cache.write_payload(seed.as_bytes()).unwrap();

        let client = reqwest::Client::new();
        let out = fetch_snapshot(
            &client,
            "k",
            &cache,
            &endpoints_at(&server),
            Duration::from_secs(0),
            None,
        )
        .await
        .unwrap();
        assert!(out.stale);
        assert_eq!(out.snapshot.session.as_ref().unwrap().utilization_pct, 10);
        assert_eq!(out.last_error.as_ref().map(|(c, _)| *c), Some(401));
    }
}
