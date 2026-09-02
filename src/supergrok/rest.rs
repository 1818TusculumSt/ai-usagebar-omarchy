//! REST API fallback for SuperGrok billing when the Grok Build ACP extension
//! is unavailable (e.g. older CLI versions that lack `x.ai/billing`).
//!
//! Uses the undocumented `cli-chat-proxy.grok.com/v1/billing` endpoint,
//! discovered from Grok CLI's own log lines. The response shape matches the
//! ACP `x.ai/billing` result, so it maps directly to `types::BillingResponse`.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::error::{AppError, Result};

use super::types::BillingResponse;

const BILLING_ENDPOINT: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const USER_AGENT: &str = "curl/8.9.1";

/// Read the OIDC token from `~/.grok/auth.json` and fetch billing via REST.
pub async fn fetch_billing(auth_path: &Path) -> Result<BillingResponse> {
    let token = read_token(auth_path)?;
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| AppError::Other(format!("failed to build HTTP client: {e}")))?;

    let response = client
        .get(BILLING_ENDPOINT)
        .bearer_auth(&token)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Http {
            status: status.as_u16(),
            body,
        });
    }

    let json: Value = response.json().await?;
    parse_billing_response(json)
}

/// Extract the Bearer token from `~/.grok/auth.json`.
///
/// The file is keyed by `"<oidc_issuer>::<client_id>"` — there's only ever
/// one entry. Returns `Credentials` error if the file is missing, unreadable,
/// or the token is absent/expired.
fn read_token(auth_path: &Path) -> Result<String> {
    let data: Value = {
        let bytes = std::fs::read(auth_path).map_err(|e| {
            AppError::Credentials(format!(
                "cannot read Grok auth file {}: {e}",
                auth_path.display()
            ))
        })?;
        serde_json::from_slice(&bytes).map_err(|e| {
            AppError::Credentials(format!(
                "Grok auth file {} is not valid JSON: {e}",
                auth_path.display()
            ))
        })?
    };

    // Extract the single entry's key (Bearer token)
    let entry = data
        .as_object()
        .and_then(|obj| obj.values().next())
        .and_then(|v| v.as_object());

    let entry = entry.ok_or_else(|| {
        AppError::Credentials("Grok auth file has unexpected structure".into())
    })?;

    // Check expiration
    if let Some(expires_at) = entry.get("expires_at").and_then(|v| v.as_f64()) {
        // expires_at is in milliseconds
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as f64;
        if expires_at <= now_ms {
            return Err(AppError::Credentials(
                "Grok sign-in expired; run `grok` or `grok login` to refresh".into(),
            ));
        }
    }

    entry
        .get("key")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .ok_or_else(|| AppError::Credentials(
            "Grok auth file has no key; run `grok login` to sign in".into(),
        ))
}

/// Parse the REST billing response into the same `BillingResponse` the ACP
/// extension returns. The endpoint returns a `config` object with
/// `creditUsagePercent` and `currentPeriod`, matching the ACP shape.
fn parse_billing_response(json: Value) -> Result<BillingResponse> {
    // The REST response wraps the billing data in a `config` object,
    // same as the ACP response. Deserialize directly into BillingResponse.
    serde_json::from_value(json).map_err(|e| {
        AppError::Schema(format!("Grok REST billing response does not match expected schema: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_weekly_billing_response() {
        let json = json!({
            "config": {
                "creditUsagePercent": 42.5,
                "currentPeriod": {
                    "type": "USAGE_PERIOD_TYPE_WEEKLY",
                    "start": "2026-08-28T00:00:00Z",
                    "end": "2026-09-04T00:00:00Z"
                }
            }
        });
        let response = parse_billing_response(json).unwrap();
        assert_eq!(response.config.unwrap().credit_usage_percent, Some(42.5));
    }

    #[test]
    fn parse_response_with_subscription_tier() {
        let json = json!({
            "config": {
                "creditUsagePercent": 10.0,
                "currentPeriod": {
                    "type": "USAGE_PERIOD_TYPE_WEEKLY",
                    "end": "2026-09-01T00:00:00Z"
                }
            },
            "subscription_tier": "SuperGrok"
        });
        let response = parse_billing_response(json).unwrap();
        assert_eq!(response.subscription_tier.as_deref(), Some("SuperGrok"));
        assert_eq!(response.config.unwrap().credit_usage_percent, Some(10.0));
    }
}
