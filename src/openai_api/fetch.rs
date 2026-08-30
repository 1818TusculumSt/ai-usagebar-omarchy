//! OpenAI Admin API fetch — the Costs API
//! (`GET /v1/organization/costs`) reports daily billed dollars grouped by
//! line item, under the shared cache + flock primitives. Auth is a platform
//! **Admin key** (`Authorization: Bearer`); regular project `sk-` keys are
//! rejected by these organization endpoints. There is no balance endpoint —
//! spend against a self-configured limit is the whole story, exactly like
//! the Anthropic Admin vendor.

use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::cache::{Cache, MAX_STALE, acquire_lock_async};
use crate::error::{AppError, Result};
use crate::usage::finite_amount;

use super::types::{CostsPage, bucket_date};

pub const BASE_URL: &str = "https://api.openai.com";
/// Days of billed history to sum — a rolling month, matching the period the
/// platform dashboard shows by default.
pub const WINDOW_DAYS: i64 = 30;
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);
/// 30 day-buckets × every line item is a few hundred rows; 6 pages of 100
/// covers it while bounding a runaway cursor loop.
const MAX_PAGES: usize = 6;
const PAGE_LIMIT: usize = 100;

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub costs: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            costs: format!("{BASE_URL}/v1/organization/costs"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub snapshot: crate::usage::OpenAiApiSnapshot,
    pub stale: bool,
    pub last_error: Option<(u16, String)>,
    pub cache_age: Option<Duration>,
}

/// The aggregate carried out of one (or a cached) fetch.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct CostAggregate {
    pub spent: f64,
    /// Day → spend, ascending.
    pub by_day: Vec<(chrono::NaiveDate, f64)>,
    /// Line item → spend over the window, largest first (top 8 kept).
    pub top_items: Vec<(String, f64)>,
}

fn validate_limit(limit: Option<f64>) -> Result<Option<f64>> {
    if let Some(value) = limit
        && (!value.is_finite() || value <= 0.0)
    {
        return Err(AppError::Schema(
            "openai-api monthly_limit must be finite and greater than zero; \
             remove it to show spend without a limit"
                .into(),
        ));
    }
    Ok(limit)
}

/// First second of the window, unix seconds — the exact `start_time` sent and
/// the cache's window identity: a payload written for an earlier start is a
/// DIFFERENT window, not a stale version of this one.
fn window_start(now: DateTime<Utc>) -> i64 {
    (now - chrono::Duration::days(WINDOW_DAYS)).timestamp()
}

/// Fingerprint of the Admin key — the cache's change detector, never a
/// display value (same trick as the Anthropic Admin vendor).
fn target_key(admin_key: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    admin_key.hash(&mut hasher);
    format!("key:{:016x}", hasher.finish())
}

pub async fn fetch_snapshot(
    client: &reqwest::Client,
    admin_key: &str,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
    limit: Option<f64>,
) -> Result<FetchOutcome> {
    fetch_snapshot_at(client, admin_key, cache, endpoints, cache_ttl, limit, Utc::now()).await
}

pub async fn fetch_snapshot_at(
    client: &reqwest::Client,
    admin_key: &str,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
    limit: Option<f64>,
    now: DateTime<Utc>,
) -> Result<FetchOutcome> {
    let limit = validate_limit(limit)?;
    cache.ensure_dir()?;
    let _lock = acquire_lock_async(&cache.lock_path(), LOCK_TIMEOUT).await?;

    let start = window_start(now);
    let target = target_key(admin_key);

    if let Some(bytes) = cache.fresh_payload(cache_ttl)?
        && let Ok(outcome) = reuse_cache(&bytes, cache, false, limit, start, &target)
    {
        return Ok(outcome);
    }

    match fetch_live(client, endpoints, admin_key, start).await {
        Ok(aggregate) => {
            let bytes = serde_json::to_vec(&serde_json::json!({
                "start": start,
                "target": target,
                "aggregate": aggregate,
            }))?;
            cache.write_payload(&bytes)?;
            Ok(FetchOutcome {
                snapshot: snapshot_of(aggregate, limit),
                stale: false,
                last_error: None,
                cache_age: Some(Duration::ZERO),
            })
        }
        Err(e) if e.is_transient() => fallback(cache, None, limit, start, &target, e),
        Err(AppError::Http { status, body }) => {
            cache.mark_stale();
            let diag = cache.write_last_error(status, &body);
            fallback(
                cache,
                Some(diag),
                limit,
                start,
                &target,
                AppError::Http { status, body },
            )
        }
        Err(e) => {
            cache.mark_stale();
            let diag = cache.write_last_error(0, &e.to_string());
            fallback(cache, Some(diag), limit, start, &target, e)
        }
    }
}

fn snapshot_of(aggregate: CostAggregate, limit: Option<f64>) -> crate::usage::OpenAiApiSnapshot {
    crate::usage::OpenAiApiSnapshot {
        spent: aggregate.spent,
        by_day: aggregate.by_day,
        top_items: aggregate.top_items,
        limit,
    }
}

/// On failure, serve the last good aggregate with the error alongside it;
/// with nothing usable cached the ORIGINAL error reaches the caller, so a
/// first-run 401/403 keeps its Admin-key guidance.
fn fallback(
    cache: &Cache,
    last_error: Option<(u16, String)>,
    limit: Option<f64>,
    start: i64,
    target: &str,
    original: AppError,
) -> Result<FetchOutcome> {
    let Some(bytes) = cache.fallback_payload(MAX_STALE)? else {
        return Err(original);
    };
    // The limit always comes from the current config, so editing it takes
    // effect without a refetch.
    match reuse_cache(&bytes, cache, true, limit, start, target) {
        Ok(mut outcome) => {
            outcome.last_error = last_error;
            Ok(outcome)
        }
        Err(_) => Err(original),
    }
}

fn reuse_cache(
    bytes: &[u8],
    cache: &Cache,
    stale: bool,
    limit: Option<f64>,
    start: i64,
    target: &str,
) -> Result<FetchOutcome> {
    let v: serde_json::Value = serde_json::from_slice(bytes)?;
    if v.get("start").and_then(serde_json::Value::as_i64) != Some(start) {
        return Err(AppError::Schema(
            "openai-api cache is for a different window; refetching".into(),
        ));
    }
    if v.get("target").and_then(serde_json::Value::as_str) != Some(target) {
        return Err(AppError::Schema(
            "openai-api cache belongs to a different Admin key; refetching".into(),
        ));
    }
    let aggregate: CostAggregate = serde_json::from_value(
        v.get("aggregate")
            .cloned()
            .ok_or_else(|| AppError::Schema("openai-api cache missing 'aggregate'".into()))?,
    )?;
    finite_amount("openai-api cache", "spent", aggregate.spent)?;
    Ok(FetchOutcome {
        snapshot: snapshot_of(aggregate, limit),
        stale,
        last_error: cache.read_last_error(),
        cache_age: cache.payload_age(),
    })
}

async fn fetch_live(
    client: &reqwest::Client,
    endpoints: &Endpoints,
    admin_key: &str,
    start: i64,
) -> Result<CostAggregate> {
    let mut aggregate = CostAggregate::default();
    let mut page: Option<String> = None;
    let mut seen_pages: Vec<String> = Vec::new();

    for _ in 0..MAX_PAGES {
        let mut req = client
            .get(&endpoints.costs)
            .header("Authorization", format!("Bearer {admin_key}"))
            .query(&[
                ("start_time", start.to_string().as_str()),
                ("bucket_width", "1d"),
                ("group_by[]", "line_item"),
                ("limit", PAGE_LIMIT.to_string().as_str()),
            ]);
        if let Some(p) = &page {
            req = req.query(&[("page", p.as_str())]);
        }

        let resp = tokio::time::timeout(HTTP_TIMEOUT, req.send())
            .await
            .map_err(|_| {
                AppError::Transport(format!("openai-api timeout: {}", endpoints.costs))
            })??;

        let status = resp.status();
        let bytes = crate::vendor::read_body_capped(resp, crate::vendor::MAX_BODY_BYTES).await?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes).chars().take(200).collect();
            return Err(AppError::Http {
                status: status.as_u16(),
                body,
            });
        }

        let costs: CostsPage = serde_json::from_slice(&bytes)
            .map_err(|e| AppError::Schema(format!("openai-api costs: {e}")))?;
        absorb(&costs, &mut aggregate)?;

        // A partial window is indistinguishable from a genuinely smaller
        // spend once cached, so pagination anomalies are errors, never an
        // early return with whatever was summed so far.
        match (costs.has_next_page, costs.next_page) {
            (false, _) => {
                finalize(&mut aggregate)?;
                return Ok(aggregate);
            }
            (true, None) => {
                return Err(AppError::Schema(
                    "openai-api costs: has_next_page without next_page; refusing a partial window"
                        .into(),
                ));
            }
            (true, Some(p)) if p.trim().is_empty() => {
                return Err(AppError::Schema(
                    "openai-api costs: empty next_page; refusing a partial window".into(),
                ));
            }
            (true, Some(p)) => {
                if seen_pages.contains(&p) {
                    return Err(AppError::Schema(format!(
                        "openai-api costs: pagination repeated cursor {p:?}; \
                         refusing a partial window"
                    )));
                }
                seen_pages.push(p.clone());
                page = Some(p);
            }
        }
    }
    Err(AppError::Schema(format!(
        "openai-api costs: more than {MAX_PAGES} pages; refusing a partial window"
    )))
}

/// Merge one page into the running aggregate. Tolerant by design: rows with
/// no amount, no date, a null line item (fees, or a request that lost its
/// group), or a non-USD currency are skipped rather than failing the window.
fn absorb(page: &CostsPage, aggregate: &mut CostAggregate) -> Result<()> {
    for row in &page.data {
        let Some(amount) = &row.amount else { continue };
        let Some(value) = amount.value else { continue };
        if !value.is_finite() {
            continue;
        }
        if let Some(currency) = &amount.currency
            && currency != "USD"
        {
            continue;
        }
        let value = crate::usage::finite_amount("openai-api", "costs amount", value)?;
        let Some(date) = bucket_date(row.start_time) else { continue };
        aggregate.spent = finite_amount(
            "openai-api",
            "costs running total",
            aggregate.spent + value,
        )?;
        let item = row
            .line_item
            .as_ref()
            .and_then(|li| li.name.clone())
            .unwrap_or_else(|| "other".into());
        upsert_day(&mut aggregate.by_day, date, value);
        upsert_item(&mut aggregate.top_items, item, value);
    }
    Ok(())
}

fn upsert_day(rows: &mut Vec<(chrono::NaiveDate, f64)>, date: chrono::NaiveDate, value: f64) {
    match rows.iter_mut().find(|(d, _)| *d == date) {
        Some(entry) => entry.1 += value,
        None => rows.push((date, value)),
    }
}

fn upsert_item(rows: &mut Vec<(String, f64)>, name: String, value: f64) {
    match rows.iter_mut().find(|(k, _)| *k == name) {
        Some(entry) => entry.1 += value,
        None => rows.push((name, value)),
    }
}

/// Sort days ascending, items by spend descending, cap the item list.
fn finalize(aggregate: &mut CostAggregate) -> Result<()> {
    aggregate.by_day.sort_by_key(|(date, _)| *date);
    aggregate.top_items.sort_by(|a, b| b.1.total_cmp(&a.1));
    aggregate.top_items.truncate(8);
    finite_amount("openai-api", "window total", aggregate.spent)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absorb_is_tolerant_of_missing_and_foreign_fields() {
        let page: CostsPage = serde_json::from_str(
            r#"{"data": [
                {"start_time": 1754006400, "amount": {"value": 1.5, "currency": "USD"},
                 "line_item": {"name": "gpt-5.2"}},
                {"start_time": 1754092800, "amount": {"value": 2.0, "currency": "USD"},
                 "line_item": {"name": "gpt-5.2"}},
                {"start_time": 1754179200, "amount": {"value": 9.0, "currency": "EUR"},
                 "line_item": {"name": "eur-fee"}},
                {"start_time": 1754265600, "amount": {"value": null}, "line_item": null},
                {"start_time": 1754352000, "amount": null},
                {"start_time": 1754438400, "amount": {"value": 3.0, "currency": "USD"},
                 "line_item": null}
            ], "has_next_page": false}"#,
        )
        .unwrap();
        let mut aggregate = CostAggregate::default();
        absorb(&page, &mut aggregate).unwrap();
        finalize(&mut aggregate).unwrap();
        assert!((aggregate.spent - 6.5).abs() < 1e-9, "{:?}", aggregate.spent);
        assert_eq!(aggregate.by_day.len(), 3, "EUR and null-amount rows skipped");
        // Null line items bucket under "other"; gpt-5.2 leads by spend.
        assert_eq!(aggregate.top_items[0].0, "gpt-5.2");
        assert!((aggregate.top_items[0].1 - 3.5).abs() < 1e-9);
        assert!(aggregate.top_items.iter().any(|(k, _)| k == "other"));
    }
}
