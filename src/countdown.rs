//! Human-readable countdown between two instants.
//!
//! Mirrors claudebar's `countdown()` shell function (claudebar:252-268):
//!   - missing / unparseable reset → `"—"`
//!   - reset already in the past → `"now"`
//!   - ≥1 day remaining → `"{d}d {h}h"`
//!   - otherwise → `"{h}h {mm}m"` (zero-padded minutes)

use chrono::{DateTime, Utc};

/// Format `reset - now` as a short human string.
///
/// Same buckets as claudebar; `None` for `reset` returns `"—"`, matching the
/// shell behavior where `[[ -z "$ts" ]]` short-circuits.
pub fn format(reset: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(reset) = reset else {
        return "—".to_string();
    };

    let diff = reset.signed_duration_since(now);
    let secs = diff.num_seconds();
    if secs <= 0 {
        return "now".to_string();
    }

    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;

    if days > 0 {
        format!("{days}d {hours}h")
    } else {
        format!("{hours}h {mins:02}m")
    }
}

/// Bar-tile countdown: ONE unit, at most one decimal.
///
///   - under an hour  → `42m`   (whole minutes, rounded up so 30s reads `1m`)
///   - under a day    → `5.3h`  (tenths of an hour; `5h` when the tenth is 0)
///   - a day or more  → `2.3d`  (tenths of a day; `7d` for a full week)
///
/// The compact form is for status-bar tiles only — tooltips and panels keep
/// [`format`]'s two-component detail (`2h 05m`). Truncated (never rounded up)
/// past the minutes bucket so a bar never claims more time than remains.
pub fn format_compact(reset: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(reset) = reset else {
        return "—".to_string();
    };

    let secs = reset.signed_duration_since(now).num_seconds();
    if secs <= 0 {
        return "now".to_string();
    }
    if secs < 3_600 {
        // Ceil to whole minutes, capped at 59: 59m59s reads "59m", never
        // "60m" (an hour it is not — one second later it becomes one).
        return format!("{}m", ((secs + 59) / 60).min(59));
    }
    let (unit_secs, suffix) = if secs < 86_400 {
        (3_600, "h")
    } else {
        (86_400, "d")
    };
    let tenths = secs * 10 / unit_secs;
    let whole = tenths / 10;
    let frac = tenths % 10;
    if frac == 0 {
        format!("{whole}{suffix}")
    } else {
        format!("{whole}.{frac}{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32, h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, h, m, 0).unwrap()
    }

    #[test]
    fn missing_reset_renders_em_dash() {
        let now = at(2026, 5, 23, 12, 0);
        assert_eq!(format(None, now), "—");
    }

    #[test]
    fn past_reset_renders_now() {
        let now = at(2026, 5, 23, 12, 0);
        let reset = at(2026, 5, 23, 11, 0);
        assert_eq!(format(Some(reset), now), "now");
    }

    #[test]
    fn exact_zero_renders_now() {
        // Bash uses `<= 0`, so a zero diff is "now".
        let t = at(2026, 5, 23, 12, 0);
        assert_eq!(format(Some(t), t), "now");
    }

    #[test]
    fn hours_minutes_zero_padded() {
        let now = at(2026, 5, 23, 12, 0);
        let reset = at(2026, 5, 23, 13, 5); // 1h 5m
        assert_eq!(format(Some(reset), now), "1h 05m");
    }

    #[test]
    fn hours_minutes_no_days_under_one_day() {
        let now = at(2026, 5, 23, 12, 0);
        let reset = at(2026, 5, 24, 11, 59); // 23h 59m
        assert_eq!(format(Some(reset), now), "23h 59m");
    }

    #[test]
    fn one_day_one_hour() {
        let now = at(2026, 5, 23, 12, 0);
        let reset = at(2026, 5, 24, 13, 30); // 1d 1h (minutes dropped)
        assert_eq!(format(Some(reset), now), "1d 1h");
    }

    #[test]
    fn multiple_days_drops_minutes() {
        let now = at(2026, 5, 23, 12, 0);
        let reset = at(2026, 5, 27, 13, 45); // 4d 1h
        assert_eq!(format(Some(reset), now), "4d 1h");
    }

    #[test]
    fn one_second_remaining_renders_zero_hours() {
        // Mirrors claudebar: anything > 0 but < 1 min → "0h 00m"
        let now = at(2026, 5, 23, 12, 0);
        let reset = now + chrono::Duration::seconds(1);
        assert_eq!(format(Some(reset), now), "0h 00m");
    }

    #[test]
    fn compact_missing_reset_renders_em_dash() {
        let now = at(2026, 8, 29, 12, 0);
        assert_eq!(format_compact(None, now), "—");
    }

    #[test]
    fn compact_past_reset_renders_now() {
        let now = at(2026, 8, 29, 12, 0);
        assert_eq!(format_compact(Some(at(2026, 8, 29, 11, 0)), now), "now");
        assert_eq!(format_compact(Some(now), now), "now");
    }

    #[test]
    fn compact_sub_hour_is_whole_minutes_rounded_up() {
        let now = at(2026, 8, 29, 12, 0);
        let reset = now + chrono::Duration::minutes(42);
        assert_eq!(format_compact(Some(reset), now), "42m");
        // 30 seconds still reads as one minute left, never "0m".
        let half = now + chrono::Duration::seconds(30);
        assert_eq!(format_compact(Some(half), now), "1m");
    }

    #[test]
    fn compact_sub_day_is_decimal_hours() {
        let now = at(2026, 8, 29, 12, 0);
        // 5h 20m → 5.3h
        let reset = now + chrono::Duration::hours(5) + chrono::Duration::minutes(20);
        assert_eq!(format_compact(Some(reset), now), "5.3h");
        // A whole number of hours drops the ".0": exactly 5h → "5h".
        let whole = now + chrono::Duration::hours(5);
        assert_eq!(format_compact(Some(whole), now), "5h");
        // 23h 59m stays under a day: "23.9h", not "1d".
        let edge = now + chrono::Duration::hours(23) + chrono::Duration::minutes(59);
        assert_eq!(format_compact(Some(edge), now), "23.9h");
    }

    #[test]
    fn compact_day_scale_is_decimal_days() {
        let now = at(2026, 8, 29, 12, 0);
        // 2d 7h 12m → 2.3d
        let reset = now
            + chrono::Duration::days(2)
            + chrono::Duration::hours(7)
            + chrono::Duration::minutes(12);
        assert_eq!(format_compact(Some(reset), now), "2.3d");
        // Exactly one day drops the decimal: "1d".
        let day = now + chrono::Duration::days(1);
        assert_eq!(format_compact(Some(day), now), "1d");
        // A full week: "7d".
        let week = now + chrono::Duration::days(7);
        assert_eq!(format_compact(Some(week), now), "7d");
    }

    #[test]
    fn compact_never_rounds_up_past_the_remaining_time() {
        // 59m59s is minutes, not "1.0h"; 23h59m59s is 23.9h, not "1.0d" —
        // a countdown that rounds up claims time the user does not have.
        let now = at(2026, 8, 29, 12, 0);
        let just_under_hour = now + chrono::Duration::seconds(3_599);
        assert_eq!(format_compact(Some(just_under_hour), now), "59m");
        let just_under_day = now + chrono::Duration::seconds(86_399);
        assert_eq!(format_compact(Some(just_under_day), now), "23.9h");
    }
}
