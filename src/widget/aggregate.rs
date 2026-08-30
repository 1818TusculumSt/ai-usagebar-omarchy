//! `--vendor all` — the aggregate bar: every enabled vendor and account,
//! tiled side by side in one Waybar module. This is the presentation the old
//! GNOME panel extension used (`key1 ◔42% ◔15% │ key2 ◔80% …`), rebuilt on
//! this project's report projection so the hover popup shows the same rows
//! every other frontend shows.
//!
//! Pure functions over `(TabId, TabState)` pairs so the layout is unit
//! testable without I/O; `run.rs` owns fetching (sequentially — several
//! vendors share per-vendor cache locks, and firing every account at a
//! provider at once is a good way to get rate-limited for no gain).

use chrono::{DateTime, Utc};

use crate::pacing::PaceSeverity;
use crate::pango::{color_span, escape, severity_color};
use crate::theme::Theme;
use crate::tooltip::{Line as TooltipLine, render_bordered};
use crate::tui::app::{TabId, TabState};
use crate::tui::panels::{Section, SectionProjection, sections_with_metadata_for};
use crate::vendor::RenderOpts;
use crate::waybar::{Class, WaybarOutput};

/// The pacing tolerance the report and the TUI panels use.
const PACE_TOLERANCE: u32 = 5;

/// One account's tile: everything the bar line and the popup block need.
#[derive(Debug, Clone, PartialEq)]
pub struct AggregateEntry {
    /// Bar tag — the account label (name ONLY, the provider stays in the
    /// popup), or the provider's name for default accounts (`kimi 37%`).
    /// Same rule the QML tile applies.
    pub label: String,
    /// Popup header — display name, plus the account label when named.
    pub title: String,
    /// The tile's figures in order (5h window first, then weekly), each
    /// carrying its own reset. Falls back to the single headline figure
    /// for accounts without windows (balances).
    pub figures: Vec<TileFigure>,
    /// The headline figure ("42%", "$1.23"); `None` marks an error row.
    /// Kept beside `figures` for severity/balance consumers.
    pub headline: Option<String>,
    pub severity: PaceSeverity,
    pub error: Option<String>,
    /// Routine not-configured state — calm styling, never drives the
    /// module's alert class (same classification the report ships).
    pub unconfigured: bool,
    /// Every reported window maxed / balance spent (the report's
    /// `exhausted` rule). The BAR omits exhausted tiles — the account has
    /// no capacity left, and its reset arrives with the next report; the
    /// popup keeps the row so the state stays visible on hover.
    pub exhausted: bool,
    pub stale: bool,
    pub fetched_at: Option<DateTime<Utc>>,
    /// Projected rows shared with the TUI/report (metrics carry their own
    /// severity and absolute reset).
    pub sections: Vec<SectionRow>,
}

/// The subset of a projected section the popup renders.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionRow {
    pub label: String,
    pub percent: Option<u16>,
    pub value: String,
    pub severity: PaceSeverity,
    pub reset_at: Option<DateTime<Utc>>,
    /// Server-classified window ("session"/"weekly"), straight from the
    /// section projection — this file never re-derives it.
    pub window: Option<&'static str>,
}

impl AggregateEntry {
    /// Build one entry from a refreshed tab. Mirrors the Omarchy panel's
    /// `Model.headline` rule so the bar tag and the QML tile can never
    /// disagree about which figure is "the" figure: the highest-percent
    /// metric wins, money-style metrics show their value instead of a %.
    pub fn from_tab(tab: &TabId, state: &TabState, now: DateTime<Utc>) -> Self {
        let vendor_name = tab.vendor.display_name();
        let title = match &tab.account {
            Some(label) => format!("{vendor_name} · {label}"),
            None => vendor_name.to_string(),
        };
        let label = tab
            .account
            .clone()
            .unwrap_or_else(|| tab.vendor.slug().to_string());

        let (error, unconfigured) = match state {
            TabState::Error(message) => {
                let unconfigured = crate::report::is_unconfigured_error(message);
                (Some(message.clone()), unconfigured)
            }
            _ => (None, false),
        };
        let (stale, fetched_at) = match state {
            TabState::Ready(ready) => (ready.stale, ready.fetched_at),
            _ => (false, None),
        };

        let mut sections = Vec::new();
        // Like the report: an error is already a first-class field, and the
        // sections an Error tab projects carry the TUI's interactive retry
        // instructions — not data for a bar tooltip.
        let mut best: Option<HeadlineMetric> = None;
        if error.is_none() {
            for projected in sections_with_metadata_for(state, now, PACE_TOLERANCE) {
                let SectionProjection {
                    section,
                    reset_at,
                    window,
                } = projected;
                match section {
                    Section::Metric {
                        label,
                        pct,
                        severity,
                        value_label,
                        ..
                    } => {
                        if best.as_ref().is_none_or(|metric| pct > metric.pct) {
                            best = Some(HeadlineMetric {
                                label: label.clone(),
                                pct,
                                value: value_label.clone(),
                                severity,
                            });
                        }
                        sections.push(SectionRow {
                            label,
                            percent: Some(pct),
                            value: value_label,
                            severity,
                            reset_at,
                            window,
                        });
                    }
                    Section::Text { label, value } => {
                        sections.push(SectionRow {
                            label,
                            percent: None,
                            value,
                            severity: PaceSeverity::Low,
                            reset_at,
                            window,
                        });
                    }
                    _ => {}
                }
            }
        }

        // Error rows keep their sections (there are none today) but lose the
        // headline: the tile shows the warning mark instead.
        let headline = if error.is_some() {
            None
        } else {
            headline_figure(best.as_ref(), &sections)
        };
        let severity = best
            .as_ref()
            .map(|metric| metric.severity)
            .unwrap_or(PaceSeverity::Low);
        let figures = if error.is_some() {
            Vec::new()
        } else {
            tile_figures(&sections, best.as_ref())
        };
        // Same rule the report ships (`report::rows_exhausted`): any maxed
        // window or spent balance hides the tile from the bar.
        let exhausted = error.is_none()
            && crate::report::rows_exhausted(sections.iter().map(|row| {
                (
                    row.label.as_str(),
                    row.percent,
                    row.value.as_str(),
                )
            }));

        Self {
            label,
            title,
            headline,
            figures,
            severity,
            error,
            unconfigured,
            exhausted,
            stale,
            fetched_at,
            sections,
        }
    }
}

/// One bar-tile figure: a percentage plus its window's reset.
#[derive(Debug, Clone, PartialEq)]
pub struct TileFigure {
    pub text: String,
    pub reset_at: Option<DateTime<Utc>>,
    /// A window figure (as opposed to the balance fallback): it owns a
    /// countdown slot, dashed when the window has not started.
    pub is_window: bool,
}

/// The tile's figures: the 5h window first, then the weekly one, each with
/// its own reset; the headline figure when neither exists.
fn tile_figures(sections: &[SectionRow], best: Option<&HeadlineMetric>) -> Vec<TileFigure> {
    let mut session = None;
    let mut weekly = None;
    for row in sections {
        match row.window {
            Some("session") if session.is_none() => session = Some(row),
            Some("weekly") if weekly.is_none() => weekly = Some(row),
            _ => {}
        }
    }
    let picked: Vec<&SectionRow> = session.into_iter().chain(weekly).collect();
    if !picked.is_empty() {
        return picked
            .into_iter()
            .map(|row| TileFigure {
                text: row.value.clone(),
                reset_at: row.reset_at,
                is_window: true,
            })
            .collect();
    }
    // No windows (balance-style accounts): the single headline figure.
    best.and_then(|metric| {
        let text = headline_figure(Some(metric), &[]);
        text.map(|text| {
            vec![TileFigure {
                text,
                reset_at: None,
                is_window: false,
            }]
        })
    })
    .unwrap_or_default()
}

/// The fullest metric, as picked by [`AggregateEntry::from_tab`] — what
/// drives severity and the no-window fallback.
struct HeadlineMetric {
    label: String,
    pct: u16,
    value: String,
    severity: PaceSeverity,
}

/// The single figure a tile leads with. Rule shared with `Model.headline`:
/// the fullest metric's percent, except balance-style metrics whose value
/// string is the figure.
fn headline_figure(best: Option<&HeadlineMetric>, sections: &[SectionRow]) -> Option<String> {
    if let Some(HeadlineMetric {
        label,
        pct,
        value,
        ..
    }) = best
    {
        return Some(
            if label.to_lowercase().contains("balance") && !value.is_empty() {
                value.clone()
            } else {
                format!("{pct}%")
            },
        );
    }
    sections
        .iter()
        .find(|row| {
            row.percent.is_none()
                && !row.value.is_empty()
                && ["balance", "available", "spend", "prepaid"]
                    .iter()
                    .any(|word| row.label.to_lowercase().contains(word))
        })
        .map(|row| row.value.clone())
}

/// `PaceSeverity` has no ordering (it is presentation, not magnitude); the
/// aggregate needs one to pick the module's worst class.
fn severity_rank(severity: PaceSeverity) -> u8 {
    match severity {
        PaceSeverity::Low => 0,
        PaceSeverity::Mid => 1,
        PaceSeverity::High => 2,
        PaceSeverity::Critical => 3,
    }
}

/// Render the full aggregate output: bar text + combined bordered tooltip.
pub fn render(
    entries: &[AggregateEntry],
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> WaybarOutput {
    let class = worst_class(entries);
    let text = render_bar_text(entries, theme, opts, now);
    let tooltip = render_tooltip(entries, theme, now);
    WaybarOutput {
        text,
        tooltip,
        class,
    }
}

fn worst_class(entries: &[AggregateEntry]) -> Class {
    entries
        .iter()
        .map(|entry| {
            if entry.error.is_some() {
                if entry.unconfigured {
                    // Routine absence (no key, no login): calm, same as the
                    // QML bar — the popup carries the remedy.
                    PaceSeverity::Low
                } else {
                    // An unreachable account is at least as urgent as a
                    // maxed window — the tile is the alert for it.
                    PaceSeverity::Critical
                }
            } else {
                entry.severity
            }
        })
        .max_by_key(|severity| severity_rank(*severity))
        .map(Class::from)
        .unwrap_or(Class::Low)
}

/// `icon label 42%·5.3h │ …` — one severity-colored span per *working,
/// non-exhausted* account, joined with a vertical bar like the GNOME panel's
/// groups. Each tile carries its window figures with compact countdowns
/// ("42%·5.3h 15%·2.3d"), the panel presentation the old extension used.
/// Misconfigured accounts (no key, broken config) are deliberately absent
/// from the bar — the popup keeps their rows and remedies — and so are
/// exhausted ones (any window maxed, balance spent): a tile that is all
/// zeros is noise until its reset lands. Both stay in the popup; a bar is
/// not the place to nag.
fn render_bar_text(entries: &[AggregateEntry], theme: &Theme, opts: &RenderOpts, now: DateTime<Utc>) -> String {
    let tiles: Vec<String> = entries
        .iter()
        .filter(|entry| entry.error.is_none() && !entry.exhausted)
        .map(|entry| {
            let color = severity_color(entry.severity, theme);
            let tile = format!("{} {}", escape(&entry.label), {
                if entry.figures.is_empty() {
                    "—".to_string()
                } else {
                    entry
                        .figures
                        .iter()
                        .map(|figure| {
                            if !figure.is_window {
                                return escape(&figure.text);
                            }
                            // The compact one-unit countdown ("5.3h" of
                            // "5h 20m") — a bar tile has room for one. A
                            // window that has not started keeps its slot
                            // with a dash instead of looking like a missing
                            // timer.
                            let reset = figure
                                .reset_at
                                .map(|at| crate::countdown::format_compact(Some(at), now))
                                .unwrap_or_else(|| "-".to_string());
                            format!("{}·{}", escape(&figure.text), reset)
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
            });
            color_span(color, &tile)
        })
        .collect();
    if tiles.is_empty() {
        // Every account failed: no tiles to show, but never an empty module.
        return match opts.icon.as_deref() {
            Some(ic) if !ic.is_empty() => ic.to_string(),
            _ => "󰚩".to_string(),
        };
    }
    // No padding spaces around the divider — the │ glyph has whitespace of
    // its own, and a bar stays readable only while it stays narrow.
    let joined = tiles.join("│");
    let icon_prefix = match opts.icon.as_deref() {
        Some(ic) if !ic.is_empty() => format!("{ic} "),
        _ => String::new(),
    };
    format!("{icon_prefix}{joined}")
}

/// One bordered box: a header block per account followed by its metric rows
/// — the same rows the TUI panel and `usage --json` carry, so hover content
/// matches what every other frontend of this project shows.
fn render_tooltip(entries: &[AggregateEntry], theme: &Theme, now: DateTime<Utc>) -> String {
    let blue = &theme.blue;
    let dim = &theme.dim;
    let mut lines: Vec<TooltipLine> = Vec::new();

    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            lines.push(TooltipLine::Body(String::new()));
            lines.push(TooltipLine::Sep);
            lines.push(TooltipLine::Body(String::new()));
        }
        let title = match (&entry.error, entry.stale) {
            (Some(_), _) => format!(
                "{}  <span foreground='{dim}'>·</span> <span foreground='{}'>unavailable</span>",
                escape(&entry.title),
                theme.red
            ),
            (None, true) => format!(
                "{}  <span foreground='{dim}'>· cached</span>",
                escape(&entry.title)
            ),
            (None, false) => escape(&entry.title),
        };
        lines.push(TooltipLine::Center(format!(
            "<span font_weight='bold' foreground='{blue}'>{title}</span>"
        )));

        if let Some(error) = &entry.error {
            lines.push(TooltipLine::Body(format!(
                " <span foreground='{}'>  {}</span>",
                theme.red,
                escape(error)
            )));
        }
        for row in &entry.sections {
            let color = severity_color(row.severity, theme);
            let reset = row
                .reset_at
                .map(|at| {
                    format!(
                        " <span foreground='{dim}'>· reset {}</span>",
                        crate::countdown::format(Some(at), now)
                    )
                })
                .unwrap_or_default();
            match row.percent {
                // Window metrics carry "42%" as their value already — print
                // one figure, not `42% 42%`. The bare percent is the fallback
                // for metrics whose value string is empty.
                Some(pct) => {
                    let figure = if row.value.is_empty() {
                        format!("<b>{pct:>3}%</b>")
                    } else {
                        format!("<b>{}</b>", escape(&row.value))
                    };
                    lines.push(TooltipLine::Body(format!(
                        "  {}  <span foreground='{color}'>{figure}</span>{}",
                        escape(&row.label),
                        reset,
                    )));
                }
                None => {
                    if !row.value.is_empty() {
                        lines.push(TooltipLine::Body(format!(
                            "  {}  <span foreground='{dim}'>{}</span>",
                            escape(&row.label),
                            escape(&row.value)
                        )));
                    }
                }
            }
        }
    }

    render_bordered(&lines, theme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{UsageWindow, ZaiSnapshot};
    use crate::vendor::VendorId;

    fn window(pct: i32) -> UsageWindow {
        UsageWindow {
            utilization_pct: pct,
            resets_at: None,
            window_duration: chrono::Duration::hours(5),
        }
    }

    fn zai_state(session: i32) -> TabState {
        TabState::Ready(Box::new(crate::tui::app::ReadyTab {
            snapshot: crate::usage::VendorSnapshot::Zai(ZaiSnapshot {
                plan: "GLM Coding Pro".into(),
                session: Some(window(session)),
                weekly: None,
                mcp: None,
                usage_stats: None,
            }),
            stale: false,
            last_error: None,
            fetched_at: None,
        }))
    }

    fn kimi_state(weekly: i32) -> TabState {
        TabState::Ready(Box::new(crate::tui::app::ReadyTab {
            snapshot: crate::usage::VendorSnapshot::Kimi(crate::usage::KimiSnapshot {
                plan: None,
                weekly_limit: 100,
                weekly_used: weekly as u64,
                weekly_remaining: 100 - weekly as u64,
                weekly_reset_at: None,
                window_limit: 40,
                window_used: 0,
                window_remaining: 40,
                window_reset_at: None,
            }),
            stale: false,
            last_error: None,
            fetched_at: None,
        }))
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
    fn tiles_one_severity_colored_span_per_account() {
        let now = Utc::now();
        let entries = vec![
            AggregateEntry::from_tab(
                &TabId::account_for(VendorId::Zai, "team".to_string()),
                &zai_state(42),
                now,
            ),
            AggregateEntry::from_tab(&TabId::vendor(VendorId::Kimi), &kimi_state(95), now),
        ];
        let text = render_bar_text(&entries, &Theme::default(), &opts(), now);
        // Tags: the account label (name only) for named accounts, the
        // provider name for default ones; figures; the separator between
        // tiles.
        assert!(text.contains("team"), "{text}");
        assert!(text.contains("42%"), "{text}");
        assert!(text.contains("kimi"), "{text}");
        assert!(!text.contains("kmi "), "short code leaked: {text}");
        assert!(text.contains("95%"), "{text}");
        assert!(text.contains("│"), "{text}");
        // Severity colors differ (42 → low/green, 95 → critical/red).
        assert!(text.contains('#'), "expected pango color spans: {text}");
    }

    /// Misconfigured accounts never tile the bar (requirement: no-key and
    /// broken-config entries stay out of sight) — but they keep their popup
    /// row with the remedy and still drive the module's alert class.
    #[test]
    fn error_accounts_stay_off_the_bar_but_drive_the_class() {
        let now = Utc::now();
        let entries = vec![
            AggregateEntry::from_tab(&TabId::vendor(VendorId::Zai), &zai_state(10), now),
            AggregateEntry::from_tab(
                &TabId::vendor(VendorId::Kimi),
                &TabState::Error("HTTP 401".into()),
                now,
            ),
        ];
        let text = render_bar_text(&entries, &Theme::default(), &opts(), now);
        assert!(!text.contains("kmi"), "error tile leaked into the bar: {text}");
        assert!(!text.contains("⚠"), "{text}");
        assert!(text.contains("zai"), "{text}");
        assert_eq!(worst_class(&entries), Class::Critical);

        let tooltip = render_tooltip(&entries, &Theme::default(), now);
        assert!(tooltip.contains("unavailable"), "{tooltip}");
        assert!(tooltip.contains("HTTP 401"), "{tooltip}");
    }

    /// Every account broken → no tiles, but the module never goes empty.
    #[test]
    fn all_error_entries_leave_the_icon_alone() {
        let now = Utc::now();
        let entries = vec![AggregateEntry::from_tab(
            &TabId::vendor(VendorId::Kimi),
            &TabState::Error("no key".into()),
            now,
        )];
        let text = render_bar_text(&entries, &Theme::default(), &opts(), now);
        assert!(!text.is_empty());
        assert!(!text.contains("│"), "{text}");
    }

    /// Tiles carry each window's own reset countdown, compact one-unit form:
    /// the 5h figure first, then the weekly one ("42%·2h 15%·2.1d").
    #[test]
    fn tiles_carry_both_windows_with_their_resets() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 8, 29, 12, 0, 0).unwrap();
        let mut state = zai_state(42);
        let TabState::Ready(ready) = &mut state else {
            unreachable!()
        };
        if let crate::usage::VendorSnapshot::Zai(snap) = &mut ready.snapshot {
            snap.session.as_mut().unwrap().resets_at =
                Some(now + chrono::Duration::hours(2) + chrono::Duration::minutes(5));
            snap.weekly = Some(crate::usage::UsageWindow {
                utilization_pct: 15,
                resets_at: Some(now + chrono::Duration::days(2) + chrono::Duration::hours(3)),
                window_duration: chrono::Duration::days(7),
            });
        }
        let entries = vec![AggregateEntry::from_tab(&TabId::vendor(VendorId::Zai), &state, now)];
        let text = render_bar_text(&entries, &Theme::default(), &opts(), now);
        assert!(text.contains("42%·2h"), "{text}");
        assert!(text.contains("15%·2.1d"), "{text}");
        assert!(!text.contains(" · "), "old single-figure spacing leaked: {text}");
    }

    /// A window that has not started (no reset reported) keeps its countdown
    /// slot with a dash — `0%·-` — instead of looking like a missing timer.
    #[test]
    fn unstarted_window_keeps_a_dash_placeholder() {
        let now = Utc::now();
        let state = zai_state(0);
        let entries = vec![AggregateEntry::from_tab(&TabId::vendor(VendorId::Zai), &state, now)];
        let text = render_bar_text(&entries, &Theme::default(), &opts(), now);
        assert!(text.contains("0%·-"), "{text}");
    }

    /// An exhausted account (any window at 100) leaves the bar but keeps its
    /// popup rows — the reset arrives with the next report, and until then a
    /// maxed tile is pure noise. Recovery needs no state: the next snapshot
    /// under 100 simply tiles again.
    #[test]
    fn exhausted_accounts_leave_the_bar_but_keep_their_popup_rows() {
        let now = Utc::now();
        let entries = vec![
            AggregateEntry::from_tab(&TabId::vendor(VendorId::Zai), &zai_state(100), now),
            AggregateEntry::from_tab(&TabId::vendor(VendorId::Kimi), &kimi_state(30), now),
        ];
        assert!(entries[0].exhausted);
        assert!(!entries[1].exhausted);
        let text = render_bar_text(&entries, &Theme::default(), &opts(), now);
        assert!(!text.contains("100%"), "exhausted tile leaked into the bar: {text}");
        assert!(!text.contains("│"), "a single tile must not draw a separator: {text}");
        assert!(text.contains("30%"), "{text}");

        // The popup still shows the maxed account with its figures.
        let tooltip = render_tooltip(&entries, &Theme::default(), now);
        assert!(tooltip.contains("Z.AI"), "{tooltip}");
        assert!(tooltip.contains("100%"), "{tooltip}");
    }

    /// Every working account exhausted → no tiles, but the module never goes
    /// empty (same fallback as every account erroring).
    #[test]
    fn all_exhausted_entries_leave_the_icon_alone() {
        let now = Utc::now();
        let entries = vec![AggregateEntry::from_tab(
            &TabId::vendor(VendorId::Zai),
            &zai_state(100),
            now,
        )];
        let text = render_bar_text(&entries, &Theme::default(), &opts(), now);
        assert!(!text.is_empty());
        assert!(!text.contains("│"), "{text}");
    }

    #[test]
    fn tooltip_lists_every_account_with_its_rows() {
        let now = Utc::now();
        let entries = vec![
            AggregateEntry::from_tab(&TabId::vendor(VendorId::Zai), &zai_state(42), now),
            AggregateEntry::from_tab(
                &TabId::account_for(VendorId::Kimi, "work".to_string()),
                &kimi_state(20),
                now,
            ),
        ];
        let tooltip = render_tooltip(&entries, &Theme::default(), now);
        assert!(tooltip.contains("Z.AI"), "{tooltip}");
        assert!(tooltip.contains("Kimi · work"), "{tooltip}");
        assert!(tooltip.contains("42%"), "{tooltip}");
    }

    #[test]
    fn balance_style_metrics_lead_with_their_value_not_a_percent() {
        let now = Utc::now();
        // OpenRouter's first metric is "Credit balance" with a dollar value.
        let state = TabState::Ready(Box::new(crate::tui::app::ReadyTab {
            snapshot: crate::usage::VendorSnapshot::Openrouter(crate::usage::OpenRouterSnapshot {
                label: "OpenRouter".into(),
                total_credits: 10.0,
                total_usage: 8.66,
                usage_daily: 0.0,
                usage_weekly: 0.0,
                usage_monthly: 0.0,
                is_free_tier: false,
                limit: None,
                limit_remaining: None,
            }),
            stale: false,
            last_error: None,
            fetched_at: None,
        }));
        let entry = AggregateEntry::from_tab(&TabId::vendor(VendorId::Openrouter), &state, now);
        assert_eq!(entry.headline.as_deref(), Some("$1.34"));
    }
}
