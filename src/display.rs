//! Sanitization for text that crosses an untrusted data boundary into a UI.
//!
//! Vendor responses and cached diagnostics are data, not terminal programs.
//! Keep ordinary Unicode and line breaks, but remove terminal control bytes
//! before the text is persisted or handed to Pango/ratatui/ANSI renderers.

/// Generous bound for one remote label or diagnostic field. Legitimate values
/// are normally a few dozen characters; the cap prevents a valid-but-hostile
/// JSON response from turning one UI cell or cache sidecar into megabytes.
pub const MAX_UNTRUSTED_FIELD_CHARS: usize = 4 * 1024;

/// Strip terminal control characters while preserving readable line layout.
///
/// Newlines are safe and useful in diagnostics. Tabs and carriage returns are
/// normalized to spaces; every other Unicode control character (including ESC,
/// BEL, DEL, and C1 controls) is removed. Invisible bidirectional markers and
/// overrides are also removed so an untrusted label cannot visually reorder
/// neighboring UI text. The result is capped by character, not byte, so UTF-8
/// is never split. Token-shaped substrings are redacted last: some gateways
/// echo a rejected credential back inside a 400/404 body, and that body is
/// about to be persisted to `.last_error` and shown in a tooltip.
pub fn sanitize_untrusted_field(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter_map(|ch| match ch {
            '\n' => Some('\n'),
            '\t' | '\r' => Some(' '),
            _ if ch.is_control() || is_bidi_control(ch) => None,
            _ => Some(ch),
        })
        .take(MAX_UNTRUSTED_FIELD_CHARS)
        .collect();
    redact_token_shapes(&cleaned)
}

/// Redact token-shaped substrings an upstream error body might echo back.
///
/// Covers the three shapes credentials actually arrive in: vendor key
/// prefixes (`sk-…`, `sk-or-…`, `sk-ant-…`) with at least 8 following key
/// characters, a `Bearer <token>` header value, and a bare opaque run of
/// 32+ base64/hex characters (no hyphens or dots, so hyphenated model ids
/// and dotted JWTs' segments stay legible). The diagnostic value of a
/// key-shaped run is nil; its leak cost is a persisted file and a tooltip.
pub fn redact_token_shapes(value: &str) -> String {
    const REDACTED: &str = "[redacted]";
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    let key_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let token_char = |c: char| {
        c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '~' | '+' | '/' | '=')
    };
    let opaque_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_');
    while i < chars.len() {
        // `sk-<8+ key chars>` — the vendor key prefixes.
        if chars[i] == 's'
            && chars.get(i + 1) == Some(&'k')
            && chars.get(i + 2) == Some(&'-')
            && chars.get(i + 3).is_some_and(|&c| key_char(c))
        {
            let mut j = i + 3;
            while j < chars.len() && key_char(chars[j]) {
                j += 1;
            }
            if j - (i + 3) >= 8 {
                out.push_str(REDACTED);
                i = j;
                continue;
            }
        }
        // `Bearer <16+ token chars>`, case-insensitive on the scheme word.
        if chars[i] == 'B' || chars[i] == 'b' {
            let lower: String = chars[i..(i + 6).min(chars.len())]
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .collect();
            if lower == "bearer" {
                let mut j = i + 6;
                while j < chars.len() && chars[j] == ' ' {
                    j += 1;
                }
                let token_start = j;
                while j < chars.len() && token_char(chars[j]) {
                    j += 1;
                }
                if token_start < j && j - token_start >= 16 {
                    out.push_str("Bearer ");
                    out.push_str(REDACTED);
                    i = j;
                    continue;
                }
            }
        }
        // A bare opaque run: 32+ base64/hex-ish characters with no hyphen or
        // dot — long hashes, raw base64 blobs, unprefixed key bodies.
        if chars[i].is_ascii_alphanumeric() {
            let mut j = i;
            while j < chars.len() && opaque_char(chars[j]) {
                j += 1;
            }
            if j - i >= 32 && !chars[i..j].contains(&'-') {
                out.push_str(REDACTED);
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// One line of untrusted text on its way to a terminal or a log.
///
/// [`sanitize_untrusted_field`] keeps newlines, which is right for a multi-line
/// diagnostic in a UI cell and wrong for anything sharing a line-oriented
/// stream with output the user is reading: one embedded newline forges a line.
/// A subprocess's stderr is exactly that case — it is one or more lines on
/// their way into an error message.
pub fn sanitize_untrusted_line(value: &str) -> String {
    sanitize_untrusted_field(value).replace('\n', " ")
}

/// A filesystem path on its way to the same place.
///
/// [`std::path::Display`] escapes nothing, and a path is not always a literal
/// this program chose — it can carry a component from an account name, a
/// vendor response, or an archive member. This is what [`crate::error::AppError::Io`]
/// renders its path through, so an attacker-chosen path cannot carry a terminal
/// escape out of *any* error site rather than only the ones that remembered.
pub fn sanitize_untrusted_path(path: &std::path::Path) -> String {
    sanitize_untrusted_line(&path.to_string_lossy())
}

fn is_bidi_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_terminal_sequences_but_keeps_text_and_newlines() {
        let input = "before\x1b]52;c;Y2xpcGJvYXJk\x07after\nnext\tcolumn\rreturn\u{202e}spoof";
        assert_eq!(
            sanitize_untrusted_field(input),
            "before]52;c;Y2xpcGJvYXJkafter\nnext column returnspoof"
        );
    }

    /// A subprocess's stderr shares a line-oriented stream with the message
    /// carrying it, so a newline in it forges a line the program never wrote.
    /// This is the shape `security` and `tar` diagnostics arrive in.
    #[test]
    fn collapses_newlines_so_untrusted_text_cannot_forge_a_line() {
        let stderr = "tar: \x1b[2Kall good\nRESTORED: 0 files";
        let out = sanitize_untrusted_line(stderr);
        assert!(!out.contains('\n'), "{out:?}");
        assert!(!out.contains('\u{1b}'), "{out:?}");
        assert_eq!(out, "tar: [2Kall good RESTORED: 0 files");
    }

    #[test]
    fn a_path_carrying_an_escape_renders_without_it() {
        let path = std::path::Path::new("/tmp/\x1b[2Kspoofed");
        assert_eq!(sanitize_untrusted_path(path), "/tmp/[2Kspoofed");
    }

    #[test]
    fn caps_untrusted_fields_without_splitting_unicode() {
        let input = "é".repeat(MAX_UNTRUSTED_FIELD_CHARS + 10);
        let output = sanitize_untrusted_field(&input);
        assert_eq!(output.chars().count(), MAX_UNTRUSTED_FIELD_CHARS);
        assert!(output.chars().all(|ch| ch == 'é'));
    }

    /// Some gateways echo the rejected credential back inside a non-401/403
    /// error body ("malformed Authorization: sk-ant-…"), and that body is
    /// persisted to `.last_error` and rendered in tooltips. Key-shaped runs
    /// must not survive the sanitize boundary — this is the LOW-1 hardening.
    #[test]
    fn redacts_token_shapes_an_error_body_might_echo() {
        // Vendor key prefixes.
        assert_eq!(
            redact_token_shapes("bad key sk-ant-api03-AbCdEf1234567890 rejected"),
            "bad key [redacted] rejected"
        );
        assert_eq!(
            redact_token_shapes("invalid sk-or-v1-0123456789abcdef"),
            "invalid [redacted]"
        );
        // A Bearer header value.
        assert_eq!(
            redact_token_shapes("Bearer abcdef1234567890abcdef1234567890 expired"),
            "Bearer [redacted] expired"
        );
        // Bare opaque blobs: 32+ base64/hex with no hyphen or dot.
        assert_eq!(
            redact_token_shapes("request d41d8cd98f00b204e9800998ecf8427e failed"),
            "request [redacted] failed"
        );
    }

    /// The redaction must not eat legitimate diagnostics: hyphenated model
    /// ids, dotted JWT segments, and ordinary words stay legible.
    #[test]
    fn redaction_spares_legible_diagnostics() {
        assert_eq!(
            redact_token_shapes("claude-3-5-sonnet-latest-20241022"),
            "claude-3-5-sonnet-latest-20241022"
        );
        assert_eq!(
            redact_token_shapes("rate limited, retry after 30s"),
            "rate limited, retry after 30s"
        );
        // A short `sk-` fragment without key material is prose, not a key.
        assert_eq!(redact_token_shapes("the sk- prefix"), "the sk- prefix");
    }

    /// The sanitize boundary itself applies the redaction, so every sink
    /// (`.last_error` persistence, tooltips, report errors) inherits it.
    #[test]
    fn sanitize_field_redacts_as_well() {
        let out = sanitize_untrusted_field(
            "400: authorization sk-proj-AbCdEfGh1234567890 is malformed\nline2",
        );
        assert_eq!(out, "400: authorization [redacted] is malformed\nline2");
    }
}
