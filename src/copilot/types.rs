//! Strict wire types for `GET /copilot_internal/user`.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;

use crate::error::{AppError, Result};
use crate::usage::CopilotSnapshot;

const MAX_LABEL_CHARS: usize = 128;
/// Used-percent above 100 is the included pool spent, up to this ceiling.
/// Past it the payload is drift, not an overshoot.
const MAX_EXHAUSTED_PERCENT: f64 = 1_000.0;
/// Entitlements are request counts, not money; anything past this is drift.
const MAX_BENIGN_QUOTA: f64 = 1e9;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CopilotUser {
    pub login: Option<String>,
    pub copilot_plan: Option<String>,
    pub quota_snapshots: Option<QuotaSnapshots>,
    /// Full timestamp when present; `quota_reset_date` is the date-only form
    /// older responses carry.
    pub quota_reset_date_utc: Option<String>,
    pub quota_reset_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct QuotaSnapshots {
    pub premium_interactions: Option<QuotaSnapshot>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct QuotaSnapshot {
    pub entitlement: Option<f64>,
    /// Fractional remainder. `remaining` is the same number rounded, so this
    /// one is preferred wherever both are present.
    pub quota_remaining: Option<f64>,
    pub remaining: Option<f64>,
    pub percent_remaining: Option<f64>,
    pub unlimited: Option<bool>,
    pub overage_count: Option<f64>,
    pub overage_permitted: Option<bool>,
}

pub fn to_snapshot(user: CopilotUser) -> Result<CopilotSnapshot> {
    let quota = user
        .quota_snapshots
        .and_then(|snapshots| snapshots.premium_interactions)
        .ok_or_else(|| {
            AppError::Schema("Copilot response has no premium_interactions quota".into())
        })?;

    let unlimited = quota.unlimited.unwrap_or(false);
    let entitlement = checked_quota(quota.entitlement, "entitlement")?.unwrap_or(0.0);
    // A negative remainder is the included pool spent. GitHub clamps
    // `percent_remaining` at 0 and puts the overshoot here; that is the
    // limit, not schema drift.
    let raw_remaining = checked_remaining(quota.quota_remaining.or(quota.remaining))?;
    let spent_past_entitlement = raw_remaining.is_some_and(|value| value < 0.0);

    let premium_pct = if unlimited {
        0
    } else {
        let pct = used_percent(
            quota.percent_remaining,
            entitlement,
            raw_remaining.map(|value| value.max(0.0)),
        )?;
        if spent_past_entitlement {
            pct.max(100)
        } else {
            pct
        }
    };
    // `remaining` is what's left of the included pool, never a debt.
    // Consumption can pass the entitlement; that overshoot stays on `used`.
    let raw = raw_remaining.unwrap_or(0.0);
    let remaining = raw.max(0.0);

    Ok(CopilotSnapshot {
        plan: plan_label(user.copilot_plan.as_deref())?,
        account: checked_label(user.login.as_deref())?.unwrap_or_default(),
        premium_pct,
        entitlement,
        // Derived rather than read: `credits_used` is rounded to whole
        // requests, so it disagrees with the fractional remainder.
        used: (entitlement - raw).max(0.0),
        remaining,
        unlimited,
        overage_count: checked_quota(quota.overage_count, "overage_count")?.unwrap_or(0.0),
        overage_permitted: quota.overage_permitted.unwrap_or(false),
        reset_at: resolve_reset_at(&user.quota_reset_date_utc, &user.quota_reset_date)?,
    })
}

/// The server's own percentage wins; the counters are the fallback for a
/// response that omits it.
fn used_percent(
    percent_remaining: Option<f64>,
    entitlement: f64,
    remaining: Option<f64>,
) -> Result<i32> {
    if let Some(percent) = percent_remaining {
        return checked_percent(100.0 - percent);
    }
    match remaining {
        Some(remaining) if entitlement > 0.0 => {
            checked_percent((1.0 - remaining / entitlement) * 100.0)
        }
        _ => Err(AppError::Schema(
            "Copilot quota has neither a percentage nor a usable entitlement".into(),
        )),
    }
}

fn checked_percent(value: f64) -> Result<i32> {
    // Above 100 is the same exhausted state: a percentage that followed the
    // overshoot, or a counter fallback before the remainder was clamped.
    // Absurd magnitudes are still drift.
    if !value.is_finite() || !(-0.5..=MAX_EXHAUSTED_PERCENT).contains(&value) {
        return Err(AppError::Schema(
            "Copilot quota percentage is outside the supported range".into(),
        ));
    }
    Ok(value.round().clamp(0.0, 100.0) as i32)
}

fn checked_quota(value: Option<f64>, field: &str) -> Result<Option<f64>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !value.is_finite() || !(0.0..=MAX_BENIGN_QUOTA).contains(&value) {
        return Err(AppError::Schema(format!(
            "Copilot quota {field} is outside the supported range"
        )));
    }
    Ok(Some(value))
}

/// Included-pool remainder. Negative means the pool is spent — GitHub reports
/// the overshoot here and clamps `percent_remaining` at 0 — not schema drift.
/// Only non-finite or absurd magnitudes are rejected.
fn checked_remaining(value: Option<f64>) -> Result<Option<f64>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !value.is_finite() || !(-MAX_BENIGN_QUOTA..=MAX_BENIGN_QUOTA).contains(&value) {
        return Err(AppError::Schema(
            "Copilot quota remaining is outside the supported range".into(),
        ));
    }
    Ok(Some(value))
}

/// Plans arrive as bare slugs (`individual`, `business`); render them the way
/// GitHub's own billing page does.
fn plan_label(value: Option<&str>) -> Result<String> {
    let Some(value) = checked_label(value)? else {
        return Ok("GitHub Copilot".into());
    };
    let named = match value.as_str() {
        "individual" | "copilot_individual" => "Copilot Pro",
        "individual_pro_plus" | "copilot_pro_plus" => "Copilot Pro+",
        "business" | "copilot_business" => "Copilot Business",
        "enterprise" | "copilot_enterprise" => "Copilot Enterprise",
        "free" | "copilot_free" => "Copilot Free",
        _ => return Ok(format!("Copilot {value}")),
    };
    Ok(named.to_string())
}

fn checked_label(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if value.chars().count() > MAX_LABEL_CHARS || value.chars().any(char::is_control) {
        return Err(AppError::Schema("Copilot account label is invalid".into()));
    }
    Ok(Some(value.to_string()))
}

fn resolve_reset_at(
    timestamp: &Option<String>,
    date_only: &Option<String>,
) -> Result<Option<DateTime<Utc>>> {
    if let Some(value) = timestamp
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return DateTime::parse_from_rfc3339(value)
            .map(|dt| Some(dt.with_timezone(&Utc)))
            .map_err(|_| AppError::Schema("Copilot quota_reset_date_utc is not RFC 3339".into()));
    }
    let Some(value) = date_only
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|naive| Some(naive.and_utc()))
        .ok_or_else(|| AppError::Schema("Copilot quota_reset_date is not a date".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured verbatim from a live individual-plan response.
    const LIVE: &str = r#"{
        "login": "octocat",
        "copilot_plan": "individual",
        "quota_reset_date": "2026-10-01",
        "quota_reset_date_utc": "2026-10-01T00:00:00.000Z",
        "quota_snapshots": {
            "premium_interactions": {
                "entitlement": 200,
                "remaining": 198,
                "quota_remaining": 198.6,
                "percent_remaining": 99.3,
                "credits_used": 1,
                "unlimited": false,
                "overage_count": 0,
                "overage_permitted": false
            }
        }
    }"#;

    #[test]
    fn live_individual_shape_is_coherent() {
        let snapshot = to_snapshot(serde_json::from_str(LIVE).unwrap()).unwrap();
        assert_eq!(snapshot.plan, "Copilot Pro");
        assert_eq!(snapshot.account, "octocat");
        assert_eq!(snapshot.premium_pct, 1);
        assert_eq!(snapshot.entitlement, 200.0);
        assert_eq!(snapshot.remaining, 198.6);
        assert!((snapshot.used - 1.4).abs() < 1e-9);
        assert!(!snapshot.unlimited);
        assert_eq!(
            snapshot.reset_at.unwrap().to_rfc3339(),
            "2026-10-01T00:00:00+00:00"
        );
    }

    /// The fractional remainder is the real balance; `remaining` is it rounded.
    #[test]
    fn fractional_remainder_wins_over_the_rounded_one() {
        let snapshot = to_snapshot(serde_json::from_str(LIVE).unwrap()).unwrap();
        assert_eq!(snapshot.remaining, 198.6);
    }

    #[test]
    fn unlimited_premium_quota_never_reports_usage() {
        let user: CopilotUser = serde_json::from_str(
            r#"{"copilot_plan":"business","quota_snapshots":{"premium_interactions":{"unlimited":true,"percent_remaining":100.0,"entitlement":0}}}"#,
        )
        .unwrap();
        let snapshot = to_snapshot(user).unwrap();
        assert!(snapshot.unlimited);
        assert_eq!(snapshot.premium_pct, 0);
        assert_eq!(snapshot.plan, "Copilot Business");
    }

    #[test]
    fn counters_cover_a_response_without_a_percentage() {
        let user: CopilotUser = serde_json::from_str(
            r#"{"quota_snapshots":{"premium_interactions":{"entitlement":300,"quota_remaining":75}}}"#,
        )
        .unwrap();
        assert_eq!(to_snapshot(user).unwrap().premium_pct, 75);
    }

    #[test]
    fn a_missing_premium_quota_is_an_error_not_a_zero() {
        let user: CopilotUser = serde_json::from_str(r#"{"quota_snapshots":{}}"#).unwrap();
        assert!(to_snapshot(user).is_err());
    }

    #[test]
    fn malformed_numbers_and_dates_are_rejected() {
        for percent in [-1.0, 200.0, f64::INFINITY] {
            assert!(checked_percent(100.0 - percent).is_err() || !(0.0..=100.0).contains(&percent));
        }
        assert!(checked_quota(Some(-1.0), "entitlement").is_err());
        assert!(checked_quota(Some(f64::NAN), "entitlement").is_err());
        assert!(resolve_reset_at(&Some("not-a-date".into()), &None).is_err());
        assert!(resolve_reset_at(&None, &Some("2026-13-45".into())).is_err());
        assert!(resolve_reset_at(&None, &None).unwrap().is_none());
    }

    #[test]
    fn labels_are_bounded_and_control_free() {
        assert!(checked_label(Some(&"x".repeat(MAX_LABEL_CHARS + 1))).is_err());
        assert!(checked_label(Some("bad\u{1b}[31m")).is_err());
        assert_eq!(plan_label(None).unwrap(), "GitHub Copilot");
        assert_eq!(
            plan_label(Some("some_new_tier")).unwrap(),
            "Copilot some_new_tier"
        );
    }

    /// Live shape once the included pool is spent: percentage clamped at 0,
    /// overshoot on the remainder, overage not permitted. That is the limit.
    #[test]
    fn negative_remaining_is_the_limit_not_schema_drift() {
        let user: CopilotUser = serde_json::from_str(
            r#"{"copilot_plan":"individual","quota_snapshots":{"premium_interactions":{
                "entitlement":200,"remaining":-3,"quota_remaining":-3.0,
                "percent_remaining":0.0,"unlimited":false,
                "overage_count":0,"overage_permitted":false
            }}}"#,
        )
        .unwrap();
        let snapshot = to_snapshot(user).unwrap();
        assert_eq!(snapshot.plan, "Copilot Pro");
        assert_eq!(snapshot.premium_pct, 100);
        assert_eq!(snapshot.remaining, 0.0);
        assert!((snapshot.used - 203.0).abs() < 1e-9);
        assert!(!snapshot.overage_permitted);
        assert_eq!(snapshot.overage_count, 0.0);
    }

    #[test]
    fn negative_remaining_without_a_percentage_is_still_the_limit() {
        let user: CopilotUser = serde_json::from_str(
            r#"{"quota_snapshots":{"premium_interactions":{"entitlement":200,"quota_remaining":-3.0}}}"#,
        )
        .unwrap();
        let snapshot = to_snapshot(user).unwrap();
        assert_eq!(snapshot.premium_pct, 100);
        assert_eq!(snapshot.remaining, 0.0);
        assert!((snapshot.used - 203.0).abs() < 1e-9);
    }

    #[test]
    fn absurd_remaining_is_still_rejected() {
        assert!(checked_remaining(Some(f64::NAN)).is_err());
        assert!(checked_remaining(Some(f64::NEG_INFINITY)).is_err());
        assert!(checked_remaining(Some(-1e12)).is_err());
        assert!(checked_remaining(None).unwrap().is_none());
    }
}
