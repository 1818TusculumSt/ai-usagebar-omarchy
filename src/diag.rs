//! Best-effort append-only debug log for postmortems:
//! `~/.local/state/ai-usagebar-omarchy/debug.log` (mode 0600, size-capped).
//!
//! Design constraints:
//! - **Never fails the caller.** Every error (unwritable state dir, full
//!   disk) is swallowed; diagnostics must not break the widget's exit-0
//!   invariant or a settings save.
//! - **Never logs secrets.** Requests pass through [`redact`] first; the
//!   redaction is also unit-tested against the exact shapes the settings
//!   bridge accepts so a renamed field cannot slip a key into the log.
//! - Low volume by construction: only notable events (settings applies,
//!   widget fallback errors, layout migrations, report failures) — the
//!   60-second widget loop logs nothing on the happy path.

use std::io::Write;
use std::path::PathBuf;

fn str_len(s: &str) -> usize {
    s.chars().count()
}

/// Keys whose values must never reach the log.
const SECRET_KEYS: &[&str] = &[
    "api_key",
    "token",
    "access_token",
    "refresh_token",
    "secret",
    "password",
    "credentials",
];

/// Cap: past this size the log is rewritten keeping its tail.
const MAX_LOG_BYTES: u64 = 512 * 1024;
/// Tail kept when rotating.
const KEEP_BYTES: usize = 256 * 1024;

/// The log file path (`~/.local/state/ai-usagebar-omarchy/debug.log`,
/// falling back to the cache dir where the platform has no state dir).
pub fn log_path() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()
        .and_then(|b| b.state_dir().map(PathBuf::from))
        .or_else(|| {
            crate::cache::app_cache_root()
                .ok()
                .and_then(|p| p.parent().map(PathBuf::from))
        })?;
    Some(base.join(crate::APP_DIR).join("debug.log"))
}

/// Append one event line: `<iso-ts> <kind> <detail>`.
pub fn event(kind: &str, detail: &str) {
    let Some(path) = log_path() else { return };
    let line = format!(
        "{} {} {}\n",
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ"),
        kind,
        detail.replace(['\n', '\r'], " ")
    );
    let _ = append(&path, line.as_bytes());
}

fn append(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // New files start tight: the log can quote config-adjacent input.
    if !path.exists() {
        let _ = std::fs::File::create(path);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                path,
                std::fs::Permissions::from_mode(0o600),
            );
        }
    }
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX_LOG_BYTES {
        rotate(path)?;
    }
    let mut f = std::fs::OpenOptions::new().append(true).open(path)?;
    f.write_all(bytes)
}

/// Keep only the tail; on any failure truncate rather than grow unbounded.
fn rotate(path: &std::path::Path) -> std::io::Result<()> {
    let data = std::fs::read(path)?;
    let keep = data.len().saturating_sub(KEEP_BYTES);
    let start = data[keep..]
        .iter()
        .position(|b| *b == b'\n')
        .map(|i| keep + i + 1)
        .unwrap_or(keep);
    let mut tmp = tempfile::Builder::new()
        .prefix(".log.")
        .tempfile_in(path.parent().unwrap_or(std::path::Path::new(".")))?;
    tmp.write_all(&data[start..])?;
    tmp.persist(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(0o600),
        );
    }
    Ok(())
}

/// Recursively redact a JSON value for logging: secret-named keys collapse
/// to `<redacted>`, and every `{"action":"set","value":…}` mutation loses
/// its payload (the settings bridge's key shape — `value` is the key).
pub fn redact(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let is_set = map.get("action").and_then(|a| a.as_str()) == Some("set");
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if SECRET_KEYS.contains(&k.as_str()) {
                    // A secret-named OBJECT is a mutation ({action, value});
                    // keep its structure — the `value` rule below strips the
                    // payload — so "clear" stays diagnosable.
                    let replacement = match v {
                        serde_json::Value::Object(_) => redact(v),
                        _ => serde_json::Value::String("<redacted>".into()),
                    };
                    out.insert(k.clone(), replacement);
                } else if is_set && k == "value" {
                    out.insert(
                        k.clone(),
                        serde_json::Value::String(format!(
                            "<redacted:{}chars>",
                            v.as_str().map(str_len).unwrap_or(0)
                        )),
                    );
                } else {
                    out.insert(k.clone(), redact(v));
                }
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(redact).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp_log() -> (tempfile::TempDir, PathBuf) {
        let td = tempfile::TempDir::new().unwrap();
        let p = td.path().join("nested").join("debug.log");
        (td, p)
    }

    fn append_to(path: &std::path::Path, bytes: &[u8]) {
        append(path, bytes).unwrap();
    }

    #[test]
    fn events_append_with_timestamps_and_cap_permissions() {
        let (_td, path) = tmp_log();
        append_to(&path, b"2026-01-01T00:00:00.000Z boot start\n");
        append_to(&path, b"2026-01-01T00:00:01.000Z apply ok\n");
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("boot start"));
        assert!(contents.contains("apply ok"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "diagnostics start tight");
        }
    }

    #[test]
    fn rotation_keeps_the_tail_not_the_head() {
        let (_td, path) = tmp_log();
        let head = format!("{} HEAD\n", "x".repeat(1024));
        let tail = format!("{} TAIL-MARKER\n", "y".repeat(1024));
        let mut big = String::new();
        for _ in 0..300 {
            big.push_str(&head);
        }
        for _ in 0..300 {
            big.push_str(&tail);
        }
        append_to(&path, big.as_bytes());
        // Over the cap → the next append rotates.
        append_to(&path, b"z after rotate\n");
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("TAIL-MARKER"));
        assert!(contents.contains("after rotate"));
        assert!(!contents.contains("HEAD"), "rotation must drop the head");
        assert!(contents.len() < KEEP_BYTES + 4096);
    }

    /// The exact shapes the settings bridge accepts: a plain vendor key
    /// mutation, an account mutation with a key, and inline config values
    /// must all lose their payloads — this is the test a renamed field has
    /// to keep passing.
    #[test]
    fn settings_requests_never_log_key_material() {
        let request = json!({
            "schema_version": 1,
            "primary": "zai",
            "keys": {
                "kimi": {"action": "set", "value": "sk-live-kimi-secret"}
            },
            "accounts": {
                "zai": [
                    {"action": "update", "label": "team",
                     "fields": {"organization_id": "org-1"},
                     "api_key": {"action": "set", "value": "team-key-secret"}},
                    {"action": "add", "name": "payg",
                     "api_key": {"action": "clear"}}
                ]
            }
        });
        let redacted = redact(&request);
        let text = redacted.to_string();
        assert!(!text.contains("sk-live-kimi-secret"), "{text}");
        assert!(!text.contains("team-key-secret"), "{text}");
        assert!(text.contains("<redacted:19chars>"), "{text}");
        // Structure survives for debugging.
        assert!(text.contains("organization_id"), "{text}");
        assert!(text.contains("\"label\":\"team\""), "{text}");
        assert!(text.contains("\"clear\""), "{text}");
    }
}
