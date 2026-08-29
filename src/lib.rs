//! ai-usagebar library — shared core for the Waybar widget and TUI binaries.
//!
//! The crate is organized by concern, not by binary:
//! - low-level primitives (`cache`, `countdown`, `pacing`, `pango`, `theme`)
//! - the vendor abstraction (`vendor`, `vendors::*`, `usage`)
//! - bin-specific composition (`widget`, `tui`) which lives next to its binary
//!
//! The two binaries (`ai-usagebar` and `ai-usagebar-tui`) are thin: they parse
//! CLI args, instantiate vendors, and hand off to a renderer in this crate.

pub mod account;
pub mod active;
pub mod anthropic;
pub mod anthropic_api;
pub mod antigravity;
pub mod cache;
pub mod claude_desktop;
pub mod config;
pub mod context;
pub mod countdown;
pub mod cursor;
pub mod deepseek;
pub mod diag;
pub mod display;
pub mod error;
pub mod format;
pub mod grok;
/// Source-scanning helpers for structural guard tests. Test-only.
#[cfg(test)]
pub(crate) mod guard;
pub mod kilo;
pub mod kimi;
pub mod kiro;
pub mod minimax;
pub mod moonshot;
pub mod novita;
pub mod openai;
pub mod opencode_go;
pub mod openrouter;
pub mod pacing;
pub mod pango;
pub mod report;
pub mod safe_storage;
pub mod supergrok;
pub mod theme;
pub mod tooltip;
pub mod tui;
pub mod usage;
pub mod vendor;
pub mod waybar;
pub mod widget;
pub mod zai;

pub use error::{AppError, Result};

/// This project's directory name under the platform's config and cache
/// roots (`~/.config/ai-usagebar-omarchy`, `~/.cache/ai-usagebar-omarchy`).
/// Distinct from the upstream `ai-usagebar` project this repository forked
/// from, so both can be installed side by side without sharing config,
/// credentials, lock files, or caches. The single source for every path
/// builder — a second literal is how the two installs start colliding again.
pub const APP_DIR: &str = "ai-usagebar-omarchy";

/// The upstream/pre-rename directory name. Read-honored and one-time
/// migrated (see `config::migrate_legacy_layout`) for installs that predate
/// the fork; never chosen for new writes.
pub const LEGACY_APP_DIR: &str = "ai-usagebar";
