//! Settings overlay — opened from the TUI by pressing `s`. Lets the user pick
//! the primary vendor and paste an API key for any key-authenticated vendor
//! (including Z.AI, Kimi, MiniMax, and the balance vendors) without hand-editing
//! config.toml. Anthropic, OpenAI, Cursor, and Antigravity authenticate through
//! local product state, so they have no key field here. Kimi keeps its key
//! field because a platform key is still one of its two credentials, but a
//! subscriber whose credential is the Kimi Code CLI login has nothing to paste
//! and enables `[kimi]` in config.toml instead.
//!
//! Persistence uses `toml_edit` so the existing config keeps its comments,
//! whitespace, and unrelated fields. Writing a key also flips that vendor's
//! `enabled = true` (the opt-in vendors are disabled by default), so "paste the
//! key and save" is all it takes. Files with inline keys are atomically written
//! and `chmod 600`ed.

use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui_bubbletea_theme::BubbleTheme;
use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, value};

use crate::config::{Config, ZaiAccountType};
use crate::error::{AppError, Result};
use crate::theme::Theme;
use crate::tui::style::bubble_theme;
use crate::vendor::VendorId;

/// A vendor that authenticates with an inline API key (vs. OAuth). The order of
/// this table is the tab order of the key fields and the layout of the state's
/// `keys` vec.
pub struct KeyVendor {
    pub id: VendorId,
    pub label: &'static str,
    pub env: &'static str,
    pub section: &'static str,
    /// Extra hint after the env var (e.g. "management key"). Empty for none.
    pub note: &'static str,
}

pub const KEY_VENDORS: &[KeyVendor] = &[
    KeyVendor {
        id: VendorId::AnthropicApi,
        label: "Anthropic API",
        env: "ANTHROPIC_ADMIN_KEY",
        section: "anthropic_api",
        note: "admin key — monthly spend",
    },
    KeyVendor {
        id: VendorId::Zai,
        label: "Z.AI",
        env: "ZAI_API_KEY",
        section: "zai",
        note: "",
    },
    KeyVendor {
        id: VendorId::Openrouter,
        label: "OpenRouter",
        env: "OPENROUTER_API_KEY",
        section: "openrouter",
        note: "",
    },
    KeyVendor {
        id: VendorId::Deepseek,
        label: "DeepSeek",
        env: "DEEPSEEK_API_KEY",
        section: "deepseek",
        note: "",
    },
    KeyVendor {
        id: VendorId::Kimi,
        label: "Kimi",
        env: "KIMI_API_KEY",
        section: "kimi",
        note: "coding-plan usage",
    },
    KeyVendor {
        id: VendorId::Kilo,
        label: "Kilo",
        env: "KILO_API_KEY",
        section: "kilo",
        note: "",
    },
    KeyVendor {
        id: VendorId::Novita,
        label: "Novita",
        env: "NOVITA_API_KEY",
        section: "novita",
        note: "",
    },
    KeyVendor {
        id: VendorId::Moonshot,
        label: "Moonshot",
        env: "MOONSHOT_API_KEY",
        section: "moonshot",
        note: "account balance",
    },
    KeyVendor {
        id: VendorId::Grok,
        label: "Grok",
        env: "XAI_MANAGEMENT_KEY",
        section: "grok",
        note: "management key, not the inference key",
    },
    KeyVendor {
        id: VendorId::Minimax,
        label: "MiniMax",
        env: "MINIMAX_API_KEY",
        section: "minimax",
        note: "Token Plan subscription key",
    },
    KeyVendor {
        id: VendorId::OpenCodeGo,
        label: "OpenCode Go",
        env: "OPENCODE_GO_API_KEY",
        section: "opencode-go",
        note: "usage quota",
    },
];

/// Read the inline `api_key` currently in config for a given section, so the
/// field opens pre-filled (masked) when one is already set.
fn config_inline_key<'a>(cfg: &'a Config, section: &str) -> Option<&'a str> {
    match section {
        "anthropic_api" => cfg.anthropic_api.api_key.as_deref(),
        "zai" => cfg.zai.api_key.as_deref(),
        "openrouter" => cfg.openrouter.api_key.as_deref(),
        "deepseek" => cfg.deepseek.api_key.as_deref(),
        "kimi" => cfg.kimi.api_key.as_deref(),
        "kilo" => cfg.kilo.api_key.as_deref(),
        "novita" => cfg.novita.api_key.as_deref(),
        "moonshot" => cfg.moonshot.api_key.as_deref(),
        "grok" => cfg.grok.api_key.as_deref(),
        "minimax" => cfg.minimax.api_key.as_deref(),
        "opencode-go" => cfg.opencode_go.api_key.as_deref(),
        _ => None,
    }
}

/// Which control has keyboard focus. `Key(i)` indexes into [`KEY_VENDORS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Primary,
    Key(usize),
    Save,
}

impl Focus {
    pub fn next(self) -> Self {
        match self {
            Focus::Primary => Focus::Key(0),
            Focus::Key(i) if i + 1 < KEY_VENDORS.len() => Focus::Key(i + 1),
            Focus::Key(_) => Focus::Save,
            Focus::Save => Focus::Primary,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Focus::Primary => Focus::Save,
            Focus::Key(0) => Focus::Primary,
            Focus::Key(i) => Focus::Key(i - 1),
            Focus::Save => Focus::Key(KEY_VENDORS.len() - 1),
        }
    }
}

/// Per-field text-input state — cursor + buffer + reveal flag.
#[derive(Debug, Clone, Default)]
pub struct KeyInput {
    pub buf: String,
    /// Char-index cursor position (0..=buf.chars().count()).
    pub cursor: usize,
    /// When true, the field renders the actual characters; otherwise `•`.
    pub revealed: bool,
    /// True after the user has typed/edited; only then does save write the
    /// value back (avoids clobbering an existing key with the empty
    /// placeholder the user opened the dialog with).
    pub dirty: bool,
}

impl KeyInput {
    pub fn from_config(initial: Option<&str>) -> Self {
        let buf = initial.unwrap_or("").to_string();
        let cursor = buf.chars().count();
        Self {
            buf,
            cursor,
            revealed: false,
            dirty: false,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let byte_idx = self.char_to_byte(self.cursor);
        self.buf.insert(byte_idx, c);
        self.cursor += 1;
        self.dirty = true;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev_byte = self.char_to_byte(self.cursor - 1);
        let cur_byte = self.char_to_byte(self.cursor);
        self.buf.replace_range(prev_byte..cur_byte, "");
        self.cursor -= 1;
        self.dirty = true;
    }

    pub fn delete(&mut self) {
        let n = self.buf.chars().count();
        if self.cursor >= n {
            return;
        }
        let cur_byte = self.char_to_byte(self.cursor);
        let next_byte = self.char_to_byte(self.cursor + 1);
        self.buf.replace_range(cur_byte..next_byte, "");
        self.dirty = true;
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }
    pub fn move_right(&mut self) {
        if self.cursor < self.buf.chars().count() {
            self.cursor += 1;
        }
    }
    pub fn move_home(&mut self) {
        self.cursor = 0;
    }
    pub fn move_end(&mut self) {
        self.cursor = self.buf.chars().count();
    }
    pub fn toggle_reveal(&mut self) {
        self.revealed = !self.revealed;
    }

    /// Render for display — bullets when masked, raw chars when revealed.
    pub fn display(&self) -> String {
        if self.revealed {
            self.buf.clone()
        } else {
            "•".repeat(self.buf.chars().count())
        }
    }

    fn char_to_byte(&self, char_idx: usize) -> usize {
        self.buf
            .char_indices()
            .map(|(b, _)| b)
            .chain(std::iter::once(self.buf.len()))
            .nth(char_idx)
            .unwrap_or(self.buf.len())
    }
}

/// Mutable state of the overlay while open.
#[derive(Debug, Clone)]
pub struct SettingsState {
    pub focus: Focus,
    /// Enabled vendors only. The primary selector must not offer a value that
    /// cannot actually be used by the widget or TUI.
    pub primary_choices: Vec<VendorId>,
    pub primary: VendorId,
    /// One input per [`KEY_VENDORS`] entry, same order.
    pub keys: Vec<KeyInput>,
    /// Non-secret Z.AI account fields (billing type, site, team ids). The
    /// terminal overlay round-trips them untouched; the native settings form
    /// edits them. Saving always writes them back to `[zai]`.
    pub zai: ZaiFields,
    /// One-line status displayed in the footer ("saved …", "save failed …").
    pub status: String,
}

/// The editable, non-secret Z.AI fields — what the old GNOME panel exposed
/// as the per-key "Account type" selector plus its dependent team inputs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZaiFields {
    /// `personal` | `team` | `usage` (kept as a string so the bridge stays
    /// presentation-agnostic; validated before it reaches a config).
    pub account_type: String,
    /// `""` (unset) | `global` | `cn`.
    pub site: String,
    pub organization_id: String,
    pub project_id: String,
}

impl ZaiFields {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            account_type: match cfg.zai.account_type {
                crate::config::ZaiAccountType::Personal => "personal",
                crate::config::ZaiAccountType::Team => "team",
                crate::config::ZaiAccountType::Usage => "usage",
            }
            .to_string(),
            site: match cfg.zai.site {
                Some(crate::config::ZaiSite::Global) => "global".to_string(),
                Some(crate::config::ZaiSite::Cn) => "cn".to_string(),
                None => String::new(),
            },
            organization_id: cfg.zai.organization_id.clone().unwrap_or_default(),
            project_id: cfg.zai.project_id.clone().unwrap_or_default(),
        }
    }
}

impl SettingsState {
    pub fn from_config(cfg: &Config) -> Self {
        let keys = KEY_VENDORS
            .iter()
            .map(|kv| KeyInput::from_config(config_inline_key(cfg, kv.section)))
            .collect();
        let primary_choices = cfg.enabled_vendors();
        // A configured but disabled primary is ineffective. Display the first
        // enabled vendor instead; when none are enabled retain the historical
        // Anthropic fallback in memory without inventing a persisted primary.
        let primary = cfg
            .ui
            .primary
            .filter(|vendor| primary_choices.contains(vendor))
            .or_else(|| primary_choices.first().copied())
            .unwrap_or_else(|| cfg.ui.primary.unwrap_or(VendorId::Anthropic));
        Self {
            focus: Focus::Primary,
            primary_choices,
            primary,
            keys,
            zai: ZaiFields::from_config(cfg),
            status: String::new(),
        }
    }

    /// The focused key input, if a key row is focused.
    fn focused_key_mut(&mut self) -> Option<&mut KeyInput> {
        match self.focus {
            Focus::Key(i) => self.keys.get_mut(i),
            _ => None,
        }
    }
}

/// What the key handler asks the host app to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Stay open, keep listening for keys.
    Continue,
    /// Close the overlay (discard or save already happened).
    Close,
    /// Save just succeeded — caller should refresh affected vendors.
    SavedAndClose,
    /// Quit the host TUI. Ctrl-C remains global even while the overlay owns
    /// keyboard focus.
    Quit,
}

/// Permission note appended to the "saved" status line. The overlay `chmod
/// 600`s the file on Unix; Windows has no such step, so the note is empty there.
#[cfg(unix)]
const PERMS_NOTE: &str = " (chmod 600)";
#[cfg(not(unix))]
const PERMS_NOTE: &str = "";

fn saved_status() -> String {
    format!(
        "saved to {}{}",
        crate::config::config_path_hint(),
        PERMS_NOTE
    )
}

/// Key map. Returns the action to perform after the keypress.
pub fn handle_key(state: &mut SettingsState, code: KeyCode, mods: KeyModifiers) -> Action {
    if matches!(code, KeyCode::Esc) {
        return Action::Close;
    }
    if matches!(code, KeyCode::Char('c')) && mods.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }
    // Ctrl-S triggers save from any field.
    if matches!(code, KeyCode::Char('s')) && mods.contains(KeyModifiers::CONTROL) {
        return try_save(state);
    }
    if matches!(code, KeyCode::Char('v')) && mods.contains(KeyModifiers::CONTROL) {
        if let Some(input) = state.focused_key_mut() {
            input.toggle_reveal();
        }
        return Action::Continue;
    }
    match code {
        KeyCode::Tab | KeyCode::Down => {
            state.focus = state.focus.next();
            return Action::Continue;
        }
        KeyCode::BackTab | KeyCode::Up => {
            state.focus = state.focus.prev();
            return Action::Continue;
        }
        _ => {}
    }

    // A modifier chord is not text. The overlay swallows every key while open,
    // so every unhandled chord must be ignored rather than corrupting the
    // secret silently. SHIFT is deliberately not rejected — it is how
    // uppercase arrives. Ctrl-C was handled above because it is a global quit.
    if matches!(code, KeyCode::Char(_))
        && mods.intersects(
            KeyModifiers::CONTROL
                | KeyModifiers::ALT
                | KeyModifiers::SUPER
                | KeyModifiers::HYPER
                | KeyModifiers::META,
        )
    {
        return Action::Continue;
    }

    // Field-specific handling.
    match state.focus {
        Focus::Primary => handle_primary(state, code),
        Focus::Key(i) => {
            if let Some(input) = state.keys.get_mut(i) {
                handle_input(input, code);
            }
        }
        Focus::Save => {
            if matches!(code, KeyCode::Enter) {
                return try_save(state);
            }
        }
    }
    Action::Continue
}

fn try_save(state: &mut SettingsState) -> Action {
    match save_to_config_default(state) {
        Ok(()) => {
            state.status = saved_status();
            Action::SavedAndClose
        }
        Err(e) => {
            state.status = format!("save failed: {e}");
            Action::Continue
        }
    }
}

fn handle_primary(state: &mut SettingsState, code: KeyCode) {
    // Left/Right cycles the primary-vendor radio over enabled vendors only.
    let choices = &state.primary_choices;
    let Some(idx) = choices.iter().position(|v| *v == state.primary) else {
        return;
    };
    let step = match code {
        KeyCode::Left => -1,
        KeyCode::Right | KeyCode::Char(' ') => 1,
        _ => return,
    };
    state.primary = choices[((idx as i32 + step).rem_euclid(choices.len() as i32)) as usize];
}

fn handle_input(input: &mut KeyInput, code: KeyCode) {
    match code {
        KeyCode::Char(c) => input.insert_char(c),
        KeyCode::Backspace => input.backspace(),
        KeyCode::Delete => input.delete(),
        KeyCode::Left => input.move_left(),
        KeyCode::Right => input.move_right(),
        KeyCode::Home => input.move_home(),
        KeyCode::End => input.move_end(),
        _ => {}
    }
}

/// Save to the platform config path (creating it). On success, signal a running
/// Waybar (`SIGRTMIN+13`) so a `signal: 13` module refreshes immediately.
fn save_to_config_default(state: &SettingsState) -> Result<()> {
    let path = default_config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io_at(parent, e))?;
    }
    save_to_path(state, &path)?;
    crate::waybar::request_refresh();
    Ok(())
}

/// Same as `save_to_config_default` but with an explicit path — exposed for
/// tests. Writing a non-empty key also sets that vendor's `enabled = true`.
pub fn save_to_path(state: &SettingsState, path: &Path) -> Result<()> {
    let original = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(AppError::io_at(path, error)),
    };
    let mut doc: DocumentMut = if original.trim().is_empty() {
        DocumentMut::new()
    } else {
        original.parse().map_err(|e: toml_edit::TomlError| {
            AppError::Other(format!("config.toml not parseable: {e}"))
        })?
    };

    // Do not write a disabled primary as a side effect of saving an API key.
    // With no enabled vendors, leave any existing value alone so the legacy
    // resolver's Anthropic fallback remains intact.
    if state.primary_choices.contains(&state.primary) {
        set_string(&mut doc, "ui", "primary", state.primary.slug())?;
    }

    for (i, kv) in KEY_VENDORS.iter().enumerate() {
        let Some(input) = state.keys.get(i) else {
            continue;
        };
        update_key(&mut doc, kv.section, input)?;
    }
    update_zai_fields(&mut doc, &state.zai)?;

    let bytes = doc.to_string();
    crate::cache::atomic_write(path, bytes.as_bytes())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
    Ok(())
}

/// Apply one key field to the document. Untouched fields are left alone; a
/// field the user cleared is *removed*, so an inline secret can be deleted
/// from the overlay rather than lingering in the file. Writing a non-empty key
/// also opts the vendor in — the opt-in vendors would otherwise never fetch.
fn update_key(doc: &mut DocumentMut, section: &str, input: &KeyInput) -> Result<()> {
    if !input.dirty {
        return Ok(());
    }
    if input.buf.is_empty() {
        if let Some(table) = doc.get_mut(section).and_then(toml_edit::Item::as_table_mut) {
            table.remove("api_key");
        }
        return Ok(());
    }
    // Trimmed: pasted keys drag trailing whitespace/newlines along.
    set_string(doc, section, "api_key", input.buf.trim())?;
    set_bool(doc, section, "enabled", true)
}

/// Persist the Z.AI fields: a value equal to the default removes the key
/// instead of writing it, so the file stays minimal and the code defaults
/// remain the single source of truth.
fn update_zai_fields(doc: &mut DocumentMut, zai: &ZaiFields) -> Result<()> {
    fn write(doc: &mut DocumentMut, key: &str, value: &str, default: &str) -> Result<()> {
        if value == default {
            if let Some(table) = doc.get_mut("zai").and_then(toml_edit::Item::as_table_mut) {
                table.remove(key);
            }
            return Ok(());
        }
        set_string(doc, "zai", key, value)
    }
    write(doc, "account_type", &zai.account_type, "personal")?;
    write(doc, "site", &zai.site, "")?;
    write(doc, "organization_id", &zai.organization_id, "")?;
    write(doc, "project_id", &zai.project_id, "")
}

/// Set or update a string field in a TOML section, preserving comments and
/// formatting of unaffected nodes.
fn set_string(doc: &mut DocumentMut, section: &str, key: &str, new_value: &str) -> Result<()> {
    let table = doc
        .entry(section)
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or_else(|| AppError::Other(format!("config.toml: [{section}] is not a table")))?;

    if let Some(item) = table.get_mut(key)
        && let Some(v) = item.as_value_mut()
    {
        *v = toml_edit::Value::from(new_value);
        v.decor_mut().set_prefix(" ");
        return Ok(());
    }
    table.insert(key, value(new_value));
    Ok(())
}

/// Same as [`set_string`] for a boolean field.
fn set_bool(doc: &mut DocumentMut, section: &str, key: &str, new_value: bool) -> Result<()> {
    let table = doc
        .entry(section)
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or_else(|| AppError::Other(format!("config.toml: [{section}] is not a table")))?;

    if let Some(item) = table.get_mut(key)
        && let Some(v) = item.as_value_mut()
    {
        *v = toml_edit::Value::from(new_value);
        v.decor_mut().set_prefix(" ");
        return Ok(());
    }
    table.insert(key, value(new_value));
    Ok(())
}

fn default_config_path() -> Result<PathBuf> {
    // Save back to the same file Config::load() selected. On macOS this may be
    // the legacy ~/.config path when the canonical Application Support file is
    // absent; writing a new canonical file would shadow the existing config on
    // the next load and silently discard all settings the overlay did not copy.
    crate::config::resolved_path()
        .ok_or_else(|| AppError::Other("could not resolve config dir".into()))
}

// ─── Native frontend bridge ───────────────────────────────────────────────

/// Versioned, non-secret description consumed by native desktop frontends.
/// Inline key values are deliberately represented only as booleans: a
/// long-lived shell process never needs to receive credentials just to draw a
/// settings form.
#[derive(Debug, Serialize)]
struct SettingsSnapshot {
    schema_version: u8,
    primary: String,
    primary_choices: Vec<PrimaryChoice>,
    keys: Vec<KeyStatus>,
    /// Per-vendor account lists for vendors that support more than one
    /// subscription (Z.AI today). Additive on schema 1: older frontends see
    /// only `keys`. The zai vendor is absent from `keys` — its card lives
    /// here, one entry per configured account plus the default section.
    #[serde(default)]
    accounts: Vec<AccountStatus>,
}

/// One Z.AI account: the `[zai]` default section (`label: ""`) or one
/// `[[zai.accounts]]` entry. Field order is the form order — name, site,
/// account type, the two team ids, API key LAST.
#[derive(Debug, Serialize)]
struct AccountStatus {
    /// Machine vendor id ("zai", "kimi", "deepseek", …) — the QML groups and
    /// the patch's `accounts.<vendor>` key.
    vendor: &'static str,
    /// The vendor's display name ("Z.AI", "Kimi", …), Rust-owned so the
    /// frontend never keeps its own vendor-name table.
    vendor_display: String,
    /// Config label; "" marks the default `[zai]` section (no name field).
    label: String,
    /// Card title in the form.
    display: String,
    environment: String,
    configured: bool,
    inline_configured: bool,
    environment_configured: bool,
    fields: Vec<FieldStatus>,
}

/// One key-account vendor rendered as Kimi-style cards by the native
/// settings bridge: a default card plus one numbered card per
/// `[[vendor.accounts]]` entry, positional addressing, optional per-account
/// spend limit for the two Admin-API spend vendors.
struct KeyAccountVendor {
    id: VendorId,
    display: &'static str,
    section: &'static str,
    /// The section's default env var name (shown as the environment hint).
    env: &'static str,
    /// Whether cards carry an editable `monthly_limit` field.
    supports_limit: bool,
    /// Kimi's default may hold the CLI's own OAuth login — removing it is
    /// refused; every other vendor's default is just a key, so removal
    /// clears it.
    refuse_default_removal: bool,
}

/// Every vendor that manages keys as ACCOUNT CARDS, in panel display order.
/// `openrouter`'s legacy hand-written labels are gone — the shared
/// positional rule owns naming for all of these.
const KEY_ACCOUNT_VENDORS: &[KeyAccountVendor] = &[
    KeyAccountVendor { id: VendorId::Kimi, display: "Kimi", section: "kimi", env: "KIMI_API_KEY", supports_limit: false, refuse_default_removal: true },
    KeyAccountVendor { id: VendorId::AnthropicApi, display: "Anthropic API", section: "anthropic_api", env: "ANTHROPIC_ADMIN_KEY", supports_limit: true, refuse_default_removal: false },
    KeyAccountVendor { id: VendorId::OpenaiApi, display: "OpenAI API", section: "openai_api", env: "OPENAI_ADMIN_KEY", supports_limit: true, refuse_default_removal: false },
    KeyAccountVendor { id: VendorId::Openrouter, display: "OpenRouter", section: "openrouter", env: "OPENROUTER_API_KEY", supports_limit: false, refuse_default_removal: false },
    KeyAccountVendor { id: VendorId::Deepseek, display: "DeepSeek", section: "deepseek", env: "DEEPSEEK_API_KEY", supports_limit: false, refuse_default_removal: false },
    KeyAccountVendor { id: VendorId::Kilo, display: "Kilo", section: "kilo", env: "KILO_API_KEY", supports_limit: false, refuse_default_removal: false },
    KeyAccountVendor { id: VendorId::Novita, display: "Novita", section: "novita", env: "NOVITA_API_KEY", supports_limit: false, refuse_default_removal: false },
    KeyAccountVendor { id: VendorId::Moonshot, display: "Moonshot", section: "moonshot", env: "MOONSHOT_API_KEY", supports_limit: false, refuse_default_removal: false },
    KeyAccountVendor { id: VendorId::Grok, display: "Grok", section: "grok", env: "XAI_MANAGEMENT_KEY", supports_limit: false, refuse_default_removal: false },
    KeyAccountVendor { id: VendorId::Minimax, display: "MiniMax", section: "minimax", env: "MINIMAX_API_KEY", supports_limit: false, refuse_default_removal: false },
    KeyAccountVendor { id: VendorId::OpenCodeGo, display: "OpenCode Go", section: "opencode-go", env: "OPENCODE_GO_API_KEY", supports_limit: false, refuse_default_removal: false },
];

impl KeyAccountVendor {
    /// The section's default inline key, when set.
    fn default_inline_key<'a>(&self, cfg: &'a Config) -> Option<&'a str> {
        let key = match self.id {
            VendorId::Kimi => cfg.kimi.api_key.as_deref(),
            VendorId::Openrouter => cfg.openrouter.api_key.as_deref(),
            VendorId::Deepseek => cfg.deepseek.api_key.as_deref(),
            VendorId::Kilo => cfg.kilo.api_key.as_deref(),
            VendorId::Novita => cfg.novita.api_key.as_deref(),
            VendorId::Moonshot => cfg.moonshot.api_key.as_deref(),
            VendorId::Grok => cfg.grok.api_key.as_deref(),
            VendorId::Minimax => cfg.minimax.api_key.as_deref(),
            VendorId::OpenCodeGo => cfg.opencode_go.api_key.as_deref(),
            VendorId::AnthropicApi => cfg.anthropic_api.api_key.as_deref(),
            VendorId::OpenaiApi => cfg.openai_api.api_key.as_deref(),
            _ => return None,
        };
        key.filter(|k| !k.is_empty())
    }

    /// The section-level monthly limit (the DEFAULT card's limit).
    fn default_limit(&self, cfg: &Config) -> Option<f64> {
        match self.id {
            VendorId::AnthropicApi => cfg.anthropic_api.monthly_limit,
            VendorId::OpenaiApi => cfg.openai_api.monthly_limit,
            _ => None,
        }
    }
}

/// The account cards for one key-account vendor: the default section first
/// (shown only when it has a key or the env var set, or when no numbered
/// accounts exist — the Add button is the empty state), then one card per
/// numbered account with its own key/limit state.
fn key_account_cards(
    spec: &KeyAccountVendor,
    cfg: &Config,
    environment_configured: &impl Fn(&str) -> bool,
) -> Vec<AccountStatus> {
    let accounts = cfg.key_accounts(spec.id).unwrap_or_default();
    let env_ok = environment_configured(spec.env);
    let inline = spec.default_inline_key(cfg);
    let mut out = Vec::new();
    if inline.is_some() || env_ok || accounts.is_empty() {
        let mut fields = vec![secret_field()];
        if spec.supports_limit {
            fields.insert(0, limit_field(spec.default_limit(cfg)));
        }
        out.push(AccountStatus {
            vendor: spec.section,
            vendor_display: spec.display.to_string(),
            label: String::new(),
            display: "Default".into(),
            environment: spec.env.to_string(),
            configured: inline.is_some() || env_ok,
            inline_configured: inline.is_some(),
            environment_configured: env_ok,
            fields,
        });
    }
    for account in accounts {
        let account_env_ok = account
            .api_key_env
            .as_deref()
            .map(environment_configured)
            .unwrap_or(false);
        let account_inline = account.api_key.as_deref().is_some_and(|k| !k.is_empty());
        let mut fields = vec![secret_field()];
        if spec.supports_limit {
            fields.insert(0, limit_field(account.monthly_limit));
        }
        out.push(AccountStatus {
            vendor: spec.section,
            vendor_display: spec.display.to_string(),
            label: account.label.clone(),
            display: account.label.clone(),
            environment: account
                .api_key_env
                .clone()
                .unwrap_or_else(|| "(inline key)".into()),
            configured: account_inline || account_env_ok,
            inline_configured: account_inline,
            environment_configured: account_env_ok,
            fields,
        });
    }
    out
}

fn secret_field() -> FieldStatus {
    FieldStatus {
        id: "api_key".into(),
        label: "API key".into(),
        kind: "secret",
        value: String::new(),
        choices: Vec::new(),
        labels: Default::default(),
    }
}

fn limit_field(limit: Option<f64>) -> FieldStatus {
    FieldStatus {
        id: "monthly_limit".into(),
        label: "Monthly limit (USD)".into(),
        kind: "text",
        value: limit
            .filter(|l| l.is_finite() && *l > 0.0)
            .map(|l| format!("{l}"))
            .unwrap_or_default(),
        choices: Vec::new(),
        labels: Default::default(),
    }
}

#[derive(Debug, Serialize)]
struct PrimaryChoice {
    id: String,
    label: String,
}

#[derive(Debug, Serialize)]
struct KeyStatus {
    id: String,
    label: String,
    environment: String,
    note: String,
    configured: bool,
    inline_configured: bool,
    environment_configured: bool,
}

#[derive(Debug, Serialize)]
struct FieldStatus {
    id: String,
    label: String,
    /// `choice` renders a dropdown (see `choices`), `text` a line edit,
    /// `secret` a password row (value stays empty; presence rides the
    /// account's `configured` flags).
    kind: &'static str,
    /// Current value; `""` means unset for optional fields.
    value: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    choices: Vec<String>,
    /// Display labels for choice values (site codes → product names). The
    /// canonical names live in Rust so no frontend grows its own table.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    labels: std::collections::BTreeMap<String, String>,
}

/// Additive patch accepted on stdin by `ai-usagebar-omarchy settings apply`.
/// Missing keys remain byte-for-byte untouched. `clear` explicitly removes an
/// inline key, matching the TUI overlay's existing empty-dirty-field behavior.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyRequest {
    schema_version: u8,
    primary: Option<String>,
    #[serde(default)]
    keys: BTreeMap<String, KeyMutation>,
    /// Per-vendor account-list mutations (Z.AI today), composed with `keys`.
    #[serde(default)]
    accounts: BTreeMap<String, Vec<AccountMutation>>,
}

/// One account mutation. `label` addresses an account by its POSITION
/// ("" = the default section, "1".."n" = the accounts array top to bottom);
/// `Add` appends and needs no name — every account is auto-named by its
/// position at load.
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
enum AccountMutation {
    Update {
        label: String,
        #[serde(default)]
        fields: BTreeMap<String, String>,
        api_key: Option<AccountKeyMutation>,
    },
    Add {
        /// Accepted so an older client's patch still deserializes; ignored —
        /// the account's name is its position.
        #[serde(default)]
        #[allow(dead_code)]
        name: Option<String>,
        #[serde(default)]
        fields: BTreeMap<String, String>,
        api_key: Option<AccountKeyMutation>,
    },
    Remove { label: String },
}

/// Resolve a settings-account address into a 0-based index into the vendor's
/// accounts array. Accounts carry no stored names: the address IS the
/// position ("1".."n"), and `live` bounds it by what this request has added
/// or removed so far.
fn account_index(vendor: &str, label: &str, live: usize) -> Result<usize> {
    let position: usize = label.parse().map_err(|_| {
        AppError::Other(format!(
            "{vendor} accounts are addressed by position (\"1\"..\"{live}\"); {label:?} is not a position"
        ))
    })?;
    if position == 0 || position > live {
        return Err(AppError::Other(format!(
            "{vendor} account {label:?} does not exist (positions are 1..={live})"
        )));
    }
    Ok(position - 1)
}

/// Legacy cleanup: account names are positional now, so a `label` key left
/// in the accounts array by an older format is dead weight. Drop them all
/// whenever the bridge touches the vendor's accounts — one save cleans the
/// whole section.
fn strip_legacy_labels(doc: &mut DocumentMut, section: &str) {
    if let Some(accounts) = doc
        .get_mut(section)
        .and_then(|item| item.as_table_mut())
        .and_then(|table| table.get_mut("accounts"))
        .and_then(|item| item.as_array_of_tables_mut())
    {
        for entry in accounts.iter_mut() {
            entry.remove("label");
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
enum AccountKeyMutation {
    Set { value: String },
    Clear,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
enum KeyMutation {
    Set {
        value: String,
        /// Non-secret field updates riding along with the key change.
        #[serde(default)]
        fields: BTreeMap<String, String>,
    },
    Clear {
        #[serde(default)]
        fields: BTreeMap<String, String>,
    },
    /// Field-only change: no key touched.
    Fields {
        fields: BTreeMap<String, String>,
    },
}

const SETTINGS_SCHEMA_VERSION: u8 = 1;
const MAX_SETTINGS_REQUEST_BYTES: u64 = 64 * 1024;
const MAX_API_KEY_BYTES: usize = 16 * 1024;

fn configured_key_env<'a>(cfg: &'a Config, section: &str, fallback: &'a str) -> &'a str {
    match section {
        "anthropic_api" => &cfg.anthropic_api.api_key_env,
        "zai" => &cfg.zai.api_key_env,
        "openrouter" => &cfg.openrouter.api_key_env,
        "deepseek" => &cfg.deepseek.api_key_env,
        "kimi" => &cfg.kimi.api_key_env,
        "kilo" => &cfg.kilo.api_key_env,
        "novita" => &cfg.novita.api_key_env,
        "moonshot" => &cfg.moonshot.api_key_env,
        "grok" => &cfg.grok.api_key_env,
        "minimax" => &cfg.minimax.api_key_env,
        "opencode-go" => &cfg.opencode_go.api_key_env,
        _ => fallback,
    }
}

fn snapshot_from_config_with(
    cfg: &Config,
    environment_configured: impl Fn(&str) -> bool,
) -> SettingsSnapshot {
    // The panel sees the SAME positional names the runtime uses — derive
    // even for a hand-built Config so a direct snapshot can never leak a
    // stale file label.
    let mut cfg = cfg.clone();
    cfg.derive_account_labels();
    let cfg = &cfg;
    let state = SettingsState::from_config(cfg);
    let primary_choices = state
        .primary_choices
        .iter()
        .map(|id| PrimaryChoice {
            id: id.slug().to_string(),
            label: id.display_name().to_string(),
        })
        .collect();
    // Z.AI and every key-account vendor moved to the account-card list —
    // their old key cards are gone from `keys` (the apply path still accepts
    // legacy key mutations for the TUI overlay).
    let account_card_vendors: Vec<VendorId> = std::iter::once(VendorId::Zai)
        .chain(KEY_ACCOUNT_VENDORS.iter().map(|spec| spec.id))
        .collect();
    let keys = KEY_VENDORS
        .iter()
        .filter(|vendor| !account_card_vendors.contains(&vendor.id))
        .map(|vendor| {
            let environment = configured_key_env(cfg, vendor.section, vendor.env);
            let inline_configured =
                config_inline_key(cfg, vendor.section).is_some_and(|v| !v.is_empty());
            let environment_configured = environment_configured(environment);
            KeyStatus {
                id: vendor.id.slug().to_string(),
                label: vendor.label.to_string(),
                environment: environment.to_string(),
                note: vendor.note.to_string(),
                configured: inline_configured || environment_configured,
                inline_configured,
                environment_configured,
            }
        })
        .collect();
    SettingsSnapshot {
        schema_version: SETTINGS_SCHEMA_VERSION,
        primary: state.primary.slug().to_string(),
        primary_choices,
        keys,
        accounts: zai_account_status(cfg, &environment_configured)
            .into_iter()
            .chain(KEY_ACCOUNT_VENDORS.iter().flat_map(|spec| {
                key_account_cards(spec, cfg, &environment_configured)
            }))
            .collect(),
    }
}

/// One account card's editable fields, in the form's order: account type,
/// the two team ids (team only), API key last.
///
/// No name field: the bar tags accounts with the provider logo now, so
/// accounts are named where they are created — adds auto-name server-side
/// (`auto_account_label`), and a rename stays a config.toml edit.
fn zai_account_fields(label: &str, kind: ZaiAccountType, _site: &str, org: &str, proj: &str) -> Vec<FieldStatus> {
    let _ = label;
    let mut fields = Vec::new();
    // NOTE: no site field anymore — the site (z.ai / bigmodel.cn) is
    // auto-detected from the account type (team & usage live on bigmodel.cn,
    // personal on z.ai); a hand-edited `site` in config.toml still overrides.
    fields.push(FieldStatus {
        id: "account_type".into(),
        label: "Account type".into(),
        kind: "choice",
        value: match kind {
            ZaiAccountType::Personal => "personal",
            ZaiAccountType::Team => "team",
            ZaiAccountType::Usage => "usage",
        }
        .to_string(),
        choices: vec!["personal".into(), "team".into(), "usage".into()],
        labels: Default::default(),
    });
    if matches!(kind, ZaiAccountType::Team) {
        for (id, label, value) in [
            ("organization_id", "Organization ID", org),
            ("project_id", "Project ID", proj),
        ] {
            fields.push(FieldStatus {
                id: id.into(),
                label: label.into(),
                kind: "text",
                value: value.to_string(),
                choices: Vec::new(),
                labels: Default::default(),
            });
        }
    }
    fields.push(FieldStatus {
        id: "api_key".into(),
        label: "API key".into(),
        kind: "secret",
        value: String::new(),
        choices: Vec::new(),
        labels: Default::default(),
    });
    fields
}

/// The Z.AI account list: the default `[zai]` section first (label "",
/// displayed as "Default"), then every `[[zai.accounts]]` entry.
#[allow(clippy::too_many_arguments)]
fn zai_account_status(
    cfg: &Config,
    environment_configured: &impl Fn(&str) -> bool,
) -> Vec<AccountStatus> {
    let env = cfg.zai.api_key_env.clone();
    let env_configured = environment_configured(&env);
    let default_inline = cfg.zai.api_key.as_deref().is_some_and(|v| !v.is_empty());
    let default_configured = default_inline || env_configured;
    let mut out = Vec::new();
    if default_configured {
        out.push(AccountStatus {
        vendor: "zai",
        vendor_display: "Z.AI".into(),
        label: String::new(),
        display: "Default".into(),
        environment: env.clone(),
        configured: cfg.zai.api_key.as_deref().is_some_and(|v| !v.is_empty())
            || env_configured,
        inline_configured: cfg.zai.api_key.as_deref().is_some_and(|v| !v.is_empty()),
        environment_configured: env_configured,
        fields: zai_account_fields(
            "",
            cfg.zai.account_type,
            &match cfg.zai.site {
                Some(crate::config::ZaiSite::Global) => "global".to_string(),
                Some(crate::config::ZaiSite::Cn) => "cn".to_string(),
                None => String::new(),
            },
            cfg.zai.organization_id.as_deref().unwrap_or(""),
            cfg.zai.project_id.as_deref().unwrap_or(""),
        ),
        });
    }
    for account in &cfg.zai.accounts {
        let account_env_configured = account
            .api_key_env
            .as_deref()
            .map(environment_configured)
            .unwrap_or(false);
        out.push(AccountStatus {
            vendor: "zai",
            vendor_display: "Z.AI".into(),
            label: account.label.clone(),
            display: account.label.clone(),
            environment: account
                .api_key_env
                .clone()
                .unwrap_or_else(|| "(per-account env or inline)".into()),
            configured: account.api_key.as_deref().is_some_and(|v| !v.is_empty())
                || account_env_configured,
            inline_configured: account.api_key.as_deref().is_some_and(|v| !v.is_empty()),
            environment_configured: account_env_configured,
            fields: zai_account_fields(
                &account.label,
                account.account_type,
                &match account.site {
                    Some(crate::config::ZaiSite::Global) => "global".to_string(),
                    Some(crate::config::ZaiSite::Cn) => "cn".to_string(),
                    None => String::new(),
                },
                account.organization_id.as_deref().unwrap_or(""),
                account.project_id.as_deref().unwrap_or(""),
            ),
        });
    }
    out
}

fn settings_snapshot_json(cfg: &Config) -> Result<String> {
    Ok(serde_json::to_string(&snapshot_from_config_with(
        cfg,
        |environment| std::env::var_os(environment).is_some_and(|value| !value.is_empty()),
    ))?)
}

#[cfg(test)]
fn settings_snapshot_json_with(
    cfg: &Config,
    environment_configured: impl Fn(&str) -> bool,
) -> Result<String> {
    Ok(serde_json::to_string(&snapshot_from_config_with(
        cfg,
        environment_configured,
    ))?)
}

fn vendor_from_slug(slug: &str) -> Option<VendorId> {
    VendorId::all().iter().copied().find(|id| id.slug() == slug)
}

fn state_from_apply_request(cfg: &Config, raw: &str) -> Result<SettingsState> {
    let request: ApplyRequest = serde_json::from_str(raw)?;
    if request.schema_version != SETTINGS_SCHEMA_VERSION {
        return Err(AppError::Other(format!(
            "unsupported settings schema version {}",
            request.schema_version
        )));
    }

    let mut state = SettingsState::from_config(cfg);
    if let Some(primary) = request.primary {
        let id = vendor_from_slug(&primary)
            .ok_or_else(|| AppError::Other(format!("unknown primary vendor {primary:?}")))?;
        if !state.primary_choices.contains(&id) {
            return Err(AppError::Other(format!(
                "primary vendor {primary:?} is not enabled"
            )));
        }
        state.primary = id;
    }

    for (id, mutation) in request.keys {
        let index = KEY_VENDORS
            .iter()
            .position(|vendor| vendor.id.slug() == id)
            .ok_or_else(|| AppError::Other(format!("unknown API-key vendor {id:?}")))?;
        let input = &mut state.keys[index];
        let fields = match mutation {
            KeyMutation::Set { value, fields } => {
                // Trimmed before any check: a whitespace-only paste must hit
                // the empty-key error, not slip through as a "cleared" key.
                let value = value.trim();
                if value.is_empty() {
                    return Err(AppError::Other(format!(
                        "API key for {id:?} is empty; use the clear action to remove it"
                    )));
                }
                if value.len() > MAX_API_KEY_BYTES {
                    return Err(AppError::Other(format!(
                        "API key for {id:?} exceeds {MAX_API_KEY_BYTES} bytes"
                    )));
                }
                if value.chars().any(char::is_control) {
                    return Err(AppError::Other(format!(
                        "API key for {id:?} contains control characters"
                    )));
                }
                input.buf = value.to_string();
                fields
            }
            KeyMutation::Clear { fields } => {
                input.buf.clear();
                fields
            }
            KeyMutation::Fields { fields } => fields,
        };
        input.cursor = input.buf.chars().count();
        input.dirty = true;
        input.revealed = false;

        if !fields.is_empty() {
            if id != "zai" {
                return Err(AppError::Other(format!(
                    "vendor {id:?} has no editable fields"
                )));
            }
            apply_zai_fields(&mut state.zai, &fields)?;
        }
    }
    Ok(state)
}

/// Validate and apply a field patch onto the Z.AI fields. `team` without both
/// ids is allowed to save — the fetch layer refuses it loudly with the
/// remedy, which is exactly what the widget then shows — but everything else
/// (unknown field, bad enum, oversized or control-carrying text) is rejected
/// here so a broken form can never write a broken config.
fn apply_zai_fields(zai: &mut ZaiFields, fields: &BTreeMap<String, String>) -> Result<()> {
    fn clean<'v>(field: &str, v: &'v str) -> Result<&'v str> {
        // Trimmed before validation — see the twin helper above.
        let trimmed = v.trim();
        if trimmed.chars().any(char::is_control) {
            return Err(AppError::Other(format!(
                "zai field {field:?} contains control characters"
            )));
        }
        if trimmed.chars().count() > 200 {
            return Err(AppError::Other(format!(
                "zai field {field:?} exceeds 200 characters"
            )));
        }
        Ok(trimmed)
    }
    for (field, value) in fields {
        match field.as_str() {
            "account_type" => {
                let value = clean(field, value)?;
                if !["personal", "team", "usage"].contains(&value) {
                    return Err(AppError::Other(format!(
                        "zai account_type must be personal, team, or usage (got {value:?})"
                    )));
                }
                zai.account_type = value.to_string();
            }
            "site" => {
                let value = clean(field, value)?;
                if !["", "global", "cn"].contains(&value) {
                    return Err(AppError::Other(format!(
                        "zai site must be global or cn (got {value:?})"
                    )));
                }
                zai.site = value.to_string();
            }
            "organization_id" => zai.organization_id = clean(field, value)?.to_string(),
            "project_id" => zai.project_id = clean(field, value)?.to_string(),
            other => {
                return Err(AppError::Other(format!("unknown zai field {other:?}")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
fn apply_settings_json_to_path(cfg: &Config, raw: &str, path: &Path) -> Result<()> {
    let state = state_from_apply_request(cfg, raw)?;
    save_to_path(&state, path)?;
    let request: ApplyRequest = serde_json::from_str(raw)?;
    apply_account_mutations_to_path(cfg, &request, path)
}

/// Parsed, validated account field patch.
struct AccountFields {
    site: Option<String>,
    account_type: Option<String>,
    organization_id: Option<String>,
    project_id: Option<String>,
}

impl AccountFields {
    fn parse(fields: &BTreeMap<String, String>) -> Result<Self> {
        fn clean<'v>(field: &str, v: &'v str) -> Result<&'v str> {
            // Trimmed before validation: copy-paste drags trailing tabs and
            // newlines onto ids, and those are whitespace, not
            // control-character attacks. Interior control chars still reject.
            let trimmed = v.trim();
            if trimmed.chars().any(char::is_control) {
                return Err(AppError::Other(format!(
                    "zai field {field:?} contains control characters"
                )));
            }
            if trimmed.chars().count() > 200 {
                return Err(AppError::Other(format!(
                    "zai field {field:?} exceeds 200 characters"
                )));
            }
            Ok(trimmed)
        }
        let mut out = AccountFields {
            site: None,
            account_type: None,
            organization_id: None,
            project_id: None,
        };
        for (field, value) in fields {
            match field.as_str() {
                // Accounts are named by position now; an older client still
                // sending a name gets a clear refusal rather than a silent
                // no-op.
                "name" => {
                    return Err(AppError::Other(
                        "accounts are auto-named by their position (1, 2, …); the name field is gone".into(),
                    ));
                }
                "site" => {
                    let value = clean(field, value)?;
                    if !["", "global", "cn"].contains(&value) {
                        return Err(AppError::Other(format!(
                            "zai site must be global or cn (got {value:?})"
                        )));
                    }
                    out.site = Some(value.to_string());
                }
                "account_type" => {
                    let value = clean(field, value)?;
                    if !["personal", "team", "usage"].contains(&value) {
                        return Err(AppError::Other(format!(
                            "zai account_type must be personal, team, or usage (got {value:?})"
                        )));
                    }
                    out.account_type = Some(value.to_string());
                }
                "organization_id" => out.organization_id = Some(clean(field, value)?.to_string()),
                "project_id" => out.project_id = Some(clean(field, value)?.to_string()),
                other => {
                    return Err(AppError::Other(format!("unknown zai field {other:?}")));
                }
            }
        }
        Ok(out)
    }
}

fn validate_account_key(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(AppError::Other(
            "account API key is empty; use the clear action to remove it".into(),
        ));
    }
    if value.len() > MAX_API_KEY_BYTES {
        return Err(AppError::Other(format!(
            "account API key exceeds {MAX_API_KEY_BYTES} bytes"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(AppError::Other(
            "account API key contains control characters".into(),
        ));
    }
    Ok(())
}

/// Apply the `accounts` section of a settings patch to the config file.
/// Runs after the key/primary save, so both passes compose on one file.
/// Writing rules mirror the config's own conventions: values equal to the
/// default are removed rather than spelled out, and non-team accounts never
/// keep organization ids around.
fn apply_account_mutations_to_path(cfg: &Config, request: &ApplyRequest, path: &Path) -> Result<()> {
    for vendor in request.accounts.keys() {
        if vendor != "zai"
            && !KEY_ACCOUNT_VENDORS
                .iter()
                .any(|spec| spec.section == vendor.as_str())
        {
            return Err(AppError::Other(format!(
                "vendor {vendor:?} has no account list"
            )));
        }
    }
    if request.accounts.is_empty() {
        return Ok(());
    }

    let original = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(AppError::io_at(path, error)),
    };
    let mut doc: DocumentMut = if original.trim().is_empty() {
        DocumentMut::new()
    } else {
        original.parse().map_err(|e: toml_edit::TomlError| {
            AppError::Other(format!("config.toml not parseable: {e}"))
        })?
    };

    for (vendor, mutations) in &request.accounts {
        if vendor == "zai" {
            apply_zai_mutations(&mut doc, cfg, mutations)?;
        } else if let Some(spec) = KEY_ACCOUNT_VENDORS
            .iter()
            .find(|spec| spec.section == vendor.as_str())
        {
            apply_key_account_mutations(&mut doc, cfg, spec, mutations)?;
        } else {
            return Err(AppError::Other(format!(
                "vendor {vendor:?} has no account list"
            )));
        }
    }

    let bytes = doc.to_string();
    crate::cache::atomic_write(path, bytes.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
    Ok(())
}

fn apply_zai_mutations(doc: &mut DocumentMut, cfg: &Config, mutations: &[AccountMutation]) -> Result<()> {
    // Accounts are addressed by position; `live_count` follows every add and
    // remove in this request, in order, so a sequence like remove-2 + add
    // composes exactly as the panel previewed it.
    strip_legacy_labels(doc, "zai");
    let mut live_count = cfg.zai.accounts.len();

    for mutation in mutations {
        match mutation {
            AccountMutation::Update {
                label,
                fields,
                api_key,
            } => {
                let parsed = AccountFields::parse(fields)?;
                if label.is_empty() {
                    let table = doc
                        .entry("zai")
                        .or_insert_with(toml_edit::table)
                        .as_table_mut()
                        .ok_or_else(|| {
                            AppError::Other("config.toml: [zai] is not a table".into())
                        })?;
                    write_account_fields(table, &parsed)?;
                    apply_account_key_to_table(table, api_key.as_ref())?;
                } else {
                    let index = account_index("zai", label, live_count)?;
                    let accounts = accounts_array_mut(doc, "zai")?;
                    let entry = accounts
                        .get_mut(index)
                        .ok_or_else(|| AppError::Other(format!("unknown zai account {label:?}")))?;
                    update_account_entry(entry, &parsed, api_key.as_ref())?;
                }
            }
            AccountMutation::Add {
                fields,
                api_key,
                ..
            } => {
                let parsed = AccountFields::parse(fields)?;
                // No label written: the entry's name is its position, derived
                // on every load. Appending is all "naming" there is.
                let mut table = toml_edit::Table::new();
                write_account_fields(&mut table, &parsed)?;
                let sets_key = matches!(api_key, Some(AccountKeyMutation::Set { .. }));
                if let Some(key) = api_key.as_ref() {
                    apply_account_key_to_table(&mut table, Some(key))?;
                }
                accounts_array_mut(doc, "zai")?.push(table);
                live_count += 1;
                // A key arriving through the form opts the vendor in,
                // matching the plain key-card behaviour.
                if sets_key {
                    set_bool(doc, "zai", "enabled", true)?;
                }
            }
            AccountMutation::Remove { label } => {
                if label.is_empty() {
                    // Removing the default resets the section to a bare
                    // table: key and every billing field go, so the card
                    // (shown only when a key exists) disappears with it.
                    // `entry` (not `get_mut`): the section may not exist in
                    // this file yet, and clearing a fresh table is a no-op.
                    let table = doc
                        .entry("zai")
                        .or_insert_with(toml_edit::table)
                        .as_table_mut()
                        .ok_or_else(|| {
                            AppError::Other("config.toml: [zai] is not a table".into())
                        })?;
                    for key in ["api_key", "account_type", "site", "organization_id", "project_id"] {
                        table.remove(key);
                    }
                } else {
                    let index = account_index("zai", label, live_count)?;
                    remove_account_at(doc, "zai", index)?;
                    live_count -= 1;
                }
            }
        }
    }
    Ok(())
}

/// Key-account vendors: every account is its own key (and, for the two
/// spend vendors, its own monthly limit). Names are positional; the address
/// IS the position. Kimi's default may hold the CLI's own OAuth login —
/// its removal is refused; every other vendor's default is just a key.
fn apply_key_account_mutations(
    doc: &mut DocumentMut,
    cfg: &Config,
    spec: &KeyAccountVendor,
    mutations: &[AccountMutation],
) -> Result<()> {
    strip_legacy_labels(doc, spec.section);
    let mut live_count = cfg.key_accounts(spec.id).map_or(0, |a| a.len());
    for mutation in mutations {
        match mutation {
            AccountMutation::Update {
                label,
                fields,
                api_key,
            } => {
                let limit = parse_limit_field(spec, fields)?;
                if label.is_empty() {
                    let table = doc
                        .entry(spec.section)
                        .or_insert_with(toml_edit::table)
                        .as_table_mut()
                        .ok_or_else(|| {
                            AppError::Other(format!(
                                "config.toml: [{}] is not a table",
                                spec.section
                            ))
                        })?;
                    apply_account_key_to_table(table, api_key.as_ref())?;
                    apply_limit_to_table(table, limit)?;
                } else {
                    let index = account_index(spec.display, label, live_count)?;
                    let accounts = accounts_array_mut(doc, spec.section)?;
                    let entry = accounts.get_mut(index).ok_or_else(|| {
                        AppError::Other(format!("unknown {} account {label:?}", spec.display))
                    })?;
                    apply_account_key_to_table(entry, api_key.as_ref())?;
                    apply_limit_to_table(entry, limit)?;
                }
            }
            AccountMutation::Add {
                fields,
                api_key,
                ..
            } => {
                let limit = parse_limit_field(spec, fields)?;
                // No label written: the entry's name is its position,
                // derived on every load. Appending is all "naming" there is.
                let mut table = toml_edit::Table::new();
                let sets_key = matches!(api_key, Some(AccountKeyMutation::Set { .. }));
                if let Some(key) = api_key.as_ref() {
                    apply_account_key_to_table(&mut table, Some(key))?;
                }
                apply_limit_to_table(&mut table, limit)?;
                accounts_array_mut(doc, spec.section)?.push(table);
                live_count += 1;
                // A key arriving through the form opts the vendor in,
                // matching the plain key-card behaviour.
                if sets_key {
                    set_bool(doc, spec.section, "enabled", true)?;
                }
            }
            AccountMutation::Remove { label } => {
                if label.is_empty() {
                    if spec.refuse_default_removal {
                        return Err(AppError::Other(
                            "the default kimi account cannot be removed; clear its key instead"
                                .into(),
                        ));
                    }
                    // Removing the default clears its key (and limit): the
                    // card was shown for that key, so it goes with it.
                    let table = doc
                        .entry(spec.section)
                        .or_insert_with(toml_edit::table)
                        .as_table_mut()
                        .ok_or_else(|| {
                            AppError::Other(format!(
                                "config.toml: [{}] is not a table",
                                spec.section
                            ))
                        })?;
                    table.remove("api_key");
                    if spec.supports_limit {
                        table.remove("monthly_limit");
                    }
                } else {
                    let index = account_index(spec.display, label, live_count)?;
                    remove_account_at(doc, spec.section, index)?;
                    live_count -= 1;
                }
            }
        }
    }
    Ok(())
}

/// The `monthly_limit` text field: `None` when the patch does not mention
/// it; `Some(None)` (empty string) removes it; `Some(value)` sets it.
/// Every other field is rejected — these accounts are a key and, for the
/// spend vendors, a limit; nothing else is per-account.
fn parse_limit_field(
    spec: &KeyAccountVendor,
    fields: &BTreeMap<String, String>,
) -> Result<Option<Option<f64>>> {
    for field in fields.keys() {
        if field != "monthly_limit" {
            return Err(AppError::Other(format!(
                "{} accounts have no field {field:?} — an account is its key (and, for the \
                 spend vendors, its monthly limit); everything else is vendor-level",
                spec.display
            )));
        }
    }
    let Some(raw) = fields.get("monthly_limit") else {
        return Ok(None);
    };
    if !spec.supports_limit {
        return Err(AppError::Other(format!(
            "{} accounts have no monthly_limit — it exists only for the spend-monitoring \
             vendors (Anthropic API, OpenAI API)",
            spec.display
        )));
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Some(None));
    }
    let value: f64 = trimmed.parse().map_err(|_| {
        AppError::Other(format!(
            "{} monthly_limit {trimmed:?} is not a number",
            spec.display
        ))
    })?;
    if !value.is_finite() || value <= 0.0 {
        return Err(AppError::Other(format!(
            "{} monthly_limit must be finite and greater than zero; clear the field to \
             show spend without a limit",
            spec.display
        )));
    }
    Ok(Some(Some(value)))
}

fn apply_limit_to_table(table: &mut toml_edit::Table, limit: Option<Option<f64>>) -> Result<()> {
    if let Some(limit) = limit {
        match limit {
            Some(value) => {
                table.insert("monthly_limit", toml_edit::value(value));
            }
            None => {
                table.remove("monthly_limit");
            }
        }
    }
    Ok(())
}

/// The `accounts` array-of-tables under a vendor's section, created on
/// first use.
fn accounts_array_mut<'a>(
    doc: &'a mut DocumentMut,
    section: &str,
) -> Result<&'a mut toml_edit::ArrayOfTables> {
    if !doc.contains_key(section) {
        doc.insert(section, toml_edit::Item::Table(toml_edit::Table::new()));
    }
    let table = doc
        .get_mut(section)
        .and_then(toml_edit::Item::as_table_mut)
        .ok_or_else(|| AppError::Other(format!("config.toml: [{section}] is not a table")))?;
    if !table.contains_key("accounts") {
        table.insert(
            "accounts",
            toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()),
        );
    }
    table
        .get_mut("accounts")
        .and_then(toml_edit::Item::as_array_of_tables_mut)
        .ok_or_else(|| {
            AppError::Other(format!(
                "config.toml: {section}.accounts is not an array of tables"
            ))
        })
}

/// Write validated fields into a `[zai]`-ish table. Defaults are removed so
/// the code defaults stay the single source of truth; non-team accounts
/// never keep organization ids.
fn write_account_fields(table: &mut toml_edit::Table, parsed: &AccountFields) -> Result<()> {
    if let Some(site) = &parsed.site {
        if site.is_empty() {
            table.remove("site");
        } else {
            set_string_in(table, "site", site)?;
        }
    }
    if let Some(kind) = &parsed.account_type {
        if kind == "personal" {
            table.remove("account_type");
        } else {
            set_string_in(table, "account_type", kind)?;
        }
    }
    let keep_ids = matches!(
        parsed.account_type.as_deref(),
        Some("team") | None
    ) && !matches!(parsed.account_type.as_deref(), Some("personal") | Some("usage"));
    for (key, value) in [
        ("organization_id", &parsed.organization_id),
        ("project_id", &parsed.project_id),
    ] {
        if let Some(value) = value {
            if value.is_empty() || !keep_ids {
                table.remove(key);
            } else {
                set_string_in(table, key, value)?;
            }
        } else if !keep_ids {
            // Type switched away from team: drop stale ids even when the
            // patch did not mention them.
            table.remove(key);
        }
    }
    Ok(())
}

fn apply_account_key_to_table(
    table: &mut toml_edit::Table,
    mutation: Option<&AccountKeyMutation>,
) -> Result<()> {
    match mutation {
        None => {}
        Some(AccountKeyMutation::Set { value }) => {
            let value = value.trim();
            validate_account_key(value)?;
            set_string_in(table, "api_key", value)?;
        }
        Some(AccountKeyMutation::Clear) => {
            table.remove("api_key");
        }
    }
    Ok(())
}

/// Write fields + key into one existing `[[zai.accounts]]` entry (already
/// resolved by position). No label handling — the position IS the identity.
fn update_account_entry(
    entry: &mut toml_edit::Table,
    parsed: &AccountFields,
    api_key: Option<&AccountKeyMutation>,
) -> Result<()> {
    // Type (possibly unchanged) decides whether the ids may exist at all.
    // The file's own value is the source of truth — cfg and doc were parsed
    // from the same bytes, and entries added earlier in THIS request exist
    // only in the doc.
    let is_team = entry
        .get("account_type")
        .and_then(|v| v.as_str())
        .is_some_and(|kind| kind == "team");
    let effective_team = parsed
        .account_type
        .as_deref()
        .map(|kind| kind == "team")
        .unwrap_or(is_team);
    let mut effective = AccountFields {
        site: parsed.site.clone(),
        account_type: parsed.account_type.clone(),
        organization_id: parsed.organization_id.clone(),
        project_id: parsed.project_id.clone(),
    };
    if !effective_team {
        effective.organization_id = None;
        effective.project_id = None;
    }
    write_account_fields(entry, &effective)?;
    apply_account_key_to_table(entry, api_key)?;
    Ok(())
}

/// Drop the n-th (0-based) entry of a vendor's accounts array.
fn remove_account_at(doc: &mut DocumentMut, section: &str, index: usize) -> Result<()> {
    let accounts = accounts_array_mut(doc, section)?;
    if index >= accounts.len() {
        return Err(AppError::Other(format!(
            "config.toml: {section}.accounts has no entry at position {}",
            index + 1
        )));
    }
    accounts.remove(index);
    Ok(())
}

/// `set_string` for an already-resolved table (the accounts code holds the
/// table directly).
fn set_string_in(table: &mut toml_edit::Table, key: &str, new_value: &str) -> Result<()> {
    if let Some(item) = table.get_mut(key)
        && let Some(v) = item.as_value_mut()
    {
        *v = toml_edit::Value::from(new_value);
        v.decor_mut().set_prefix(" ");
        return Ok(());
    }
    table.insert(key, toml_edit::value(new_value));
    Ok(())
}

fn read_settings_request<R: BufRead>(reader: R) -> Result<String> {
    let mut limited = reader.take(MAX_SETTINGS_REQUEST_BYTES + 1);
    let mut bytes = Vec::new();
    limited.read_until(b'\n', &mut bytes)?;
    if bytes.len() as u64 > MAX_SETTINGS_REQUEST_BYTES {
        return Err(AppError::Other(format!(
            "settings request exceeds {MAX_SETTINGS_REQUEST_BYTES} bytes"
        )));
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    String::from_utf8(bytes)
        .map_err(|_| AppError::Other("settings request is not valid UTF-8".into()))
}

fn apply_settings_from_stdin() -> Result<()> {
    let raw = read_settings_request(std::io::stdin().lock())?;
    let cfg = Config::load()?;
    let logged = crate::diag::redact(&serde_json::from_str::<serde_json::Value>(&raw).unwrap_or_default());
    let result = (|| -> Result<()> {
        let state = state_from_apply_request(&cfg, &raw)?;
        save_to_config_default(&state)?;
        // Account-list mutations compose after the key/primary pass.
        let request: ApplyRequest = serde_json::from_str(&raw)?;
        let path = default_config_path()?;
        apply_account_mutations_to_path(&cfg, &request, &path)?;
        Ok(())
    })();
    match &result {
        Ok(()) => crate::diag::event(
            "settings/apply",
            &format!("ok request={logged}"),
        ),
        Err(e) => crate::diag::event(
            "settings/apply",
            &format!("error: {} request={logged}", e.user_message()),
        ),
    }
    result
}

/// Administrative settings bridge for native frontends. `show` never emits a
/// secret; `apply` accepts its patch only over stdin so keys do not appear in
/// argv or the process environment.
pub fn run_cli(action: &crate::widget::cli::SettingsAction) -> i32 {
    let result = match action {
        crate::widget::cli::SettingsAction::Show => Config::load()
            .and_then(|cfg| settings_snapshot_json(&cfg))
            .map(|json| println!("{json}")),
        crate::widget::cli::SettingsAction::Apply => {
            apply_settings_from_stdin().map(|()| println!(r#"{{"ok":true}}"#))
        }
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("settings: {error}");
            1
        }
    }
}

// ─── Render ────────────────────────────────────────────────────────────────

/// Render the modal overlay over `area`.
pub fn render(f: &mut Frame, area: Rect, state: &SettingsState, theme: &Theme) {
    let modal = centered_rect(74, 88, area);
    f.render_widget(Clear, modal);

    let bubble = bubble_theme(theme);
    let block = bubble.titled_modal_block(" Settings ");
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    // Body (everything but the pinned hint) + a 1-line hint footer.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    // — Primary vendor + API keys header —
    let mut lines: Vec<Line> = vec![
        section_header("Primary vendor", "shown first on the bar / TUI", &bubble),
        primary_line(state, &bubble),
        Line::from(""),
        section_header(
            "API keys",
            "pick a row, type the key, then Ctrl-S — Claude & Codex use CLI login",
            &bubble,
        ),
    ];
    for (i, kv) in KEY_VENDORS.iter().enumerate() {
        let focused = state.focus == Focus::Key(i);
        lines.push(key_row(kv, &state.keys[i], focused, &bubble));
    }
    lines.push(Line::from(""));

    // — Save + status —
    lines.push(save_line(state.focus == Focus::Save, &bubble));
    if !state.status.is_empty() {
        let ok = state.status.starts_with("saved");
        let mark = if ok { "  ✓ " } else { "  ✗ " };
        let style = if ok { bubble.accent } else { bubble.selected };
        lines.push(Line::from(vec![
            Span::styled(mark, style.add_modifier(Modifier::BOLD)),
            Span::styled(state.status.clone(), bubble.muted),
        ]));
    }

    f.render_widget(Paragraph::new(lines), chunks[0]);

    // Context-aware hint footer.
    let hint = match state.focus {
        Focus::Primary => bubble.help_line([
            ("↑↓/tab", "move"),
            ("←→", "change vendor"),
            ("^S", "save"),
            ("esc", "close"),
        ]),
        Focus::Key(_) => bubble.help_line([
            ("↑↓/tab", "move"),
            ("type", "edit key"),
            ("^V", "reveal"),
            ("^S", "save"),
            ("esc", "close"),
        ]),
        Focus::Save => {
            bubble.help_line([("↑↓/tab", "move"), ("enter/^S", "save"), ("esc", "close")])
        }
    };
    f.render_widget(Paragraph::new(hint), chunks[1]);
}

fn section_header(title: &str, sub: &str, theme: &BubbleTheme) -> Line<'static> {
    Line::from(vec![
        theme.span(" "),
        Span::styled(title.to_string(), theme.title.add_modifier(Modifier::BOLD)),
        theme.muted(format!("   — {sub}")),
    ])
}

fn primary_line(state: &SettingsState, theme: &BubbleTheme) -> Line<'static> {
    let focused = state.focus == Focus::Primary;
    let name = state.primary.display_name().to_string();
    if focused {
        Line::from(vec![
            theme.span("   "),
            Span::styled("▸ ", theme.accent.add_modifier(Modifier::BOLD)),
            Span::styled("◀ ", theme.accent),
            Span::styled(
                format!(" {name} "),
                theme
                    .selected
                    .add_modifier(Modifier::REVERSED | Modifier::BOLD),
            ),
            Span::styled(" ▶", theme.accent),
            theme.muted("    ← → to change"),
        ])
    } else {
        Line::from(vec![theme.span("     "), Span::styled(name, theme.text)])
    }
}

fn key_row(kv: &KeyVendor, input: &KeyInput, focused: bool, theme: &BubbleTheme) -> Line<'static> {
    let label = format!("{:<11}", kv.label);
    let value = value_text(input, focused);

    // Env / status suffix: env-var name, whether an env override is set, note.
    let env_set = std::env::var(kv.env)
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let mut suffix = format!("   {}", kv.env);
    if env_set {
        suffix.push_str(" · env set (overrides)");
    }
    if !kv.note.is_empty() {
        suffix.push_str(&format!(" · {}", kv.note));
    }

    if focused {
        let val_style = if input.buf.is_empty() {
            theme.accent.add_modifier(Modifier::BOLD)
        } else {
            theme.selected.add_modifier(Modifier::REVERSED)
        };
        let mut spans = vec![
            theme.span("  "),
            Span::styled("▸ ", theme.accent.add_modifier(Modifier::BOLD)),
            Span::styled(label, theme.title.add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {value} "), val_style),
        ];
        if input.revealed {
            spans.push(theme.muted("  [revealed]"));
        }
        spans.push(theme.muted(suffix));
        Line::from(spans)
    } else {
        let val_style = if input.buf.is_empty() {
            theme.muted
        } else {
            theme.text
        };
        Line::from(vec![
            theme.span("    "),
            Span::styled(label, theme.text),
            Span::styled(format!(" {value}"), val_style),
            theme.muted(suffix),
        ])
    }
}

/// The value column: `(empty)` / a cursor when focused-empty / masked or
/// revealed buffer with a cursor mark inserted when focused.
fn value_text(input: &KeyInput, focused: bool) -> String {
    if input.buf.is_empty() {
        return if focused {
            "‸".to_string()
        } else {
            "(empty)".to_string()
        };
    }
    let base = input.display();
    if !focused {
        return base;
    }
    let mut chars: Vec<char> = base.chars().collect();
    let pos = input.cursor.min(chars.len());
    chars.insert(pos, '‸');
    chars.into_iter().collect()
}

fn save_line(focused: bool, theme: &BubbleTheme) -> Line<'static> {
    let style = if focused {
        theme
            .selected
            .add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        theme.accent.add_modifier(Modifier::BOLD)
    };
    let marker = if focused { "▸ " } else { "  " };
    Line::from(vec![
        theme.span("   "),
        Span::styled(marker, theme.accent.add_modifier(Modifier::BOLD)),
        Span::styled("  Save  (Ctrl-S)  ", style),
    ])
}

/// Center a rectangle of `percent_x * percent_y` over `r`.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_h = (r.height * percent_y) / 100;
    let popup_w = (r.width * percent_x) / 100;
    Rect {
        x: r.x + (r.width - popup_w) / 2,
        y: r.y + (r.height - popup_h) / 2,
        width: popup_w,
        height: popup_h,
    }
}

// crossterm types live behind ratatui; re-exported here for handle_key callers.
pub use ratatui::crossterm::event::{KeyCode, KeyModifiers};

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_config(initial: Option<&str>) -> (TempDir, std::path::PathBuf) {
        crate::cache::closed_temp_file("config.toml", initial)
    }

    /// Kimi accounts: name + key only (region auto-detected), default card
    /// only when a credential exists.
    #[test]
    fn kimi_accounts_round_trip_through_the_native_bridge() {
        let mut cfg = Config::default();
        cfg.kimi.enabled = true;
        cfg.kimi.api_key = Some("default-key".into());
        let raw = settings_snapshot_json_with(&cfg, |_| false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let kimi: Vec<&serde_json::Value> = parsed["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["vendor"] == "kimi")
            .collect();
        assert_eq!(kimi.len(), 1, "default card present (key configured)");
        assert_eq!(kimi[0]["label"], "");
        let ids: Vec<&str> = kimi[0]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["api_key"],
            "no name and no region field — accounts auto-name on add, region is auto-detected");

        // An unconfigured kimi shows no default card at all.
        let bare = settings_snapshot_json_with(&Config::default(), |_| false)
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&bare).unwrap();
        // The empty state: kimi's ONLY card is the default one (its Add
        // button), with no numbered cards until a key is added.
        let kimi: Vec<&serde_json::Value> = parsed["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["vendor"] == "kimi")
            .collect();
        assert_eq!(kimi.len(), 1, "empty state is exactly the Add entry");
        assert_eq!(kimi[0]["label"], "");
        assert_eq!(kimi[0]["vendor_display"], "Kimi");

        // Add + key-only update + remove via positional mutations.
        let (_td, path) = temp_config(None);
        let patch = r#"{"schema_version":1,"accounts":{"kimi":[
            {"action":"add","api_key":{"action":"set","value":"k"}}]}}"#;
        apply_settings_json_to_path(&Config::default(), patch, &path).unwrap();
        let reloaded = Config::load_from(&path).unwrap();
        assert!(reloaded.kimi.enabled, "key through the form opts the vendor in");
        assert!(reloaded.kimi.accounts.iter().any(|a| a.label == "1"),
            "the first account is named by its position");

        // Any field (name included) is rejected — position is the identity,
        // region is auto-detected.
        let patch = r#"{"schema_version":1,"accounts":{"kimi":[
            {"action":"update","label":"1","fields":{"name":"team"}}]}}"#;
        assert!(apply_settings_json_to_path(&reloaded, patch, &path).is_err());

        let patch = r#"{"schema_version":1,"accounts":{"kimi":[
            {"action":"update","label":"1","api_key":{"action":"clear"}}]}}"#;
        apply_settings_json_to_path(&reloaded, patch, &path).unwrap();
        let reloaded = Config::load_from(&path).unwrap();
        assert_eq!(reloaded.kimi.accounts[0].api_key, None);

        let patch = r#"{"schema_version":1,"accounts":{"kimi":[
            {"action":"remove","label":"1"}]}}"#;
        apply_settings_json_to_path(&reloaded, patch, &path).unwrap();
        let reloaded = Config::load_from(&path).unwrap();
        assert!(reloaded.kimi.accounts.is_empty());

        // Out-of-range positions are refused, not wrapped.
        let patch = r#"{"schema_version":1,"accounts":{"kimi":[
            {"action":"remove","label":"2"}]}}"#;
        assert!(apply_settings_json_to_path(&reloaded, patch, &path).is_err());

        // Fields beyond the key are rejected — region is not user-editable.
        let bad = r#"{"schema_version":1,"accounts":{"kimi":[
            {"action":"update","label":"","fields":{"region":"cn"}}]}}"#;
        let (_td2, path2) = temp_config(None);
        assert!(apply_settings_json_to_path(&Config::default(), bad, &path2).is_err());
    }

    /// Accounts are named by POSITION ("1", "2", … in config order): adds
    /// append, and the config file carries no label at all.
    #[test]
    fn adds_append_and_accounts_are_named_by_position() {
        let (_td, path) = temp_config(None);
        let patch = r#"{"schema_version":1,"accounts":{
            "kimi":[{"action":"add","api_key":{"action":"set","value":"k1"}}],
            "zai":[{"action":"add","api_key":{"action":"set","value":"z1"}}]}}"#;
        apply_settings_json_to_path(&Config::default(), patch, &path).unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert!(cfg.kimi.accounts.iter().any(|a| a.label == "1"));
        assert!(cfg.zai.accounts.iter().any(|a| a.label == "1"));

        // Second round: positions extend 1..n per vendor.
        let patch = r#"{"schema_version":1,"accounts":{
            "kimi":[{"action":"add"},{"action":"add"}],
            "zai":[{"action":"add"}]}}"#;
        apply_settings_json_to_path(&cfg, patch, &path).unwrap();
        let cfg = Config::load_from(&path).unwrap();
        let kimi_labels: Vec<&str> =
            cfg.kimi.accounts.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(kimi_labels, vec!["1", "2", "3"], "{kimi_labels:?}");
        let zai_labels: Vec<&str> = cfg.zai.accounts.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(zai_labels, vec!["1", "2"], "{zai_labels:?}");

        // No label key is ever written — position owns the identity.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("label"), "{raw}");

        // An explicit name on an add is accepted for old callers and simply
        // ignored: both entries below land as positions 1 and 2.
        let (_td2, path2) = temp_config(None);
        let patch = r#"{"schema_version":1,"accounts":{
            "zai":[{"action":"add","name":"whatever"},
                   {"action":"add","api_key":{"action":"set","value":"k"}}]}}"#;
        apply_settings_json_to_path(&Config::default(), patch, &path2).unwrap();
        let cfg = Config::load_from(&path2).unwrap();
        let zai_labels: Vec<&str> = cfg.zai.accounts.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(zai_labels, vec!["1", "2"], "{zai_labels:?}");
    }

    /// Every key-account vendor rides the same generic bridge: adds append
    /// positionally, monthly_limit exists only for the spend vendors, and
    /// the default section clears on removal.
    #[test]
    fn key_account_vendors_round_trip_through_the_generic_bridge() {
        // Two DeepSeek keys + a default, all positional.
        let (_td, path) = temp_config(None);
        let patch = r#"{"schema_version":1,"accounts":{
            "deepseek":[
                {"action":"add","api_key":{"action":"set","value":"d1"}},
                {"action":"add","api_key":{"action":"set","value":"d2"}}]}}"#;
        apply_settings_json_to_path(&Config::default(), patch, &path).unwrap();
        let cfg = Config::load_from(&path).unwrap();
        let labels: Vec<&str> = cfg.deepseek.accounts.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, vec!["1", "2"]);
        assert!(cfg.deepseek.enabled, "a key through the form opts the vendor in");
        // A limit on a non-spend vendor is refused.
        let bad = r#"{"schema_version":1,"accounts":{"deepseek":[
            {"action":"add","fields":{"monthly_limit":"100"}}]}}"#;
        assert!(apply_settings_json_to_path(&cfg, bad, &path).is_err());

        // Anthropic API accounts: key + own monthly limit per account.
        let (_td2, path2) = temp_config(None);
        let patch = r#"{"schema_version":1,"accounts":{
            "anthropic_api":[
                {"action":"add","api_key":{"action":"set","value":"admin1"},
                 "fields":{"monthly_limit":"500"}},
                {"action":"add","api_key":{"action":"set","value":"admin2"}}]}}"#;
        apply_settings_json_to_path(&Config::default(), patch, &path2).unwrap();
        let aapi_cfg = Config::load_from(&path2).unwrap();
        let accounts = &aapi_cfg.anthropic_api.accounts;
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].monthly_limit, Some(500.0));
        assert_eq!(accounts[1].monthly_limit, None);
        // Clearing the limit via the field's empty string removes it.
        let patch = r#"{"schema_version":1,"accounts":{
            "anthropic_api":[{"action":"update","label":"1","fields":{"monthly_limit":""}}]}}"#;
        apply_settings_json_to_path(&aapi_cfg, patch, &path2).unwrap();
        let aapi_cfg = Config::load_from(&path2).unwrap();
        assert_eq!(aapi_cfg.anthropic_api.accounts[0].monthly_limit, None);

        // The snapshot ships every key vendor as cards, spend vendors with
        // the limit field first and the key last.
        let raw = settings_snapshot_json_with(&aapi_cfg, |_| false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let aapi: Vec<&serde_json::Value> = parsed["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["vendor"] == "anthropic_api")
            .collect();
        assert!(aapi.len() >= 2, "{}", aapi.len());
        assert_eq!(aapi[0]["vendor_display"], "Anthropic API");
        let ids: Vec<&str> = aapi[1]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["monthly_limit", "api_key"]);

        // Removing a numbered account shifts the positions behind it.
        let patch = r#"{"schema_version":1,"accounts":{
            "deepseek":[{"action":"remove","label":"1"}]}}"#;
        apply_settings_json_to_path(&cfg, patch, &path).unwrap();
        let cfg = Config::load_from(&path).unwrap();
        let labels: Vec<&str> = cfg.deepseek.accounts.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, vec!["1"]);
        assert_eq!(cfg.deepseek.accounts[0].api_key.as_deref(), Some("d2"));
    }

    /// A config written by an older version (labels in `[[zai.accounts]]`)
    /// loads with positional names — the stored labels are ignored, and the
    /// settings bridge drops them the moment it touches an entry.
    #[test]
    fn legacy_labels_are_ignored_and_stripped_on_touch() {        let (_td, path) = temp_config(Some(
            "[[zai.accounts]]\nlabel = \"work\"\napi_key = \"k1\"\n\n\
             [[zai.accounts]]\nlabel = \"team\"\napi_key = \"k2\"\n",
        ));
        let cfg = Config::load_from(&path).unwrap();
        let labels: Vec<&str> = cfg.zai.accounts.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, vec!["1", "2"], "positions replace stored labels");

        let patch = r#"{"schema_version":1,"accounts":{"zai":[
            {"action":"update","label":"1","api_key":{"action":"clear"}}]}}"#;
        apply_settings_json_to_path(&cfg, patch, &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("label"), "touched entry lost its stale label: {raw}");
        let reloaded = Config::load_from(&path).unwrap();
        assert_eq!(reloaded.zai.accounts[0].api_key, None);
        assert_eq!(reloaded.zai.accounts[1].api_key.as_deref(), Some("k2"));
    }

    /// The native settings form's Z.AI account list: snapshot shape, and the
    /// add/update/remove mutations writing `[[zai.accounts]]` (default
    /// values by removal, org ids only on team accounts).
    #[test]
    fn zai_accounts_round_trip_through_the_native_bridge() {
        let mut cfg = Config::default();
        cfg.zai.api_key = Some("default-key".into());
        cfg.zai.accounts = vec![crate::config::ZaiAccount {
            label: "named".into(),
            api_key: Some("named-key".into()),
            api_key_env: None,
            plan_tier: None,
            account_type: crate::config::ZaiAccountType::Personal,
            organization_id: None,
            project_id: None,
            site: None,
        }];
        let raw = settings_snapshot_json_with(&cfg, |_| false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let accounts = parsed["accounts"].as_array().unwrap();
        // zai's two cards first; then the 11 key-account vendors' default
        // cards (empty states — their Add buttons).
        assert_eq!(accounts.len(), 13);
        assert_eq!(accounts[0]["label"], "");
        // Named accounts carry the name field first and the API key last.
        let named = &accounts[1];
        assert_eq!(named["label"], "1", "the panel sees the derived positional name");
        let ids: Vec<&str> = named["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["account_type", "api_key"],
            "no name field — the position is the name");

        let (_td, path) = temp_config(None);
        // Apply writes mutations, not snapshots: start from a config whose
        // account list is empty (the fixture above lived only in memory
        // for the snapshot assertions).
        let mut apply_cfg = Config::default();
        apply_cfg.zai.api_key = Some("default-key".into());
        let patch = r#"{"schema_version":1,"accounts":{"zai":[
            {"action":"add",
             "fields":{"site":"cn","account_type":"team",
                       "organization_id":"org-1","project_id":"proj-1"},
             "api_key":{"action":"set","value":"team-key"}},
            {"action":"add",
             "api_key":{"action":"set","value":"old-key"}}]}}"#;
        apply_settings_json_to_path(&apply_cfg, patch, &path).unwrap();
        let reloaded = Config::load_from(&path).unwrap();
        let labels: Vec<&str> = reloaded.zai.accounts.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, vec!["1", "2"]);
        let team = &reloaded.zai.accounts[0];
        assert_eq!(team.account_type, crate::config::ZaiAccountType::Team);
        assert_eq!(team.site, Some(crate::config::ZaiSite::Cn));
        assert_eq!(team.organization_id.as_deref(), Some("org-1"));
        assert_eq!(team.api_key.as_deref(), Some("team-key"));
        assert!(reloaded.zai.enabled, "a key through the form opts the vendor in");

        // Switch account 1 to usage by position: ids disappear, defaults by
        // removal, and the label stays "1".
        let patch = r#"{"schema_version":1,"accounts":{"zai":[
            {"action":"update","label":"1",
             "fields":{"account_type":"usage","site":""}}]}}"#;
        apply_settings_json_to_path(&reloaded, patch, &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("organization_id"), "{raw}");
        let reloaded = Config::load_from(&path).unwrap();
        let payg = &reloaded.zai.accounts[0];
        assert_eq!(payg.label, "1");
        assert_eq!(payg.account_type, crate::config::ZaiAccountType::Usage);
        assert_eq!(payg.site, None);

        // A default section without any key does not appear in the list at
        // all — the form shows just the add button (requirement: no key, no
        // default row).
        let mut bare = Config::default();
        bare.zai.accounts = reloaded.zai.accounts.clone();
        let raw = settings_snapshot_json_with(&bare, |_| false).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let zai_labels: Vec<&str> = parsed["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["vendor"] == "zai")
            .map(|a| a["label"].as_str().unwrap())
            .collect();
        assert_eq!(zai_labels, vec!["1", "2"], "no zai default row: {zai_labels:?}");

        // Remove + re-add in ONE save works: positions follow the mutations
        // in order (add → 1, remove 1, add → 1 again).
        let (_td3, path3) = temp_config(None);
        let patch = r#"{"schema_version":1,"accounts":{"zai":[
            {"action":"add","api_key":{"action":"set","value":"k1"}},
            {"action":"remove","label":"1"},
            {"action":"add","api_key":{"action":"set","value":"k2"}}]}}"#;
        apply_settings_json_to_path(&Config::default(), patch, &path3).unwrap();
        let recycled = Config::load_from(&path3).unwrap();
        assert_eq!(recycled.zai.accounts.len(), 1);
        assert_eq!(recycled.zai.accounts[0].label, "1");
        assert_eq!(recycled.zai.accounts[0].api_key.as_deref(), Some("k2"));
        // Two plain adds in one request are fine now — no names to collide.
        let dup = r#"{"schema_version":1,"accounts":{"zai":[
            {"action":"add"},
            {"action":"add"}]}}"#;
        let (_td4, path4) = temp_config(None);
        apply_settings_json_to_path(&Config::default(), dup, &path4).unwrap();
        let two = Config::load_from(&path4).unwrap();
        let labels: Vec<&str> = two.zai.accounts.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, vec!["1", "2"]);

        // Removing the default resets the section: key + billing fields go.
        let mut with_default = bare.clone();
        with_default.zai = reloaded.zai.clone();
        let (_td2, path2) = temp_config(None);
        let patch = r#"{"schema_version":1,"accounts":{"zai":[
            {"action":"remove","label":""}]}}"#;
        apply_settings_json_to_path(&with_default, patch, &path2).unwrap();
        let reset = Config::load_from(&path2).unwrap();
        assert_eq!(reset.zai.api_key, None);
        assert_eq!(reset.zai.account_type, crate::config::ZaiAccountType::Personal);
        assert_eq!(reset.zai.organization_id, None);

        // Update the default section's fields, clear the old account, remove it.
        let patch = r#"{"schema_version":1,"accounts":{"zai":[
            {"action":"update","label":"",
             "fields":{"account_type":"team","organization_id":"d-org","project_id":"d-proj"}},
            {"action":"update","label":"2",
             "api_key":{"action":"clear"}},
            {"action":"remove","label":"2"}]}}"#;
        apply_settings_json_to_path(&reloaded, patch, &path).unwrap();
        let reloaded = Config::load_from(&path).unwrap();
        assert_eq!(reloaded.zai.account_type, crate::config::ZaiAccountType::Team);
        assert_eq!(reloaded.zai.organization_id.as_deref(), Some("d-org"));
        assert_eq!(reloaded.zai.accounts.len(), 1);
    }

    #[test]
    fn zai_field_patches_are_validated() {
        let cfg = Config::default();
        for bad in [
            // Unknown field.
            r#"{"schema_version":1,"keys":{"zai":{"action":"fields","fields":{"nope":"x"}}}}"#,
            // Bad enum.
            r#"{"schema_version":1,"keys":{"zai":{"action":"fields","fields":{"account_type":"enterprise"}}}}"#,
            // Bad site.
            r#"{"schema_version":1,"keys":{"zai":{"action":"fields","fields":{"site":"eu"}}}}"#,
            // Control characters.
            r#"{"schema_version":1,"keys":{"zai":{"action":"fields","fields":{"organization_id":"a\u0000b"}}}}"#,
            // Fields on a vendor that has none.
            r#"{"schema_version":1,"keys":{"kimi":{"action":"fields","fields":{"account_type":"team"}}}}"#,
        ] {
            let (_td, path) = temp_config(None);
            assert!(
                apply_settings_json_to_path(&cfg, bad, &path).is_err(),
                "expected rejection: {bad}"
            );
        }
    }

    fn key_index(id: VendorId) -> usize {
        KEY_VENDORS.iter().position(|kv| kv.id == id).unwrap()
    }

    fn blank_state(primary: VendorId) -> SettingsState {
        SettingsState {
            focus: Focus::Primary,
            primary_choices: VendorId::all().to_vec(),
            primary,
            keys: KEY_VENDORS.iter().map(|_| KeyInput::default()).collect(),
            zai: ZaiFields::default(),
            status: String::new(),
        }
    }

    /// State with a Z.AI key and an OpenRouter key, both marked dirty.
    fn state_with(zai: &str, opr: &str, primary: VendorId) -> SettingsState {
        let mut s = blank_state(primary);
        s.keys[key_index(VendorId::Zai)] = KeyInput::from_config(Some(zai));
        s.keys[key_index(VendorId::Zai)].dirty = true;
        s.keys[key_index(VendorId::Openrouter)] = KeyInput::from_config(Some(opr));
        s.keys[key_index(VendorId::Openrouter)].dirty = true;
        s
    }

    #[test]
    fn focus_cycles_through_primary_all_keys_and_save() {
        let mut f = Focus::Primary;
        let mut seen = vec![f];
        // Full cycle = Primary + N key rows + Save.
        for _ in 0..(KEY_VENDORS.len() + 2) {
            f = f.next();
            seen.push(f);
        }
        // Primary, Key(0..n), Save, back to Primary.
        assert_eq!(seen.first(), Some(&Focus::Primary));
        assert_eq!(seen.last(), Some(&Focus::Primary));
        assert!(seen.contains(&Focus::Key(0)));
        assert!(seen.contains(&Focus::Key(KEY_VENDORS.len() - 1)));
        assert!(seen.contains(&Focus::Save));
        // prev() is the inverse of next().
        assert_eq!(Focus::Primary.next().prev(), Focus::Primary);
        assert_eq!(Focus::Save.prev().next(), Focus::Save);
        assert_eq!(Focus::Primary.prev(), Focus::Save);
    }

    #[test]
    fn every_key_vendor_has_a_field() {
        // Every enabled-by-key vendor must be reachable in the form.
        for id in [
            VendorId::Zai,
            VendorId::Openrouter,
            VendorId::Deepseek,
            VendorId::Kilo,
            VendorId::Novita,
            VendorId::Moonshot,
            VendorId::Grok,
        ] {
            assert!(
                KEY_VENDORS.iter().any(|kv| kv.id == id),
                "{id:?} has no key field"
            );
        }
        // OAuth vendors are intentionally absent.
        assert!(!KEY_VENDORS.iter().any(|kv| kv.id == VendorId::Anthropic));
        assert!(!KEY_VENDORS.iter().any(|kv| kv.id == VendorId::Openai));
    }

    #[test]
    fn from_config_prefills_existing_keys() {
        let mut cfg = Config::default();
        cfg.kilo.api_key = Some("sk-kilo".into());
        let s = SettingsState::from_config(&cfg);
        assert_eq!(s.keys[key_index(VendorId::Kilo)].buf, "sk-kilo");
        assert!(!s.keys[key_index(VendorId::Kilo)].dirty);
    }

    #[test]
    fn from_config_offers_enabled_vendors_only() {
        let cfg = Config::default();
        let s = SettingsState::from_config(&cfg);
        assert_eq!(s.primary_choices, cfg.enabled_vendors());
        // Opt-in vendors are disabled by default and must not be offered.
        assert!(!s.primary_choices.contains(&VendorId::Grok));
        assert!(s.primary_choices.contains(&s.primary));
    }

    #[test]
    fn from_config_falls_back_when_configured_primary_is_disabled() {
        // Grok is opt-in; a config naming it as primary without enabling it
        // must display the first enabled vendor instead.
        let mut cfg = Config::default();
        cfg.ui.primary = Some(VendorId::Grok);
        let s = SettingsState::from_config(&cfg);
        assert_ne!(s.primary, VendorId::Grok);
        assert_eq!(Some(s.primary), cfg.enabled_vendors().first().copied());
    }

    #[test]
    fn key_input_insert_backspace_arrow() {
        let mut k = KeyInput::default();
        k.insert_char('a');
        k.insert_char('b');
        k.insert_char('c');
        assert_eq!(k.buf, "abc");
        assert_eq!(k.cursor, 3);
        assert!(k.dirty);
        k.move_left();
        k.move_left();
        assert_eq!(k.cursor, 1);
        k.insert_char('x');
        assert_eq!(k.buf, "axbc");
        assert_eq!(k.cursor, 2);
        k.backspace();
        assert_eq!(k.buf, "abc");
        assert_eq!(k.cursor, 1);
    }

    #[test]
    fn key_input_masks_by_default_reveals_on_toggle() {
        let mut k = KeyInput::default();
        for c in "secret-key".chars() {
            k.insert_char(c);
        }
        assert_eq!(k.display(), "•".repeat(10));
        k.toggle_reveal();
        assert_eq!(k.display(), "secret-key");
    }

    #[test]
    fn key_input_handles_unicode() {
        let mut k = KeyInput::default();
        k.insert_char('a');
        k.insert_char('→');
        k.insert_char('b');
        assert_eq!(k.buf, "a→b");
        assert_eq!(k.cursor, 3);
        k.move_left();
        k.backspace();
        assert_eq!(k.buf, "ab");
    }

    #[test]
    fn value_text_shows_cursor_and_empty_states() {
        let mut k = KeyInput::default();
        assert_eq!(value_text(&k, false), "(empty)");
        assert_eq!(value_text(&k, true), "‸");
        k.insert_char('a');
        k.insert_char('b');
        // masked + cursor at end
        assert_eq!(value_text(&k, true), "••‸");
        assert_eq!(value_text(&k, false), "••");
    }

    #[test]
    fn save_writes_key_and_enables_vendor() {
        let (_dir, path) = temp_config(None);
        let mut s = blank_state(VendorId::Kilo);
        s.keys[key_index(VendorId::Kilo)] = KeyInput::from_config(Some("sk-kilo"));
        s.keys[key_index(VendorId::Kilo)].dirty = true;
        save_to_path(&s, &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("primary = \"kilo\""));
        assert!(raw.contains("[kilo]"));
        assert!(raw.contains("api_key = \"sk-kilo\""));
        assert!(raw.contains("enabled = true"));
    }

    #[test]
    fn save_writes_minimal_toml_when_starting_empty() {
        let (_dir, path) = temp_config(None);
        let s = state_with("zk", "ok", VendorId::Zai);
        save_to_path(&s, &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("primary = \"zai\""));
        assert!(raw.contains("[zai]"));
        assert!(raw.contains("api_key = \"zk\""));
        assert!(raw.contains("[openrouter]"));
        assert!(raw.contains("api_key = \"ok\""));
    }

    #[test]
    fn save_preserves_existing_comments_and_unrelated_fields() {
        let (_dir, path) = temp_config(Some(
            r##"# my comment
[ui]
# pre-existing comment
primary = "anthropic"

[zai]
enabled = true
api_key_env = "ZAI_API_KEY"
# tier comment
plan_tier = "pro"

[openrouter]
enabled = true
api_key_env = "OPENROUTER_API_KEY"

[[openrouter.accounts]]
label = "work"
api_key_env = "OPENROUTER_WORK_API_KEY"
"##,
        ));

        let s = state_with("zk2", "ok2", VendorId::Openrouter);
        save_to_path(&s, &path).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("# my comment"));
        assert!(raw.contains("# pre-existing comment"));
        assert!(raw.contains("# tier comment"));
        assert!(raw.contains("api_key_env = \"ZAI_API_KEY\""));
        assert!(raw.contains("[[openrouter.accounts]]"));
        assert!(raw.contains("api_key_env = \"OPENROUTER_WORK_API_KEY\""));
        assert!(raw.contains("plan_tier = \"pro\""));
        assert!(raw.contains("primary = \"openrouter\""));
        assert!(raw.contains("api_key = \"zk2\""));
        assert!(raw.contains("api_key = \"ok2\""));
    }

    #[test]
    fn save_refuses_to_replace_an_unreadable_existing_config() {
        let (_dir, path) = temp_config(None);
        let original = [0xff, 0xfe, 0xfd];
        std::fs::write(&path, original).unwrap();
        let state = state_with("new-secret", "", VendorId::Zai);

        assert!(save_to_path(&state, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn save_does_not_write_empty_key_when_dirty_but_blank() {
        let (_dir, path) = temp_config(None);
        let mut s = blank_state(VendorId::Anthropic);
        // Focus each key, do nothing but mark dirty (blank).
        for k in &mut s.keys {
            k.dirty = true;
        }
        save_to_path(&s, &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("api_key ="));
    }

    #[test]
    #[cfg(unix)]
    fn save_chmods_to_600() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, path) = temp_config(None);
        let s = state_with("zk", "ok", VendorId::Zai);
        save_to_path(&s, &path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn tab_cycles_focus_from_primary_to_first_key() {
        let mut s = blank_state(VendorId::Anthropic);
        assert_eq!(
            handle_key(&mut s, KeyCode::Tab, KeyModifiers::NONE),
            Action::Continue
        );
        assert_eq!(s.focus, Focus::Key(0));
        assert_eq!(
            handle_key(&mut s, KeyCode::BackTab, KeyModifiers::NONE),
            Action::Continue
        );
        assert_eq!(s.focus, Focus::Primary);
    }

    #[test]
    fn esc_closes_without_saving() {
        let mut s = blank_state(VendorId::Anthropic);
        assert_eq!(
            handle_key(&mut s, KeyCode::Esc, KeyModifiers::NONE),
            Action::Close
        );
    }

    #[test]
    fn left_right_cycles_primary_vendor() {
        // Canonical order (VendorId::all): Anthropic, AnthropicApi, Openai, …
        let mut s = blank_state(VendorId::Anthropic);
        handle_key(&mut s, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(s.primary, VendorId::AnthropicApi);
        handle_key(&mut s, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(s.primary, VendorId::Openai);
        handle_key(&mut s, KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(s.primary, VendorId::AnthropicApi);
    }

    #[test]
    fn left_right_offers_enabled_vendors_only() {
        // The selector must never land on a vendor the widget cannot use.
        let mut s = blank_state(VendorId::Anthropic);
        s.primary_choices = vec![VendorId::Anthropic, VendorId::Grok];
        handle_key(&mut s, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(s.primary, VendorId::Grok);
        // Wraps within the enabled set rather than walking into disabled ones.
        handle_key(&mut s, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(s.primary, VendorId::Anthropic);
        handle_key(&mut s, KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(s.primary, VendorId::Grok);
    }

    #[test]
    fn no_enabled_vendors_leaves_primary_selector_inert() {
        let mut s = blank_state(VendorId::Anthropic);
        s.primary_choices = vec![];
        handle_key(&mut s, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(s.primary, VendorId::Anthropic);
    }

    #[test]
    fn save_does_not_write_a_disabled_primary() {
        // Saving an API key must not persist a primary the resolver would
        // ignore; an existing value in the file stays untouched.
        let (_dir, path) = temp_config(Some("[ui]\nprimary = \"anthropic\"\n"));
        let mut s = state_with("zk", "ok", VendorId::Grok);
        s.primary_choices = vec![VendorId::Anthropic];
        save_to_path(&s, &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("primary = \"anthropic\""));
        assert!(!raw.contains("primary = \"grok\""));
        // The keys still saved.
        assert!(raw.contains("zk"));
    }

    #[test]
    fn save_removes_an_inline_key_the_user_cleared() {
        // Clearing the field in the overlay must delete the secret from the
        // file — otherwise there is no way to remove it short of hand-editing.
        let (_dir, path) = temp_config(Some(
            "[zai]\nenabled = true\napi_key = \"old-secret\"\nplan_tier = \"pro\"\n",
        ));
        let mut s = blank_state(VendorId::Zai);
        s.primary_choices = vec![VendorId::Zai];
        s.keys[key_index(VendorId::Zai)] = KeyInput::default();
        s.keys[key_index(VendorId::Zai)].dirty = true;
        save_to_path(&s, &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("old-secret"));
        assert!(!raw.contains("api_key"));
        // Unrelated fields in the same section survive.
        assert!(raw.contains("plan_tier = \"pro\""));
    }

    #[test]
    fn untouched_key_field_is_left_alone() {
        // Not dirty => the file's existing secret must survive a save.
        let (_dir, path) = temp_config(Some("[zai]\napi_key = \"keep-me\"\n"));
        let mut s = blank_state(VendorId::Zai);
        s.primary_choices = vec![VendorId::Zai];
        save_to_path(&s, &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("keep-me"));
    }

    #[test]
    fn typing_edits_the_focused_key_only() {
        let mut s = blank_state(VendorId::Anthropic);
        s.focus = Focus::Key(key_index(VendorId::Grok));
        for c in "xai-abc".chars() {
            handle_key(&mut s, KeyCode::Char(c), KeyModifiers::NONE);
        }
        assert_eq!(s.keys[key_index(VendorId::Grok)].buf, "xai-abc");
        assert!(s.keys[key_index(VendorId::Grok)].dirty);
        // No other field was touched.
        assert!(s.keys[key_index(VendorId::Zai)].buf.is_empty());
    }

    #[test]
    fn ctrl_v_toggles_reveal_on_focused_key_field() {
        let mut s = blank_state(VendorId::Anthropic);
        let zi = key_index(VendorId::Zai);
        s.focus = Focus::Key(zi);
        s.keys[zi] = KeyInput::from_config(Some("secret"));
        assert!(!s.keys[zi].revealed);
        handle_key(&mut s, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert!(s.keys[zi].revealed);
        handle_key(&mut s, KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert!(!s.keys[zi].revealed);
    }

    #[test]
    fn control_chorded_chars_do_not_type_into_fields() {
        let mut s = blank_state(VendorId::Anthropic);
        s.focus = Focus::Key(0);
        // Ctrl-A must NOT insert a literal 'a' or mark the field dirty.
        handle_key(&mut s, KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert!(s.keys[0].buf.is_empty());
        assert!(!s.keys[0].dirty);
        // Ctrl-C quits the host TUI even while the overlay owns focus.
        assert_eq!(
            handle_key(&mut s, KeyCode::Char('c'), KeyModifiers::CONTROL),
            Action::Quit
        );
        // A plain char still types normally.
        handle_key(&mut s, KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(s.keys[0].buf, "x");
    }

    #[test]
    fn ctrl_v_on_non_key_focus_is_noop() {
        let mut s = blank_state(VendorId::Anthropic);
        s.focus = Focus::Primary;
        // Must not panic when no key field is focused.
        assert_eq!(
            handle_key(&mut s, KeyCode::Char('v'), KeyModifiers::CONTROL),
            Action::Continue
        );
    }

    fn state_focused_on_zai() -> SettingsState {
        let mut state = blank_state(VendorId::Anthropic);
        state.focus = Focus::Key(key_index(VendorId::Zai));
        state
    }

    #[test]
    fn handle_key_ctrl_c_quits_without_typing_into_key_field() {
        let mut s = state_focused_on_zai();
        let zi = key_index(VendorId::Zai);
        assert_eq!(
            handle_key(&mut s, KeyCode::Char('c'), KeyModifiers::CONTROL),
            Action::Quit
        );
        assert!(s.keys[zi].buf.is_empty());
        // Untouched means save still leaves an existing key on disk alone.
        assert!(!s.keys[zi].dirty);
    }

    #[test]
    fn handle_key_alt_chord_does_not_type_into_key_field() {
        let mut s = state_focused_on_zai();
        let zi = key_index(VendorId::Zai);
        handle_key(&mut s, KeyCode::Char('x'), KeyModifiers::ALT);
        assert!(s.keys[zi].buf.is_empty());
        assert!(!s.keys[zi].dirty);
    }

    #[test]
    fn handle_key_platform_modifier_chords_do_not_type_into_key_field() {
        for modifier in [KeyModifiers::SUPER, KeyModifiers::HYPER, KeyModifiers::META] {
            let mut s = state_focused_on_zai();
            let zi = key_index(VendorId::Zai);
            handle_key(&mut s, KeyCode::Char('x'), modifier);
            assert!(s.keys[zi].buf.is_empty(), "modifier {modifier:?}");
            assert!(!s.keys[zi].dirty, "modifier {modifier:?}");
        }
    }

    #[test]
    fn handle_key_shift_still_types_uppercase() {
        let mut s = state_focused_on_zai();
        let zi = key_index(VendorId::Zai);
        handle_key(&mut s, KeyCode::Char('A'), KeyModifiers::SHIFT);
        assert_eq!(s.keys[zi].buf, "A");
        assert!(s.keys[zi].dirty);
    }

    #[test]
    fn handle_key_plain_space_still_cycles_primary_vendor() {
        let mut s = blank_state(VendorId::Anthropic);
        handle_key(&mut s, KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(s.primary, VendorId::AnthropicApi);
    }

    #[test]
    fn handle_key_ctrl_s_attempts_save_from_any_field() {
        let (_dir, path) = temp_config(None);
        let s = state_with("zk", "ok", VendorId::Zai);
        save_to_path(&s, &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("api_key = \"zk\""));
    }
    #[test]
    fn save_to_path_writes_kimi_key_when_dirty() {
        let (_dir, path) = temp_config(None);
        let mut s = blank_state(VendorId::Anthropic);
        let kimi = key_index(VendorId::Kimi);
        s.keys[kimi] = KeyInput::from_config(Some("kk"));
        s.keys[kimi].dirty = true;
        save_to_path(&s, &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("[kimi]"));
        assert!(raw.contains("api_key = \"kk\""));
    }

    #[test]
    fn settings_save_uses_the_same_config_path_as_load() {
        assert_eq!(
            default_config_path().unwrap(),
            crate::config::resolved_path().unwrap()
        );
    }

    #[test]
    fn native_snapshot_reports_key_state_without_serializing_secrets() {
        let mut cfg = Config::default();
        cfg.zai.api_key = Some("never-leak-this-key".into());
        cfg.zai.api_key_env = "CUSTOM_ZAI_KEY".into();
        let raw = settings_snapshot_json_with(&cfg, |name| name == "CUSTOM_ZAI_KEY").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();

        assert_eq!(parsed["schema_version"], 1);
        assert_eq!(parsed["primary"], "anthropic");
        // Z.AI and every key-account vendor are account lists now — absent
        // from the plain key cards entirely…
        assert!(parsed["keys"].as_array().unwrap().is_empty());
        // …present as account cards: Z.AI's configured default first, then
        // every other key vendor's empty-state default card (its Add button
        // is the entry point when nothing is configured).
        let accounts = parsed["accounts"].as_array().unwrap();
        assert_eq!(accounts.len(), 12, "zai + the 11 key-account vendors");
        let zai = &accounts[0];
        assert_eq!(zai["vendor"], "zai");
        assert_eq!(zai["label"], "");
        assert_eq!(zai["display"], "Default");
        assert_eq!(zai["configured"], true);
        assert_eq!(zai["inline_configured"], true);
        assert_eq!(zai["environment_configured"], true);
        assert_eq!(zai["environment"], "CUSTOM_ZAI_KEY");
        assert_eq!(zai["vendor_display"], "Z.AI");
        // Form order: account type, (team ids), API key LAST; no name field
        // anymore — the bar tags accounts with the provider logo.
        let ids: Vec<&str> = zai["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["account_type", "api_key"]);
        assert_eq!(zai["fields"][1]["kind"], "secret");
        // No site field — it is auto-detected from the account type; the
        // config file keeps the `site` override for hand-edited edge cases.
        assert!(!raw.contains("\"site\""));
        assert!(!raw.contains("never-leak-this-key"));
        assert!(parsed.get("api_key").is_none());
    }

    #[test]
    fn native_key_only_patch_does_not_require_or_replace_primary() {
        let cfg = Config::default();
        let original_primary = SettingsState::from_config(&cfg).primary;
        let request = serde_json::json!({
            "schema_version": 1,
            "keys": {"kimi": {"action": "set", "value": "new-kimi-key"}}
        });

        let state = state_from_apply_request(&cfg, &request.to_string()).unwrap();
        assert_eq!(state.primary, original_primary);
        let kimi_index = KEY_VENDORS
            .iter()
            .position(|vendor| vendor.id == VendorId::Kimi)
            .unwrap();
        assert!(state.keys[kimi_index].dirty);
        assert_eq!(state.keys[kimi_index].buf, "new-kimi-key");
    }

    #[test]
    fn native_patch_reuses_tui_persistence_and_preserves_existing_config() {
        let (_dir, path) = temp_config(Some(
            r#"# keep this comment
[ui]
primary = "anthropic"

[zai]
enabled = true
api_key_env = "ZAI_API_KEY"
plan_tier = "pro"

[openrouter]
enabled = true
"#,
        ));
        let cfg = Config::load_from(&path).unwrap();
        let request = serde_json::json!({
            "schema_version": 1,
            "primary": "openrouter",
            "keys": {
                "zai": {"action": "set", "value": "new-zai-key"}
            }
        });

        apply_settings_json_to_path(&cfg, &request.to_string(), &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("# keep this comment"));
        assert!(raw.contains("plan_tier = \"pro\""));
        assert!(raw.contains("api_key_env = \"ZAI_API_KEY\""));
        assert!(raw.contains("primary = \"openrouter\""));
        assert!(raw.contains("api_key = \"new-zai-key\""));
    }

    #[test]
    fn native_patch_distinguishes_clear_from_unchanged() {
        let (_dir, path) = temp_config(Some(
            "[zai]\nenabled = true\napi_key = \"remove-me\"\n\
             [openrouter]\nenabled = true\napi_key = \"keep-me\"\n",
        ));
        let cfg = Config::load_from(&path).unwrap();
        let request = serde_json::json!({
            "schema_version": 1,
            "primary": "zai",
            "keys": {"zai": {"action": "clear"}}
        });

        apply_settings_json_to_path(&cfg, &request.to_string(), &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("remove-me"));
        assert!(raw.contains("keep-me"));
    }

    #[test]
    fn native_patch_errors_never_echo_key_values() {
        let raw = serde_json::json!({
            "schema_version": 1,
            "primary": "anthropic",
            "keys": {
                "zai": {"action": "set", "value": "secret\nwith-control"}
            }
        })
        .to_string();
        let error = state_from_apply_request(&Config::default(), &raw)
            .unwrap_err()
            .to_string();
        assert!(!error.contains("secret"));
        assert!(error.contains("control characters"));
    }

    #[test]
    fn native_patch_input_is_bounded_before_json_parsing() {
        let oversized = vec![b'x'; MAX_SETTINGS_REQUEST_BYTES as usize + 1];
        let error = read_settings_request(std::io::Cursor::new(oversized))
            .unwrap_err()
            .to_string();
        assert!(error.contains("exceeds"));
    }
}
