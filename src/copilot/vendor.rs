//! Copilot renderer — premium-request usage percent + reset countdown.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::countdown;
use crate::format::{placeholders, substitute, updated_at_hm};
use crate::pacing::PaceSeverity;
use crate::pango::{self, color_span, escape, severity_color, severity_for};
use crate::theme::Theme;
use crate::tooltip::{Line as TooltipLine, render_bordered};
use crate::usage::CopilotSnapshot;
use crate::vendor::{RenderOpts, VendorId, VendorOutcome};
use crate::waybar::{Class, WaybarOutput};

use super::fetch::FetchOutcome;

pub const DEFAULT_FORMAT: &str = "{cpl_pct}% · {cpl_reset}";

const DEFAULT_ICON: &str = "";
const UNLIMITED: &str = "∞";

/// Request counts arrive fractional (a premium request can cost less than one
/// whole unit), so keep a decimal only when there is one to keep.
fn requests(value: f64) -> String {
    if (value - value.round()).abs() < 0.05 {
        format!("{:.0}", value.round())
    } else {
        format!("{value:.1}")
    }
}

pub fn build_placeholders(
    snap: &CopilotSnapshot,
    now: DateTime<Utc>,
) -> HashMap<&'static str, String> {
    let pct = snap.premium_pct.to_string();
    let reset = countdown::format(snap.reset_at, now);
    let (used, entitlement, remaining) = if snap.unlimited {
        (
            UNLIMITED.to_string(),
            UNLIMITED.to_string(),
            UNLIMITED.to_string(),
        )
    } else {
        (
            requests(snap.used),
            requests(snap.entitlement),
            requests(snap.remaining),
        )
    };

    placeholders(vec![
        ("icon", DEFAULT_ICON.to_string()),
        ("vendor_short", VendorId::Copilot.short_name().to_string()),
        // Cross-vendor compatibility aliases for the single current pool.
        ("plan", snap.plan.clone()),
        ("session_pct", pct.clone()),
        ("session_reset", reset.clone()),
        ("weekly_pct", pct.clone()),
        ("weekly_reset", reset.clone()),
        // Copilot-specific.
        ("cpl_plan", snap.plan.clone()),
        ("cpl_pct", pct),
        ("cpl_reset", reset),
        ("cpl_used", used),
        ("cpl_entitlement", entitlement),
        ("cpl_remaining", remaining),
        ("cpl_account", snap.account.clone()),
    ])
}

pub fn severity(snap: &CopilotSnapshot) -> PaceSeverity {
    if snap.unlimited {
        return PaceSeverity::Low;
    }
    severity_for(snap.premium_pct)
}

pub fn render(
    outcome: &VendorOutcome,
    snap: &CopilotSnapshot,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> WaybarOutput {
    let class = Class::from(severity(snap));
    let format = opts
        .format
        .clone()
        .unwrap_or_else(|| DEFAULT_FORMAT.to_string());
    let mut values = build_placeholders(snap, now);
    for key in ["plan", "cpl_plan", "cpl_account"] {
        if let Some(value) = values.get_mut(key) {
            *value = escape(value);
        }
    }

    let mut text = substitute(&format, &values);
    if outcome.stale {
        text.push_str(" ⏸");
    }

    let wrapper_color = severity_color(severity(snap), theme).to_string();
    let icon_prefix = match opts.icon.as_deref() {
        Some(ic) if !ic.is_empty() => format!("{ic} "),
        _ => String::new(),
    };
    let bar_text = color_span(&wrapper_color, &format!("{icon_prefix}{text}"));

    let tooltip = if let Some(fmt) = opts.tooltip_format.as_deref() {
        substitute(fmt, &values)
    } else {
        render_tooltip(outcome, snap, theme, now)
    };

    WaybarOutput {
        text: bar_text,
        tooltip,
        class,
    }
}

fn render_tooltip(
    outcome: &VendorOutcome,
    snap: &CopilotSnapshot,
    theme: &Theme,
    now: DateTime<Utc>,
) -> String {
    let fg = &theme.fg;
    let blue = &theme.blue;
    let dim = &theme.dim;

    let mut lines: Vec<TooltipLine> = Vec::new();
    lines.push(TooltipLine::Center(format!(
        "<span font_weight='bold' foreground='{blue}'>{}</span>",
        escape(&snap.plan)
    )));
    lines.push(TooltipLine::Sep);
    lines.push(TooltipLine::Body("".into()));

    lines.push(TooltipLine::Body(format!(
        " <span foreground='{fg}'>  󰔟  Premium requests</span>"
    )));
    if snap.unlimited {
        lines.push(TooltipLine::Body(format!(
            "   <span font_weight='bold' foreground='{fg}'>{UNLIMITED}</span>  <span foreground='{dim}'>included</span>"
        )));
    } else {
        let pct = snap.premium_pct;
        let color = severity_color(severity(snap), theme);
        let bar = pango::progress_bar(pct, color, theme, None);
        lines.push(TooltipLine::Body(format!(
            "   {bar}  <span font_weight='bold' foreground='{color}'>{pct}%</span>"
        )));
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{dim}'>  {} of {} used · {} left</span>",
            escape(&requests(snap.used)),
            escape(&requests(snap.entitlement)),
            escape(&requests(snap.remaining))
        )));
    }
    if snap.reset_at.is_some() {
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{dim}'>  ⏱  Resets in {}</span>",
            escape(&countdown::format(snap.reset_at, now))
        )));
    }

    if snap.overage_count > 0.0 {
        lines.push(TooltipLine::Body("".into()));
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{}'>  󰀪  {} billed over the plan</span>",
            theme.orange,
            escape(&requests(snap.overage_count))
        )));
    } else if !snap.unlimited && !snap.overage_permitted && snap.premium_pct >= 100 {
        lines.push(TooltipLine::Body("".into()));
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{dim}'>  Premium models are paused until the reset</span>"
        )));
    }

    if let Some((code, msg)) = outcome.last_error.as_ref() {
        let (icon, ecolor) = if *code >= 500 {
            ("󰅚", theme.red.as_str())
        } else {
            ("󰀪", theme.orange.as_str())
        };
        let label = if *code == 0 {
            "Refresh error".to_string()
        } else {
            format!("HTTP {code}")
        };
        lines.push(TooltipLine::Body("".into()));
        lines.push(TooltipLine::Sep);
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{ecolor}'>  {icon}  {label}</span>"
        )));
        lines.push(TooltipLine::Body(format!(
            "     <span foreground='{dim}'>{}</span>",
            escape(msg)
        )));
    }

    let updated = updated_at_hm(now, outcome.cache_age);
    lines.push(TooltipLine::Body("".into()));
    lines.push(TooltipLine::Sep);
    lines.push(TooltipLine::Body(format!(
        " <span foreground='{dim}'>  󰅐  Updated {updated}</span>"
    )));

    render_bordered(&lines, theme)
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        Self {
            snapshot: crate::usage::VendorSnapshot::Copilot(o.snapshot),
            stale: o.stale,
            last_error: o.last_error,
            cache_age: o.cache_age,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 16, 12, 0, 0).unwrap()
    }

    fn sample_snap() -> CopilotSnapshot {
        CopilotSnapshot {
            plan: "Copilot Pro".into(),
            account: "octocat".into(),
            premium_pct: 34,
            entitlement: 200.0,
            used: 68.0,
            remaining: 132.0,
            unlimited: false,
            overage_count: 0.0,
            overage_permitted: false,
            reset_at: Some(now() + chrono::Duration::days(15)),
        }
    }

    fn sample_outcome(snap: CopilotSnapshot) -> VendorOutcome {
        VendorOutcome {
            snapshot: crate::usage::VendorSnapshot::Copilot(snap),
            stale: false,
            last_error: None,
            cache_age: Some(std::time::Duration::from_secs(10)),
        }
    }

    fn opts() -> RenderOpts {
        RenderOpts {
            format: None,
            tooltip_format: None,
            icon: None,
            pace_tolerance: 5,
            format_pace_color: false,
            tooltip_pace_pts: false,
        }
    }

    #[test]
    fn renders_pct_and_reset() {
        let snap = sample_snap();
        let out = render(
            &sample_outcome(snap.clone()),
            &snap,
            &Theme::default(),
            &opts(),
            now(),
        );
        assert!(out.text.contains("34%"));
        assert!(out.tooltip.contains("Premium requests"));
        assert!(out.tooltip.contains("Copilot Pro"));
        assert!(out.tooltip.contains("68 of 200 used"));
        // Usage-% vendors draw a filled progress bar rather than a bare number.
        assert!(
            out.tooltip.contains('█') || out.tooltip.contains('░'),
            "tooltip missing progress bar cells: {}",
            out.tooltip
        );
        assert!(out.tooltip.contains("Resets in"));
    }

    #[test]
    fn high_usage_is_critical() {
        let mut snap = sample_snap();
        snap.premium_pct = 95;
        assert_eq!(severity(&snap), PaceSeverity::Critical);
    }

    /// An unlimited plan has no bar to fill and must never read as exhausted.
    #[test]
    fn unlimited_plans_render_as_unlimited_and_stay_ok() {
        let mut snap = sample_snap();
        snap.unlimited = true;
        snap.premium_pct = 0;
        assert_eq!(severity(&snap), PaceSeverity::Low);
        let out = render(
            &sample_outcome(snap.clone()),
            &snap,
            &Theme::default(),
            &opts(),
            now(),
        );
        assert!(out.tooltip.contains(UNLIMITED));
        let ph = build_placeholders(&snap, now());
        assert_eq!(ph.get("cpl_remaining").map(String::as_str), Some(UNLIMITED));
    }

    #[test]
    fn overage_is_surfaced_when_the_plan_bills_past_the_entitlement() {
        let mut snap = sample_snap();
        snap.premium_pct = 100;
        snap.remaining = 0.0;
        snap.used = 212.0;
        snap.overage_count = 12.0;
        snap.overage_permitted = true;
        let out = render(
            &sample_outcome(snap.clone()),
            &snap,
            &Theme::default(),
            &opts(),
            now(),
        );
        assert!(
            out.tooltip.contains("12 billed over the plan"),
            "{}",
            out.tooltip
        );
    }

    #[test]
    fn an_exhausted_plan_without_overage_says_so() {
        let mut snap = sample_snap();
        snap.premium_pct = 100;
        snap.remaining = 0.0;
        let out = render(
            &sample_outcome(snap.clone()),
            &snap,
            &Theme::default(),
            &opts(),
            now(),
        );
        assert!(
            out.tooltip.contains("paused until the reset"),
            "{}",
            out.tooltip
        );
    }

    #[test]
    fn placeholders_include_generic_aliases() {
        let ph = build_placeholders(&sample_snap(), now());
        assert_eq!(ph.get("vendor_short").map(String::as_str), Some("cpl"));
        assert_eq!(ph.get("weekly_pct").map(String::as_str), Some("34"));
        assert_eq!(ph.get("session_pct").map(String::as_str), Some("34"));
        assert_eq!(ph.get("cpl_entitlement").map(String::as_str), Some("200"));
        assert_eq!(ph.get("cpl_account").map(String::as_str), Some("octocat"));
    }

    /// A fractional balance is real: the endpoint bills partial requests.
    #[test]
    fn fractional_counts_keep_one_decimal() {
        assert_eq!(requests(198.6), "198.6");
        assert_eq!(requests(1.4), "1.4");
        assert_eq!(requests(200.0), "200");
        assert_eq!(requests(199.98), "200");
    }
}
