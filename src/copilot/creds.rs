//! GitHub token resolution for the Copilot quota endpoint.
//!
//! Precedence matches the official Copilot CLI so one exported variable drives
//! both: inline config, then `COPILOT_GITHUB_TOKEN` / `GH_TOKEN` /
//! `GITHUB_TOKEN`, then whatever `gh` holds in its own credential store.
//!
//! No token is ever minted, refreshed, or written to disk here, and none may
//! appear in an error: every failure below names the *source* that failed.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::error::{AppError, Result};

const GH_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_TOKEN_BYTES: usize = 8 * 1024;
/// The Copilot API rejects classic PATs outright, so catching it here turns an
/// opaque 401 into an actionable message.
const CLASSIC_PAT_PREFIX: &str = "ghp_";

pub const TOKEN_ENV_VARS: &[&str] = &["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    Config,
    Env(&'static str),
    GhCli,
}

impl TokenSource {
    pub fn label(self) -> String {
        match self {
            Self::Config => "[copilot] token".into(),
            Self::Env(var) => var.to_string(),
            Self::GhCli => "`gh auth token`".into(),
        }
    }
}

/// Pure precedence + validation. Tests drive this directly so they never read
/// the real environment or spawn anything.
pub fn choose_token(
    inline: Option<&str>,
    env: &[(&'static str, String)],
    gh: Option<&str>,
) -> Result<(String, TokenSource)> {
    let candidates = std::iter::once((TokenSource::Config, inline))
        .chain(
            env.iter()
                .map(|(var, value)| (TokenSource::Env(var), Some(value.as_str()))),
        )
        .chain(std::iter::once((TokenSource::GhCli, gh)));

    for (source, value) in candidates {
        let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
            continue;
        };
        return validate(value, source).map(|token| (token, source));
    }

    Err(AppError::Credentials(format!(
        "no GitHub token for Copilot: set one of {}, run `gh auth login`, or put one in [copilot] token",
        TOKEN_ENV_VARS.join(" / ")
    )))
}

fn validate(token: &str, source: TokenSource) -> Result<String> {
    if token.len() > MAX_TOKEN_BYTES || token.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(AppError::Credentials(format!(
            "the GitHub token from {} is not a well-formed token",
            source.label()
        )));
    }
    if token.starts_with(CLASSIC_PAT_PREFIX) {
        return Err(AppError::Credentials(format!(
            "the GitHub token from {} is a classic PAT, which Copilot rejects; use `gh auth login` or a fine-grained token with the Copilot Requests permission",
            source.label()
        )));
    }
    Ok(token.to_string())
}

/// Production resolver: reads the environment, then falls back to `gh`.
pub async fn resolve_token(
    inline: Option<&str>,
    gh_binary: &Path,
) -> Result<(String, TokenSource)> {
    let env: Vec<(&'static str, String)> = TOKEN_ENV_VARS
        .iter()
        .filter_map(|var| std::env::var(var).ok().map(|value| (*var, value)))
        .collect();

    // Only pay for the subprocess when nothing else supplied a token.
    if inline.is_some_and(|t| !t.trim().is_empty()) || !env.is_empty() {
        return choose_token(inline, &env, None);
    }

    let gh = gh_auth_token(gh_binary).await;
    choose_token(inline, &env, gh.as_deref())
}

/// `gh auth token`, bounded and best-effort: a missing or unauthenticated `gh`
/// is a miss, not an error, so the caller reports the one "no token" message.
async fn gh_auth_token(gh_binary: &Path) -> Option<String> {
    let mut command = Command::new(gh_binary);
    command
        .arg("auth")
        .arg("token")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // `gh` echoes GH_TOKEN/GITHUB_TOKEN straight back when they are set. Those
    // already lost the precedence race above, so strip them and let `gh` answer
    // from its own credential store.
    for var in crate::vendor::vendor_secret_env_vars_to_remove(&[]) {
        command.env_remove(var);
    }
    command.env("GH_PROMPT_DISABLED", "1");
    command.env("GH_NO_UPDATE_NOTIFIER", "1");

    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let mut buf = Vec::new();

    let read = tokio::time::timeout(GH_TIMEOUT, async {
        (&mut stdout)
            .take(MAX_TOKEN_BYTES as u64 + 1)
            .read_to_end(&mut buf)
            .await
    })
    .await;

    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;

    read.ok()?.ok()?;
    let token = String::from_utf8(buf).ok()?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
        pairs
            .iter()
            .map(|(var, value)| (*var, (*value).to_string()))
            .collect()
    }

    #[test]
    fn inline_config_outranks_the_environment_and_gh() {
        let (token, source) = choose_token(
            Some("gho_inline"),
            &env(&[("GH_TOKEN", "gho_env")]),
            Some("gho_gh"),
        )
        .unwrap();
        assert_eq!(token, "gho_inline");
        assert_eq!(source, TokenSource::Config);
    }

    #[test]
    fn environment_order_follows_the_copilot_cli() {
        let (token, source) = choose_token(
            None,
            &env(&[
                ("COPILOT_GITHUB_TOKEN", "gho_first"),
                ("GH_TOKEN", "gho_second"),
            ]),
            None,
        )
        .unwrap();
        assert_eq!(token, "gho_first");
        assert_eq!(source, TokenSource::Env("COPILOT_GITHUB_TOKEN"));
    }

    #[test]
    fn gh_cli_is_the_last_resort() {
        let (token, source) = choose_token(None, &[], Some("gho_gh")).unwrap();
        assert_eq!(token, "gho_gh");
        assert_eq!(source, TokenSource::GhCli);
    }

    #[test]
    fn blank_candidates_are_skipped_rather_than_selected() {
        let (token, source) =
            choose_token(Some("   "), &env(&[("GH_TOKEN", "")]), Some("gho_gh")).unwrap();
        assert_eq!(token, "gho_gh");
        assert_eq!(source, TokenSource::GhCli);
    }

    #[test]
    fn no_candidate_names_every_way_to_supply_one() {
        let error = choose_token(None, &[], None).unwrap_err().to_string();
        assert!(error.contains("COPILOT_GITHUB_TOKEN"));
        assert!(error.contains("gh auth login"));
    }

    /// The failure must name the source, never the token.
    #[test]
    fn rejections_name_the_source_and_never_echo_the_token() {
        let error = choose_token(None, &env(&[("GH_TOKEN", "ghp_secret_value")]), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("classic PAT"));
        assert!(error.contains("GH_TOKEN"));
        assert!(!error.contains("ghp_secret_value"));

        let error = choose_token(Some("has space\u{1b}"), &[], None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("[copilot] token"));
        assert!(!error.contains("has space"));
    }

    #[tokio::test]
    async fn a_missing_gh_binary_is_a_miss_not_a_crash() {
        let td = tempfile::TempDir::new().unwrap();
        assert!(
            gh_auth_token(&td.path().join("definitely-not-gh"))
                .await
                .is_none()
        );
    }
}
