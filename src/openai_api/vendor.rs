//! OpenAI Admin API renderer — trailing-30-day spend, optionally against a
//! configured monthly limit (there is no balance endpoint; the limit is a
//! config value, exactly like the Anthropic Admin vendor).

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::format::{placeholders, substitute, usd};
use crate::pacing::PaceSeverity;
use crate::pango::{color_span, escape, severity_color, severity_for};
use crate::theme::Theme;
use crate::tooltip::{Line as TooltipLine, render_bordered};
use crate::usage::OpenAiApiSnapshot;
use crate::vendor::{RenderOpts, VendorId, VendorOutcome};
use crate::waybar::{Class, WaybarOutput};


impl From<super::fetch::FetchOutcome> for VendorOutcome {
    fn from(o: super::fetch::FetchOutcome) -> Self {
        Self {
            snapshot: crate::usage::VendorSnapshot::OpenaiApi(o.snapshot),
            stale: o.stale,
            last_error: o.last_error,
            cache_age: o.cache_age,
        }
    }
}

pub const DEFAULT_FORMAT: &str = "{oai_headline}";

/// Bar headline: spend-vs-limit with a % when a limit is configured,
/// otherwise the trailing-30-day spend.
fn headline(snap: &OpenAiApiSnapshot) -> String {
    match snap.limit {
        Some(l) if l > 0.0 => format!(
            "{} / ${:.0} · {}%",
            usd(snap.spent),
            l,
            snap.pct().unwrap_or(0)
        ),
        _ => format!("{}/30d", usd(snap.spent)),
    }
}

pub fn build_placeholders(snap: &OpenAiApiSnapshot) -> HashMap<&'static str, String> {
    let pct = snap.pct().unwrap_or(0);
    placeholders(vec![
        ("icon", "󰢗".to_string()),
        ("vendor_short", VendorId::OpenaiApi.short_name().to_string()),
        // Cross-vendor aliases — spend% maps to the session/weekly slots.
        ("session_pct", pct.to_string()),
        ("session_reset", "—".to_string()),
        ("weekly_pct", pct.to_string()),
        ("weekly_reset", "—".to_string()),
        ("plan", "OpenAI API".to_string()),
        ("oai_headline", headline(snap)),
        ("oai_spent", usd(snap.spent)),
        (
            "oai_limit",
            snap.limit
                .map(|l| format!("${l:.0}"))
                .unwrap_or_else(|| "—".into()),
        ),
        ("oai_pct", pct.to_string()),
    ])
}

/// Severity keys on the spend-vs-limit %. With no limit there's no signal, so
/// it stays calm (low).
pub fn severity(snap: &OpenAiApiSnapshot) -> PaceSeverity {
    match snap.pct() {
        Some(p) => severity_for(p.min(100)),
        None => PaceSeverity::Low,
    }
}

pub fn render(
    outcome: &VendorOutcome,
    snap: &OpenAiApiSnapshot,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> WaybarOutput {
    let class = Class::from(severity(snap));
    let format = opts
        .format
        .clone()
        .unwrap_or_else(|| DEFAULT_FORMAT.to_string());
    let values = build_placeholders(snap);

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
    snap: &OpenAiApiSnapshot,
    theme: &Theme,
    now: DateTime<Utc>,
) -> String {
    let blue = &theme.blue;
    let dim = &theme.dim;
    let fg = &theme.fg;
    let color = severity_color(severity(snap), theme);

    let mut lines: Vec<TooltipLine> = Vec::new();
    lines.push(TooltipLine::Center(format!(
        "<span font_weight='bold' foreground='{blue}'>OpenAI API</span>"
    )));
    lines.push(TooltipLine::Sep);
    lines.push(TooltipLine::Body("".into()));

    lines.push(TooltipLine::Body(format!(
        " <span foreground='{fg}'>  󰉹  Spend, last 30 days</span>"
    )));
    lines.push(TooltipLine::Body(format!(
        "   <span font_weight='bold' foreground='{color}'>{spent}</span>",
        spent = escape(&usd(snap.spent))
    )));
    match snap.limit {
        Some(l) if l > 0.0 => {
            lines.push(TooltipLine::Body(format!(
                " <span foreground='{dim}'>     of ${l:.0} limit ({pct}%)</span>",
                pct = snap.pct().unwrap_or(0)
            )));
        }
        _ => {
            lines.push(TooltipLine::Body(format!(
                " <span foreground='{dim}'>     no monthly limit set (add `monthly_limit` under [openai_api])</span>"
            )));
        }
    }

    if !snap.top_items.is_empty() {
        lines.push(TooltipLine::Body("".into()));
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{dim}'>  󰋎  Top line items</span>"
        )));
        for (name, value) in snap.top_items.iter().take(5) {
            lines.push(TooltipLine::Body(format!(
                " <span foreground='{dim}'>     {} — {}</span>",
                escape(name),
                escape(&usd(*value))
            )));
        }
    }

    lines.push(TooltipLine::Body("".into()));
    lines.push(TooltipLine::Body(format!(
        " <span foreground='{dim}'>  󰋼  spend consumed, not balance —</span>"
    )));
    lines.push(TooltipLine::Body(format!(
        " <span foreground='{dim}'>     the Admin API has no balance endpoint</span>"
    )));

    if let Some((code, msg)) = outcome.last_error.as_ref() {
        let (icon, ecolor, header) = if *code == 0 {
            ("󰀪", theme.orange.as_str(), "Sync error".to_string())
        } else if *code >= 500 {
            ("󰅚", theme.red.as_str(), format!("HTTP {code}"))
        } else {
            (
                "󰀪",
                theme.red.as_str(),
                format!("HTTP {code} — Admin key required (organization settings); a project sk- key is rejected here"),
            )
        };
        lines.push(TooltipLine::Body("".into()));
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{ecolor}'>{icon}  {header}</span>"
        )));
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{dim}'>     {}</span>",
            escape(msg)
        )));
    }

    let _ = now;
    render_bordered(&lines, theme)
}
