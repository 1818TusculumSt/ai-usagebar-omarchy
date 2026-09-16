//! Config file at `~/.config/ai-usagebar-omarchy/config.toml` (pre-rename `~/.config/ai-usagebar-omarchy/` is migrated on first run — see `migrate_legacy_layout`).
//!
//! Layout:
//! ```toml
//! [anthropic]  enabled = true
//! [openai]     enabled = true   # Codex OAuth from ~/.codex/auth.json
//! [zai]        enabled = true
//! [openrouter] enabled = true
//! [deepseek]   enabled = false
//! [kimi]       enabled = false
//! ```
//!
//! Every field is optional with sensible defaults — a missing config file is
//! treated as "use defaults", except that the first run drops the annotated
//! [`EXAMPLE_CONFIG`] template at the canonical path so there is a commented
//! file to fill in (see [`seed_example_config`]; best-effort). API keys are
//! read from env vars (the relevant `*_api_key_env` field lets the user
//! override which env var name).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use serde::{Deserialize, Serialize};

use crate::anthropic::creds::CredsTarget;
use crate::cache::Cache;
use crate::error::{AppError, Result};
use crate::vendor::VendorId;

/// A misspelled section name is silently ignored without this: `[openrouer]`
/// leaves OpenRouter on its defaults and the user sees the wrong vendor set
/// with no diagnostic. Denying unknown keys is deliberately applied at the
/// *section* level only — the set of sections is small and stable, whereas
/// denying unknown keys inside every section would hard-fail configs that
/// carry a field from a future or removed version.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub ui: UiConfig,
    pub context: ContextConfig,
    pub anthropic: AnthropicConfig,
    pub anthropic_api: AnthropicApiConfig,
    pub openai_api: OpenaiApiConfig,
    pub openai: OpenAiConfig,
    pub zai: ZaiConfig,
    pub openrouter: OpenRouterConfig,
    pub deepseek: DeepseekConfig,
    pub kimi: KimiConfig,
    pub kilo: KiloConfig,
    pub novita: NovitaConfig,
    pub moonshot: MoonshotConfig,
    pub grok: GrokConfig,
    pub supergrok: SuperGrokConfig,
    pub antigravity: AntigravityConfig,
    pub cursor: CursorConfig,
    pub minimax: MinimaxConfig,
    pub kiro: KiroConfig,
    pub copilot: CopilotConfig,
    #[serde(rename = "opencode-go")]
    pub opencode_go: OpenCodeGoConfig,
}

/// UI / dispatch preferences. Currently just `primary` — which vendor the
/// widget shows when `--vendor` is omitted, and which TUI tab is selected
/// at startup.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct UiConfig {
    /// `None` → fall back to anthropic for backward compatibility.
    pub primary: Option<VendorId>,
    /// Which vendors the Overview shows (the TUI's first tab and the macOS
    /// menu-bar's top section), in this order. `None` → every enabled vendor,
    /// in the canonical order.
    pub overview_vendors: Option<Vec<VendorId>>,
    /// Layout style for vendor navigation in the TUI: sidebar | navbar | none.
    pub vendor_box: Option<VendorBoxStyle>,
}

impl UiConfig {
    pub fn vendor_box(&self) -> VendorBoxStyle {
        self.vendor_box.unwrap_or_default()
    }
}

/// Presentation style of the TUI vendor navigation box.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VendorBoxStyle {
    /// Vertical sidebar box on wide terminals; falls back to top navbar on narrow terminals.
    #[default]
    Sidebar,
    /// Horizontal navbar strip above the dashboard detail panel.
    Navbar,
    /// Completely hide vendor navigation (dashboards expand to fill full width).
    None,
}

/// Where the context view docks in the dashboard body. `v` cycles it while the
/// overlay is open; the config value is what it opens with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContextLayout {
    /// Takes the whole body, the way a vendor panel does.
    #[default]
    Full,
    /// Beside the dashboard.
    Split,
    /// Below the dashboard.
    Bottom,
}

impl ContextLayout {
    pub fn next(self) -> Self {
        match self {
            ContextLayout::Full => ContextLayout::Split,
            ContextLayout::Split => ContextLayout::Bottom,
            ContextLayout::Bottom => ContextLayout::Full,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ContextLayout::Full => "full",
            ContextLayout::Split => "split",
            ContextLayout::Bottom => "bottom",
        }
    }
}

/// Optional local Claude Code context-window monitor. This is deliberately
/// separate from vendors: sessions are discovered from local transcripts and
/// change while the TUI is running, whereas vendor tabs are config-declared
/// account identities.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ContextConfig {
    /// Keep the filesystem scanner completely dormant unless explicitly
    /// enabled. The `c` key and its footer hint are hidden while disabled.
    pub enabled: bool,
    /// Override Claude Code's normal `~/.claude/projects` transcript root.
    pub projects_path: Option<PathBuf>,
    /// Optional fallback denominator. When absent, sessions without an exact
    /// model override show their input-token count without inventing a %.
    pub context_window_tokens: Option<u64>,
    /// Exact Claude model id -> context-window size. This takes precedence
    /// over `context_window_tokens`, which keeps mixed 200K/1M histories safe.
    pub model_context_window_tokens: BTreeMap<String, u64>,
    /// Where the view opens: full | split | bottom.
    pub layout: ContextLayout,
}

impl ContextConfig {
    pub fn window_tokens_for(&self, model: Option<&str>) -> Option<u64> {
        model
            .and_then(|model| self.model_context_window_tokens.get(model).copied())
            .filter(|tokens| *tokens > 0)
            .or_else(|| self.context_window_tokens.filter(|tokens| *tokens > 0))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AnthropicConfig {
    pub enabled: bool,
    /// Override the credentials file path (defaults to `~/.claude/.credentials.json`).
    /// This is the *default* account; extra subscriptions go in `accounts`.
    pub credentials_path: Option<PathBuf>,
    /// Extra Anthropic accounts beyond the default, each selected on the CLI
    /// with `--account <label>` (issue #14). Empty by default, so existing
    /// single-account configs are byte-for-byte unchanged.
    pub accounts: Vec<AnthropicAccount>,
    /// Directory to auto-discover extra accounts from, in Claude Code's own
    /// `CLAUDE_CONFIG_DIR` layout: each immediate subdirectory becomes an
    /// account labeled by the subdirectory name. The credentials may live in
    /// that directory's `.credentials.json` or in the macOS Keychain, so
    /// discovery intentionally does not probe for the credentials file.
    /// Merged with `accounts` (explicit wins on a label clash); each is
    /// refreshed independently.
    pub accounts_dir: Option<PathBuf>,
    /// Whether the default (unnamed) Claude account gets its own tab. Defaults
    /// to `true` for back-compat. Set `false` when every account is managed
    /// explicitly (via `accounts`/`accounts_dir`) so the ambient
    /// Keychain/`~/.claude` login doesn't add a redundant "Claude" tab. Ignored
    /// when there are no named accounts, so Anthropic never loses its only tab.
    pub show_default_account: bool,
    /// Where the Claude **Desktop app**'s saved account profiles live. Defaults
    /// to `~/.claude-acc/profiles`, the store claude-acc
    /// (<https://github.com/ohmaseclaro/claude-acc>) creates — `account switch`
    /// reads and writes that layout so the two tools stay interchangeable.
    /// Unrelated to `accounts_dir`, which is the `claude` CLI's own accounts.
    pub desktop_profiles_dir: Option<PathBuf>,
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            credentials_path: None,
            accounts: Vec::new(),
            accounts_dir: None,
            show_default_account: true,
            desktop_profiles_dir: None,
        }
    }
}

/// One extra Anthropic account beyond the default (issue #14). The default
/// account stays the singular `[anthropic] credentials_path`; each entry here
/// is an additional subscription selected on the CLI with `--account <label>`.
///
/// ```toml
/// [[anthropic.accounts]]
/// label = "work"
/// credentials_path = "~/.config/ai-usagebar-omarchy/accounts/work/.credentials.json"
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AnthropicAccount {
    /// Stable name used on the CLI (`--account <label>`) and as the cache
    /// subdir (`~/.cache/ai-usagebar-omarchy/anthropic/<label>`).
    pub label: String,
    /// OAuth credentials file for this account (same JSON shape Claude Code
    /// writes). Token refreshes are written back here, so each account keeps
    /// itself alive independently.
    pub credentials_path: PathBuf,
}

impl AnthropicAccount {
    /// The `CLAUDE_CONFIG_DIR` this account occupies — the credential file's
    /// own directory. Claude Code hashes exactly this path for the account's
    /// Keychain item, so it is also the account's identity for
    /// [`crate::anthropic::keychain`].
    pub fn config_dir(&self) -> PathBuf {
        self.credentials_path
            .parent()
            .map_or_else(|| self.credentials_path.clone(), Path::to_path_buf)
    }
}

impl AnthropicConfig {
    /// Every extra account: the explicit `[[anthropic.accounts]]` entries plus
    /// any auto-discovered under [`accounts_dir`](AnthropicConfig::accounts_dir).
    /// Explicit entries take precedence on a label clash. This is what tabs and
    /// `--account` enumerate, so a discovered account behaves exactly like a
    /// hand-written one (own cache subdir, independent refresh).
    pub fn all_accounts(&self) -> Vec<AnthropicAccount> {
        let mut out = self.accounts.clone();
        if let Some(dir) = &self.accounts_dir {
            for acct in discover_accounts(dir) {
                if !out.iter().any(|a| a.label == acct.label) {
                    out.push(acct);
                }
            }
        }
        out
    }

    /// Find an extra account by label (explicit or discovered), or error listing
    /// the known labels so a typo fails loudly instead of silently hitting the
    /// default. Returns an owned account because discovered entries are
    /// synthesized, not stored.
    pub fn account(&self, label: &str) -> Result<AnthropicAccount> {
        validate_account_label(label)?;
        let all = self.all_accounts();
        all.iter().find(|a| a.label == label).cloned().ok_or_else(|| {
            let known: Vec<&str> = all.iter().map(|a| a.label.as_str()).collect();
            AppError::Credentials(format!(
                "anthropic account {label:?} not found in [[anthropic.accounts]] or accounts_dir; \
                 known labels: {known:?}"
            ))
        })
    }

    /// Resolve a named account to the credentials target + isolated cache it
    /// fetches through: [`CredsTarget::Named`], which on macOS prefers the
    /// Keychain item scoped to the file's own directory (that is where
    /// `CLAUDE_CONFIG_DIR=<dir> claude` actually writes) and falls back to
    /// the file elsewhere — never a *different* account's item, since the
    /// hash is per-directory, so issue #15's cross-account concern doesn't
    /// apply. Plus an `anthropic/<label>` cache subdir. Shared by the widget
    /// (`--account`) and the TUI's per-account tab (#14, #17) so both resolve
    /// accounts identically; the widget layers its `--cache-dir` override on
    /// top of the cache returned here.
    pub fn account_target(&self, label: &str) -> Result<(CredsTarget, Cache)> {
        let active = crate::anthropic::cli_account::home_claude_json()
            .ok()
            .and_then(|path| {
                crate::anthropic::cli_account::resolve_active_label(&path, &self.all_accounts())
            });
        self.account_target_with(label, active.as_deref())
    }

    /// The pure half of [`account_target`](AnthropicConfig::account_target),
    /// with "which account the `claude` CLI is signed into" injected — the same
    /// shape as `Cli::resolve_vendor_with`.
    ///
    /// When `label` *is* the live CLI login, its credential has been moved into
    /// the default slot and removed from its named slot. Reading the default
    /// one keeps exactly one live lineage, so a refresh here cannot invalidate
    /// the credential `claude` is using (or the other way round). The cache directory
    /// is unchanged either way, so the tab keeps its identity and its cached
    /// usage across a switch.
    pub fn account_target_with(
        &self,
        label: &str,
        cli_active: Option<&str>,
    ) -> Result<(CredsTarget, Cache)> {
        let account = self.account(label)?;
        let cache = Cache::for_vendor_account("anthropic", label)?;
        if cli_active == Some(label) {
            return Ok((
                CredsTarget::Default(crate::anthropic::creds::default_path()?),
                cache,
            ));
        }
        Ok((
            CredsTarget::Named {
                config_dir: account.config_dir(),
                path: account.credentials_path,
            },
            cache,
        ))
    }
}

/// The label doubles as a cache subdirectory name
/// (`~/.cache/ai-usagebar-omarchy/anthropic/<label>/`), which nests inside the default
/// account's cache dir — so path separators, control characters, or reserved
/// cache sidecar names would escape, spoof terminal output, or collide with the
/// cache layout (`usage.json`, `.stale`, …).
pub fn validate_account_label(label: &str) -> Result<()> {
    validate_account_label_for("anthropic", label)
}

pub(crate) fn validate_account_label_for(vendor: &str, label: &str) -> Result<()> {
    const RESERVED: [&str; 4] = ["usage.json", ".stale", ".last_error", ".fetch.lock"];
    let bad = label.is_empty()
        || label == "."
        || label == ".."
        || label.contains(['/', '\\'])
        || label.contains(':')
        || label.chars().any(char::is_control)
        || RESERVED.contains(&label);
    if bad {
        return Err(AppError::Credentials(format!(
            "invalid {vendor} account label {label:?}: must be a non-empty name \
             without path separators, drive prefixes, control characters, or reserved cache names"
        )));
    }
    Ok(())
}

/// Discover accounts under `accounts_dir` in the `CLAUDE_CONFIG_DIR` layout:
/// each immediate subdirectory becomes an account labeled by the subdirectory
/// name. Best-effort: an unreadable directory or unusable label is skipped
/// silently rather than failing the whole config — discovery is convenience,
/// while an explicit `[[anthropic.accounts]]` entry stays authoritative. The
/// fetch path resolves credentials from either `.credentials.json` or the macOS
/// Keychain. Sorted by label so the tab order is stable across runs.
fn discover_accounts(accounts_dir: &std::path::Path) -> Vec<AnthropicAccount> {
    let Ok(entries) = std::fs::read_dir(accounts_dir) else {
        return Vec::new();
    };
    let mut found: Vec<AnthropicAccount> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let label = path.file_name()?.to_str()?.to_string();
            validate_account_label(&label).ok()?;
            Some(AnthropicAccount {
                label,
                credentials_path: path.join(".credentials.json"),
            })
        })
        .collect();
    found.sort_by(|a, b| a.label.cmp(&b.label));
    found
}

/// Render a path with `$HOME` collapsed back to `~`, matching the style the docs
/// and existing `[[anthropic.accounts]]` entries use. Pure so it's testable;
/// paths outside home are returned verbatim.
pub fn tildify(path: &Path, home: &Path) -> String {
    path.strip_prefix(home)
        .map(|rest| {
            let rendered = rest.display().to_string();
            // Config paths use the same portable `~/...` spelling on every
            // platform. A Windows `~\...` would not be expanded by the loader.
            #[cfg(windows)]
            let rendered = rendered.replace('\\', "/");
            format!("~/{rendered}")
        })
        .unwrap_or_else(|_| path.display().to_string())
}

/// Where a newly-registered account's credentials file lives by default: next
/// to `config.toml`, under `accounts/<label>/.credentials.json`. Returns the
/// absolute path (for `mkdir`) — tilde-render it with [`tildify`] for display
/// and for the value written into config.
pub fn default_account_credentials_path(config_path: &Path, label: &str) -> PathBuf {
    let base = config_path.parent().unwrap_or_else(|| Path::new("."));
    base.join("accounts").join(label).join(".credentials.json")
}

/// Append a `[[anthropic.accounts]]` entry to a parsed config document, in
/// place. Pure over a `toml_edit` document so the validation, duplicate check,
/// and formatting are testable without disk. Preserves the rest of the file
/// (comments, key order, other sections) — only the new array-of-tables entry
/// is added. Errors on an invalid label or a label that already exists.
pub fn add_anthropic_account_to_doc(
    doc: &mut toml_edit::DocumentMut,
    label: &str,
    credentials_path: &str,
) -> Result<()> {
    use toml_edit::{Item, Table, value};

    validate_account_label(label)?;

    let anthropic = doc
        .entry("anthropic")
        .or_insert_with(|| Item::Table(Table::new()));
    let anthropic = anthropic
        .as_table_mut()
        .ok_or_else(|| AppError::Other("[anthropic] in config.toml is not a table".into()))?;

    let accounts = anthropic
        .entry("accounts")
        .or_insert_with(|| Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));
    let accounts = accounts.as_array_of_tables_mut().ok_or_else(|| {
        AppError::Other("[[anthropic.accounts]] in config.toml is not an array of tables".into())
    })?;

    let exists = accounts
        .iter()
        .any(|t| t.get("label").and_then(Item::as_str) == Some(label));
    if exists {
        return Err(AppError::Credentials(format!(
            "anthropic account {label:?} already exists in config.toml"
        )));
    }

    let mut table = Table::new();
    table["label"] = value(label);
    table["credentials_path"] = value(credentials_path);
    accounts.push(table);
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct OpenAiConfig {
    pub enabled: bool,
    /// Override the Codex auth file path (defaults to `~/.codex/auth.json`).
    pub codex_auth_path: Option<PathBuf>,
    /// Extra Codex logins, each its own `auth.json`. Same shape as
    /// [`AnthropicAccount`] and for the same reason: Codex is an OAuth vendor,
    /// so an account *is* a credential file, and `openai::creds::write_back`
    /// refreshes into whichever one it read.
    #[serde(default)]
    pub accounts: Vec<OpenAiAccount>,
    /// Reserved, and inert: names the env var an API-key-only path *would*
    /// read (admin key → `/v1/organization/costs`). Nothing consumes it —
    /// OpenAI usage comes solely from Codex OAuth. Kept because that path is
    /// still intended, not for back-compat: `[openai]` doesn't deny unknown
    /// fields, so an existing `admin_key_env` would load either way. See
    /// `config.example.toml`, which ships it commented out so nobody sets it
    /// expecting an effect.
    pub admin_key_env: String,
}

/// One extra Codex login.
///
/// ```toml
/// [[openai.accounts]]
/// label = "work"
/// codex_auth_path = "~/.config/ai-usagebar-omarchy/accounts/work-codex/auth.json"
/// ```
///
/// A second login is made with `CODEX_HOME=~/.codex-work codex login`; point
/// `codex_auth_path` at the `auth.json` it writes.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct OpenAiAccount {
    /// Stable name used on the CLI (`--account <label>`) and as the cache
    /// subdir (`~/.cache/ai-usagebar-omarchy/openai/<label>`).
    pub label: String,
    /// Codex OAuth file for this account. Refreshed tokens are written back
    /// here, so each account keeps itself alive independently.
    pub codex_auth_path: PathBuf,
}

impl OpenAiConfig {
    /// The auth file for `label`, or the singular/default one when `label` is
    /// `None`. An unknown label is an error rather than a silent fall back to
    /// the default account, which would report the wrong login's usage.
    pub fn resolve_auth_path(&self, label: Option<&str>) -> Result<PathBuf> {
        let Some(label) = label else {
            return match &self.codex_auth_path {
                Some(path) => Ok(path.clone()),
                None => crate::openai::creds::default_path(),
            };
        };
        self.accounts
            .iter()
            .find(|account| account.label == label)
            .map(|account| account.codex_auth_path.clone())
            .ok_or_else(|| {
                AppError::Credentials(format!(
                    "no OpenAI account named {label:?}. Add it under \
                     [[openai.accounts]], or drop --account to use the default login."
                ))
            })
    }
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            codex_auth_path: None,
            accounts: Vec::new(),
            admin_key_env: "OPENAI_ADMIN_KEY".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct OpenCodeGoConfig {
    pub enabled: bool,
    pub api_key_env: String,
    pub api_key: Option<String>,
    /// Extra keys beyond the default one (`[[vendor.accounts]]`): each entry
    /// gets its own tab, tile, and isolated cache, auto-named by its
    /// position ("1", "2", …). The default key keeps the singular fields.
    pub accounts: Vec<KeyAccount>,
    /// Whether aggregate views include the default key when numbered
    /// accounts exist. Ignored when `accounts` is empty.
    pub show_default_account: bool,
}

impl Default for OpenCodeGoConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key_env: "OPENCODE_GO_API_KEY".to_string(),
            api_key: None,
            accounts: Vec::new(),
            show_default_account: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ZaiConfig {
    pub enabled: bool,
    /// Env var name to read the key from (env wins over `api_key`).
    pub api_key_env: String,
    /// Inline key (fallback when the env var is unset). Chmod 600 your
    /// config file if you put a real key here.
    pub api_key: Option<String>,
    /// Optional plan tier label (lite/pro/max) — display-only.
    pub plan_tier: Option<String>,
    /// How the default key is billed — see [`ZaiAccountType`]. Lets a single
    /// team key be configured without an accounts array.
    pub account_type: ZaiAccountType,
    /// Team plan organization/project ids (required when
    /// `account_type = "team"`). Read them from the bigmodel.cn console:
    /// F12 → Application → Local Storage → `Bigmodel-Organization` /
    /// `Bigmodel-Project`.
    pub organization_id: Option<String>,
    pub project_id: Option<String>,
    /// Which deployment the default key belongs to — see [`ZaiSite`].
    pub site: Option<ZaiSite>,
    /// Extra Z.AI / BigModel keys beyond the default one. Each entry gets a
    /// separate aggregate-view entry and cache directory.
    pub accounts: Vec<ZaiAccount>,
    /// Whether aggregate views include the default (unnamed) key when named
    /// accounts exist. Ignored when `accounts` is empty so Z.AI never loses
    /// its only tab.
    pub show_default_account: bool,
}

impl Default for ZaiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            api_key_env: "ZAI_API_KEY".to_string(),
            api_key: None,
            plan_tier: None,
            account_type: ZaiAccountType::default(),
            organization_id: None,
            project_id: None,
            site: None,
            accounts: Vec::new(),
            show_default_account: true,
        }
    }
}

/// How one Z.AI / BigModel key is billed — decides which monitor endpoint
/// (if any) answers for it. Mirrors the three account types the old GNOME
/// panel extension shipped:
///
/// | type | query | shows |
/// | --- | --- | --- |
/// | personal (default) | quota (no params) | 5h + weekly windows |
/// | team | quota `?type=2` + org/project headers | same shape, team pools |
/// | usage | skips quota, 7-day usage stats only | prompts/tokens line |
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ZaiAccountType {
    /// Personal coding-plan subscription.
    #[default]
    Personal,
    /// Team / enterprise coding plan — same quota endpoint with `?type=2`
    /// plus `bigmodel-organization` / `bigmodel-project` headers, and both
    /// ids configured. Only exists on the CN site.
    Team,
    /// Pay-as-you-go key with no coding subscription — the quota endpoint
    /// answers "not subscribed" for it, so only the 7-day usage stats are
    /// fetched.
    Usage,
}

/// Which BigModel deployment a key belongs to. Both run the same monitor
/// gateway on the same paths; only the host differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ZaiSite {
    /// Z.ai international (`api.z.ai`) — the historical default of this tool.
    Global,
    /// BigModel China (`open.bigmodel.cn`). The team/enterprise plan and
    /// pay-as-you-go coding keys only exist here.
    Cn,
}

impl ZaiSite {
    /// The site a key of `account_type` lives on when `site` is not pinned.
    /// Personal keys default to the historical Z.ai host so existing configs
    /// never move; team and usage-only keys only exist on BigModel CN.
    pub fn default_for(account_type: ZaiAccountType) -> Self {
        match account_type {
            ZaiAccountType::Personal => Self::Global,
            ZaiAccountType::Team | ZaiAccountType::Usage => Self::Cn,
        }
    }
}

/// One extra Z.AI / BigModel account. The default key continues to use the
/// singular fields under `[zai]`; each entry here adds a key with its own
/// billing type, site, and cache directory. Entries carry no label — the
/// runtime name is the entry's 1-based position in the array.
///
/// ```toml
/// [[zai.accounts]]
/// api_key_env = "BIGMODEL_TEAM_KEY"
/// account_type = "team"
/// organization_id = "org-…"
/// project_id = "proj-…"
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ZaiAccount {
    /// Runtime identity, DERIVED at load: the entry's 1-based position in
    /// `[[zai.accounts]]` ("1", "2", …) — the settings panel's top-to-bottom
    /// order. The config file no longer carries a `label`; a hand-written
    /// one is ignored (and dropped whenever the settings bridge touches the
    /// entry). Positional names keep `--account`, report ids, and cache
    /// subdirectories stable without any user configuration.
    #[serde(skip)]
    pub label: String,
    /// Optional environment variable containing this account's key.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Inline fallback when the account environment variable is unset.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Optional display-only plan tier for this account.
    #[serde(default)]
    pub plan_tier: Option<String>,
    #[serde(default)]
    pub account_type: ZaiAccountType,
    #[serde(default)]
    pub organization_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub site: Option<ZaiSite>,
}

/// Everything a fetch needs for one Z.AI key, resolved and validated from
/// either the `[zai]` section or a `[[zai.accounts]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedZaiAccount {
    pub api_key: String,
    pub account_type: ZaiAccountType,
    pub site: ZaiSite,
    pub organization_id: Option<String>,
    pub project_id: Option<String>,
    pub plan_tier: Option<String>,
}

impl ZaiConfig {
    /// Find a named account or fail loudly instead of falling back to the
    /// default key (which would show the wrong account's usage).
    pub fn account(&self, label: &str) -> Result<&ZaiAccount> {
        validate_account_label_for("zai", label)?;
        self.accounts
            .iter()
            .find(|account| account.label == label)
            .ok_or_else(|| {
                let known: Vec<&str> = self
                    .accounts
                    .iter()
                    .map(|account| account.label.as_str())
                    .collect();
                AppError::Credentials(format!(
                    "zai account {label:?} not found in [[zai.accounts]]; known labels: {known:?}"
                ))
            })
    }

    /// Resolve the default key or one named account into a validated fetch
    /// plan. Configured values are never included in an error message.
    ///
    /// A half-configured team entry (missing an id) fails loudly here rather
    /// than silently degrading to a personal query — the personal query
    /// would answer "not subscribed" for a team key and look like an error.
    pub fn resolve_account(&self, label: Option<&str>) -> Result<ResolvedZaiAccount> {
        let (api_key, account_type, organization_id, project_id, site, plan_tier) = match label {
            None => (
                resolve_api_key("Zai", &self.api_key_env, self.api_key.as_deref())?,
                self.account_type,
                self.organization_id.clone(),
                self.project_id.clone(),
                self.site,
                self.plan_tier.clone(),
            ),
            Some(label) => {
                let account = self.account(label)?;
                (
                    resolve_api_key_in_section(
                        &format!("Zai account {label:?}"),
                        "[[zai.accounts]]",
                        account.api_key_env.as_deref().unwrap_or(""),
                        account.api_key.as_deref(),
                    )?,
                    account.account_type,
                    account.organization_id.clone(),
                    account.project_id.clone(),
                    account.site,
                    account.plan_tier.clone(),
                )
            }
        };
        if account_type == ZaiAccountType::Team {
            let (organization_id, project_id) = match (&organization_id, &project_id) {
                (Some(org), Some(project)) if !org.is_empty() && !project.is_empty() => {
                    (organization_id, project_id)
                }
                _ => {
                    return Err(AppError::Credentials(
                        "zai: a team account needs both `organization_id` and `project_id` \
                         (bigmodel.cn console → F12 → Application → Local Storage → \
                         `Bigmodel-Organization` / `Bigmodel-Project`)"
                            .into(),
                    ));
                }
            };
            return Ok(ResolvedZaiAccount {
                api_key,
                account_type,
                site: site.unwrap_or_else(|| ZaiSite::default_for(account_type)),
                organization_id,
                project_id,
                plan_tier,
            });
        }
        Ok(ResolvedZaiAccount {
            api_key,
            account_type,
            site: site.unwrap_or_else(|| ZaiSite::default_for(account_type)),
            organization_id,
            project_id,
            plan_tier,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct OpenRouterConfig {
    pub enabled: bool,
    /// Extra OpenRouter accounts beyond the default key. Each account gets a
    /// separate aggregate-view entry and cache directory.
    pub accounts: Vec<OpenRouterAccount>,
    /// Whether aggregate views include the default (unnamed) key when named
    /// accounts exist. Ignored when `accounts` is empty so OpenRouter never
    /// loses its only tab.
    pub show_default_account: bool,
    pub api_key_env: String,
    pub api_key: Option<String>,
}

impl Default for OpenRouterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            accounts: Vec::new(),
            show_default_account: true,
            api_key_env: "OPENROUTER_API_KEY".to_string(),
            api_key: None,
        }
    }
}

impl OpenRouterConfig {
    /// Find a named account or fail loudly instead of falling back to the
    /// default key (which would show the wrong account's usage).
    pub fn account(&self, label: &str) -> Result<&OpenRouterAccount> {
        validate_account_label_for("openrouter", label)?;
        self.accounts
            .iter()
            .find(|account| account.label == label)
            .ok_or_else(|| {
                let known: Vec<&str> = self
                    .accounts
                    .iter()
                    .map(|account| account.label.as_str())
                    .collect();
                AppError::Credentials(format!(
                    "openrouter account {label:?} not found in [[openrouter.accounts]]; \
                     known labels: {known:?}"
                ))
            })
    }

    /// Resolve either the backward-compatible default key or one named
    /// account. Configured values are never included in an error message.
    pub fn resolve_api_key(&self, label: Option<&str>) -> Result<String> {
        match label {
            None => resolve_api_key("OpenRouter", &self.api_key_env, self.api_key.as_deref()),
            Some(label) => {
                self.account(label)?;
                resolve_key_account("OpenRouter", "openrouter", &self.accounts, label)
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DeepseekConfig {
    pub enabled: bool,
    pub api_key_env: String,
    pub api_key: Option<String>,
    /// Extra keys beyond the default one (`[[vendor.accounts]]`): each entry
    /// gets its own tab, tile, and isolated cache, auto-named by its
    /// position ("1", "2", …). The default key keeps the singular fields.
    pub accounts: Vec<KeyAccount>,
    /// Whether aggregate views include the default key when numbered
    /// accounts exist. Ignored when `accounts` is empty.
    pub show_default_account: bool,
}

impl Default for DeepseekConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key_env: "DEEPSEEK_API_KEY".to_string(),
            api_key: None,
            accounts: Vec::new(),
            show_default_account: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct KimiConfig {
    pub enabled: bool,
    pub api_key_env: String,
    /// Optional: with no key set, the vendor falls back to the Kimi Code CLI's
    /// own OAuth login, which is what a subscriber already has locally.
    pub api_key: Option<String>,
    /// Override for kimi-code's credential file (default
    /// `~/.kimi-code/credentials/kimi-code.json`), mirroring `[cursor] db_path`
    /// and `[kiro] db_path`. Useful with a relocated `KIMI_CODE_HOME`.
    pub credentials_path: Option<PathBuf>,
    /// `"auto"` follows kimi-code's own install marker (`~/.kimi-code/region`);
    /// `"cn"` pins `api.kimi.com` / `auth.kimi.com`, `"global"` pins
    /// `api.kimi.ai` / `auth.kimi.ai`. A token minted by one deployment means
    /// nothing to the other, so this picks the instance, not a currency.
    pub region: String,
    /// Extra Kimi For Coding subscriptions beyond the default one. Accounts
    /// are API-key based — the CLI OAuth login is a single account, so named
    /// entries must carry their own key (issue a key per subscription at
    /// kimi.com/code/console).
    pub accounts: Vec<KimiAccount>,
    /// Whether aggregate views include the default login/key when named
    /// accounts exist. Ignored when `accounts` is empty.
    pub show_default_account: bool,
}

impl Default for KimiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key_env: "KIMI_API_KEY".to_string(),
            api_key: None,
            credentials_path: None,
            region: "auto".to_string(),
            accounts: Vec::new(),
            show_default_account: true,
        }
    }
}

/// One extra API-key account for every key-based vendor
/// (`[[kimi.accounts]]`, `[[openrouter.accounts]]`, `[[deepseek.accounts]]`,
/// …). The runtime name is DERIVED at load: the entry's 1-based position in
/// its array ("1", "2", … — the settings panel's top-to-bottom order); the
/// config file carries no labels. `monthly_limit` exists only for the
/// spend-monitoring vendors (`anthropic_api`, `openai_api`) — each account
/// is its own organization with its own budget; the other vendors never
/// write it.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct KeyAccount {
    #[serde(skip)]
    pub label: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub monthly_limit: Option<f64>,
}

/// One named Kimi account (`[[kimi.accounts]]`). Each entry reports its own
/// weekly + 5h subscription quota with an isolated cache.
pub type KimiAccount = KeyAccount;

/// One named OpenRouter account. The default account continues to use the
/// singular `api_key_env` / `api_key` fields under `[openrouter]`.
pub type OpenRouterAccount = KeyAccount;

impl KimiConfig {
    /// Find a named account or fail loudly instead of falling back to the
    /// default login (which would show the wrong subscription's usage).
    pub fn account(&self, label: &str) -> Result<&KimiAccount> {
        validate_account_label_for("kimi", label)?;
        self.accounts
            .iter()
            .find(|account| account.label == label)
            .ok_or_else(|| {
                let known: Vec<&str> = self
                    .accounts
                    .iter()
                    .map(|account| account.label.as_str())
                    .collect();
                AppError::Credentials(format!(
                    "kimi account {label:?} not found in [[kimi.accounts]]; known labels: {known:?}"
                ))
            })
    }

    /// Resolve the API key of one named account. Accounts are key-based: the
    /// Kimi Code CLI login is a single account and cannot back a named entry.
    pub fn resolve_account_key(&self, label: &str) -> Result<String> {
        self.account(label)?;
        resolve_key_account("Kimi", "kimi", &self.accounts, label)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct KiloConfig {
    pub enabled: bool,
    pub api_key_env: String,
    pub api_key: Option<String>,
    /// Optional Kilo organization id — scopes the balance to a team via the
    /// `x-kilocode-organizationid` header. Omit for the personal balance.
    pub organization_id: Option<String>,
    /// Extra keys beyond the default one (`[[vendor.accounts]]`): each entry
    /// gets its own tab, tile, and isolated cache, auto-named by its
    /// position ("1", "2", …). The default key keeps the singular fields.
    pub accounts: Vec<KeyAccount>,
    /// Whether aggregate views include the default key when numbered
    /// accounts exist. Ignored when `accounts` is empty.
    pub show_default_account: bool,
}

impl Default for KiloConfig {
    fn default() -> Self {
        // Opt-in like DeepSeek: requires an explicit API key, so it defaults to
        // disabled and never affects existing installs.
        Self {
            enabled: false,
            api_key_env: "KILO_API_KEY".to_string(),
            api_key: None,
            organization_id: None,
            accounts: Vec::new(),
            show_default_account: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct NovitaConfig {
    pub enabled: bool,
    pub api_key_env: String,
    pub api_key: Option<String>,
    /// Extra keys beyond the default one (`[[vendor.accounts]]`): each entry
    /// gets its own tab, tile, and isolated cache, auto-named by its
    /// position ("1", "2", …). The default key keeps the singular fields.
    pub accounts: Vec<KeyAccount>,
    /// Whether aggregate views include the default key when numbered
    /// accounts exist. Ignored when `accounts` is empty.
    pub show_default_account: bool,
}

impl Default for NovitaConfig {
    fn default() -> Self {
        // Opt-in like DeepSeek/Kilo: needs an explicit API key.
        Self {
            enabled: false,
            api_key_env: "NOVITA_API_KEY".to_string(),
            api_key: None,
            accounts: Vec::new(),
            show_default_account: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MinimaxConfig {
    pub enabled: bool,
    pub api_key_env: String,
    pub api_key: Option<String>,
    /// `"global"` → api.minimax.io; `"cn"` → api.minimaxi.com. Unlike
    /// Moonshot's, this does not change the unit — MiniMax reports quota as a
    /// percentage either way. It picks the *instance*: a key issued for one
    /// host is rejected by the other (`status_code 2049`), so pointing this at
    /// the wrong region reads as an invalid key rather than an empty plan.
    pub region: String,
    /// Extra keys beyond the default one (`[[vendor.accounts]]`): each entry
    /// gets its own tab, tile, and isolated cache, auto-named by its
    /// position ("1", "2", …). The default key keeps the singular fields.
    pub accounts: Vec<KeyAccount>,
    /// Whether aggregate views include the default key when numbered
    /// accounts exist. Ignored when `accounts` is empty.
    pub show_default_account: bool,
}

impl Default for MinimaxConfig {
    fn default() -> Self {
        // Opt-in like the other API-key vendors: needs an explicit key.
        Self {
            enabled: false,
            api_key_env: "MINIMAX_API_KEY".to_string(),
            api_key: None,
            region: "global".to_string(),
            accounts: Vec::new(),
            show_default_account: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MoonshotConfig {
    pub enabled: bool,
    pub api_key_env: String,
    pub api_key: Option<String>,
    /// `"global"` → api.moonshot.ai (USD); `"cn"` → api.moonshot.cn (CNY).
    pub region: String,
    /// Extra keys beyond the default one (`[[vendor.accounts]]`): each entry
    /// gets its own tab, tile, and isolated cache, auto-named by its
    /// position ("1", "2", …). The default key keeps the singular fields.
    pub accounts: Vec<KeyAccount>,
    /// Whether aggregate views include the default key when numbered
    /// accounts exist. Ignored when `accounts` is empty.
    pub show_default_account: bool,
}

impl Default for MoonshotConfig {
    fn default() -> Self {
        // Opt-in like DeepSeek/Kilo/Novita: needs an explicit API key.
        Self {
            enabled: false,
            api_key_env: "MOONSHOT_API_KEY".to_string(),
            api_key: None,
            region: "global".to_string(),
            accounts: Vec::new(),
            show_default_account: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct GrokConfig {
    pub enabled: bool,
    /// Env var for the xAI **Management** key (distinct from the inference key).
    pub api_key_env: String,
    pub api_key: Option<String>,
    /// Optional team id. When absent, it's auto-resolved from the management
    /// key via `/auth/management-keys/validation`.
    pub team_id: Option<String>,
    /// Extra keys beyond the default one (`[[vendor.accounts]]`): each entry
    /// gets its own tab, tile, and isolated cache, auto-named by its
    /// position ("1", "2", …). The default key keeps the singular fields.
    pub accounts: Vec<KeyAccount>,
    /// Whether aggregate views include the default key when numbered
    /// accounts exist. Ignored when `accounts` is empty.
    pub show_default_account: bool,
}

impl Default for GrokConfig {
    fn default() -> Self {
        // Opt-in: needs a management key (and, for prepaid, a team).
        Self {
            enabled: false,
            api_key_env: "XAI_MANAGEMENT_KEY".to_string(),
            api_key: None,
            team_id: None,
            accounts: Vec::new(),
            show_default_account: true,
        }
    }
}

/// SuperGrok subscription auth — no API key. Asks the official Grok Build
/// CLI for billing through its `x.ai/billing` ACP extension, leaving every
/// credential, issuer, proxy, and token-rotation decision inside Grok Build.
///
/// Opt-in like Cursor/Kiro (`enabled` defaults to `false`): it requires a
/// separate official executable and signed-in session, so it stays off until
/// the user explicitly turns it on.
/// GitHub Copilot. Reports the premium-request pool the editor extensions
/// meter, read from the same `copilot_internal/user` endpoint they use.
///
/// No token is minted here: one that already exists is reused, in the order the
/// official Copilot CLI checks — `COPILOT_GITHUB_TOKEN`, `GH_TOKEN`,
/// `GITHUB_TOKEN`, then `gh`'s own credential store.
///
/// Opt-in like Cursor/Kiro (`enabled` defaults to `false`): it needs a
/// signed-in `gh` or an exported token, so it stays off until the user turns
/// it on.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CopilotConfig {
    pub enabled: bool,
    /// Inline token, lowest-friction but highest-exposure. Prefer `gh auth
    /// login` or an environment variable; set here, it gets the same 0600
    /// treatment as every other inline key in this file.
    pub token: Option<String>,
    /// `gh` executable, consulted only when no token is configured or exported.
    /// Unlike the pinned vendor CLIs above, `gh` has no single canonical
    /// install path — distro package, Homebrew, and version managers all
    /// differ — so PATH is the only portable default. Pin an absolute path to
    /// avoid resolving a `gh` that happens to come earlier on PATH.
    pub gh_binary: PathBuf,
}

impl Default for CopilotConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token: None,
            gh_binary: PathBuf::from("gh"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SuperGrokConfig {
    pub enabled: bool,
    /// Trusted official Grok Build executable. Defaults to its canonical
    /// `$GROK_HOME/bin/grok` (or `~/.grok/bin/grok`) installation path instead
    /// of searching PATH, where unrelated programs can share the name.
    pub grok_binary: PathBuf,
    /// Opaque auth/config files used only to fingerprint the active cache
    /// scope. Their contents are never parsed or copied to the cache.
    pub auth_path: Option<PathBuf>,
    pub config_path: Option<PathBuf>,
}

impl Default for SuperGrokConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            grok_binary: default_grok_binary(),
            auth_path: None,
            config_path: None,
        }
    }
}

fn default_grok_binary() -> PathBuf {
    let executable = if cfg!(windows) { "grok.exe" } else { "grok" };
    let grok_home = std::env::var_os("GROK_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::cache::home_dir().ok().map(|home| home.join(".grok")));
    grok_home
        .map(|home| home.join("bin").join(executable))
        .unwrap_or_else(|| PathBuf::from(executable))
}

/// Antigravity reads its quota from whichever local Antigravity product is
/// running, so it needs no credentials — only an on/off switch.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct AntigravityConfig {
    pub enabled: bool,
}

/// Cursor reads its quota through a session token the Cursor IDE already
/// wrote to its local `state.vscdb` — no API key, but (unlike Antigravity)
/// there is a real on-disk path that can need overriding (e.g. a portable or
/// non-default Cursor install), mirroring `openai.codex_auth_path`.
///
/// Opt-in like DeepSeek/Kilo/etc (`enabled` defaults to `false`, matching
/// `bool::default()`): reads an undocumented endpoint via a session token
/// scraped from a local IDE file, so it stays off until the user explicitly
/// turns it on.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct CursorConfig {
    pub enabled: bool,
    /// Override Cursor's local state database path (defaults to the
    /// platform-standard `.../User/globalStorage/state.vscdb` — see
    /// `cursor::db::default_db_path`).
    pub db_path: Option<PathBuf>,
    /// Override the headless `cursor-agent` CLI's own login file (defaults to
    /// `.../cursor/auth.json` — see `cursor::db::default_agent_auth_path`).
    /// Used as a fallback when `db_path` doesn't exist, so a text-only
    /// machine that never runs the desktop IDE still gets usage.
    pub agent_auth_path: Option<PathBuf>,
}

/// Kiro CLI reads its quota through the AWS SSO OIDC session kiro-cli already
/// wrote to its own local `data.sqlite3` — no API key, but (like Cursor) a
/// real on-disk path that can need overriding.
///
/// Opt-in like Cursor/DeepSeek/Kilo/etc (`enabled` defaults to `false`):
/// calls a reverse-engineered CodeWhisperer endpoint via a session token
/// scraped from a local CLI database, so it stays off until the user
/// explicitly turns it on.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct KiroConfig {
    pub enabled: bool,
    /// Override kiro-cli's local database path (defaults to the
    /// platform-standard `.../kiro-cli/data.sqlite3` — see
    /// `kiro::db::default_db_path`).
    pub db_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AnthropicApiConfig {
    pub enabled: bool,
    /// Env var for the Console **Admin key** (`sk-ant-admin01-…`), distinct from
    /// an inference key and from the Claude Code OAuth login.
    pub api_key_env: String,
    pub api_key: Option<String>,
    /// Monthly USD spend limit, used only for the spend-vs-limit % display. The
    /// API exposes neither this limit nor the remaining prepaid balance.
    pub monthly_limit: Option<f64>,
    /// Extra keys beyond the default one (`[[vendor.accounts]]`): each entry
    /// gets its own tab, tile, and isolated cache, auto-named by its
    /// position ("1", "2", …). The default key keeps the singular fields.
    pub accounts: Vec<KeyAccount>,
    /// Whether aggregate views include the default key when numbered
    /// accounts exist. Ignored when `accounts` is empty.
    pub show_default_account: bool,
}

impl Default for AnthropicApiConfig {
    fn default() -> Self {
        // Opt-in: needs an explicit Admin key.
        Self {
            enabled: false,
            api_key_env: "ANTHROPIC_ADMIN_KEY".to_string(),
            api_key: None,
            monthly_limit: None,
            accounts: Vec::new(),
            show_default_account: true,
        }
    }
}

/// OpenAI Admin API — trailing-30-day spend from the Costs API over a
/// platform **Admin key** (regular project `sk-` keys are rejected by the
/// organization endpoints). There is no balance endpoint: spend against an
/// optional monthly limit is the whole display, mirroring
/// [`AnthropicApiConfig`].
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct OpenaiApiConfig {
    pub enabled: bool,
    /// Env var for the platform **Admin key** (organization settings).
    pub api_key_env: String,
    pub api_key: Option<String>,
    /// Monthly USD spend limit, used only for the spend-vs-limit % display.
    pub monthly_limit: Option<f64>,
    pub accounts: Vec<KeyAccount>,
    pub show_default_account: bool,
}

impl Default for OpenaiApiConfig {
    fn default() -> Self {
        // Opt-in: needs an explicit Admin key.
        Self {
            enabled: false,
            api_key_env: "OPENAI_ADMIN_KEY".to_string(),
            api_key: None,
            monthly_limit: None,
            accounts: Vec::new(),
            show_default_account: true,
        }
    }
}

/// Resolve an API key for a vendor: a valid env-var name wins, then inline
/// config, then a clear error naming both fields. Used by every API-key vendor.
pub fn resolve_api_key(
    vendor_label: &str,
    env_var_name: &str,
    inline: Option<&str>,
) -> crate::error::Result<String> {
    let section = match vendor_label {
        "OpenCode Go" => "[opencode-go]".to_string(),
        _ => format!("[{}]", vendor_label.to_lowercase()),
    };
    resolve_api_key_in_section(vendor_label, &section, env_var_name, inline)
}

/// The env-then-inline lookup without the "or fail" ending, for vendors where
/// an absent API key is a legitimate state rather than an error — Kimi accepts
/// a Kimi Code CLI subscription login instead.
pub fn optional_api_key(env_var_name: &str, inline: Option<&str>) -> Option<String> {
    if is_valid_env_var_name(env_var_name)
        && let Ok(v) = std::env::var(env_var_name)
        && !v.is_empty()
    {
        return Some(v);
    }
    inline.filter(|v| !v.is_empty()).map(str::to_string)
}

/// Resolve the key of one numbered `[[vendor.accounts]]` entry: its env var
/// wins, the inline key falls back — exactly the semantics of every vendor's
/// default section, so all key vendors behave identically. `accounts` must
/// be label-derived (post `derive_account_labels`).
pub(crate) fn resolve_key_account(
    vendor_display: &str,
    section: &str,
    accounts: &[KeyAccount],
    account: &str,
) -> Result<String> {
    let entry = accounts
        .iter()
        .find(|candidate| candidate.label == account)
        .ok_or_else(|| {
            AppError::Credentials(format!(
                "{vendor_display} account {account:?} not found in [[{section}.accounts]]"
            ))
        })?;
    resolve_api_key_in_section(
        &format!("{vendor_display} account {account:?}"),
        &format!("[[{section}.accounts]]"),
        entry.api_key_env.as_deref().unwrap_or(""),
        entry.api_key.as_deref(),
    )
}

fn resolve_api_key_in_section(
    vendor_label: &str,
    section: &str,
    env_var_name: &str,
    inline: Option<&str>,
) -> crate::error::Result<String> {    if let Some(key) = optional_api_key(env_var_name, inline) {
        return Ok(key);
    }
    let valid_env_name = is_valid_env_var_name(env_var_name);
    let advice = if valid_env_name {
        "set an API key in a valid environment variable or set `api_key`"
    } else {
        "fix the invalid `api_key_env` with a valid environment variable name or set `api_key`"
    };
    Err(crate::error::AppError::Credentials(format!(
        "{vendor_label}: no API key. Either {advice} under {section} in {}.",
        config_path_hint()
    )))
}

fn is_valid_env_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Config {
    /// Load from `~/.config/ai-usagebar-omarchy/config.toml`. When the file doesn't
    /// exist yet, first drops the fully annotated example template there
    /// (best-effort — an unwritable config dir keeps the historical
    /// defaults-only behaviour) so a fresh install has a file-shaped place
    /// to fill in. Errors only on actual parse failures.
    pub fn load() -> Result<Self> {
        // Best-effort, idempotent: hoists a pre-rename `~/.config/ai-usagebar-omarchy`
        // (and cache dir) out of the upstream project's reach on the first
        // run after upgrading to the fork. No-op for fresh and already-
        // migrated installs.
        migrate_legacy_layout();
        let Some(path) = resolved_path() else {
            return Ok(Self::default());
        };
        match seed_example_config(&path) {
            Ok(true) => crate::diag::event(
                "config/seed",
                &format!("template written to {}", path.display()),
            ),
            Err(e) => crate::diag::event(
                "config/seed",
                &format!("failed: {}", e.user_message()),
            ),
            _ => {}
        }
        Self::load_from(&path)
    }

    pub fn load_from(path: &std::path::Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(s) => {
                let mut config: Self = toml::from_str(&s)?;
                // `~` is shell syntax, not path syntax: `PathBuf` keeps it
                // literally, so a documented `credentials_path = "~/..."`
                // silently pointed at a directory named `~`.
                config.expand_paths();
                config.derive_account_labels();
                config.validate()?;
                #[cfg(unix)]
                config.protect_inline_api_keys(path)?;
                Ok(config)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(AppError::io_at(path, e)),
        }
    }

    /// Name Z.AI and every key-based vendor's accounts by POSITION ("1",
    /// "2", … in config = settings-panel order). The config files carry no
    /// labels; these derived names are the runtime identity for `--account`,
    /// report ids, cache subdirectories, and settings addressing. Anthropic
    /// and OpenAI (Codex) accounts keep their explicit labels — theirs come
    /// from external login directories and carry meaning a position cannot.
    pub(crate) fn derive_account_labels(&mut self) {
        for (index, account) in self.zai.accounts.iter_mut().enumerate() {
            account.label = (index + 1).to_string();
        }
        let mut sections: [Vec<KeyAccount>; 11] = [
            std::mem::take(&mut self.kimi.accounts),
            std::mem::take(&mut self.openrouter.accounts),
            std::mem::take(&mut self.deepseek.accounts),
            std::mem::take(&mut self.kilo.accounts),
            std::mem::take(&mut self.novita.accounts),
            std::mem::take(&mut self.moonshot.accounts),
            std::mem::take(&mut self.grok.accounts),
            std::mem::take(&mut self.minimax.accounts),
            std::mem::take(&mut self.opencode_go.accounts),
            std::mem::take(&mut self.anthropic_api.accounts),
            std::mem::take(&mut self.openai_api.accounts),
        ];
        for accounts in &mut sections {
            for (index, account) in accounts.iter_mut().enumerate() {
                account.label = (index + 1).to_string();
            }
        }
        let [kimi, openrouter, deepseek, kilo, novita, moonshot, grok, minimax, opencode_go, anthropic_api, openai_api] =
            sections;
        self.kimi.accounts = kimi;
        self.openrouter.accounts = openrouter;
        self.deepseek.accounts = deepseek;
        self.kilo.accounts = kilo;
        self.novita.accounts = novita;
        self.moonshot.accounts = moonshot;
        self.grok.accounts = grok;
        self.minimax.accounts = minimax;
        self.opencode_go.accounts = opencode_go;
        self.anthropic_api.accounts = anthropic_api;
        self.openai_api.accounts = openai_api;
    }

    /// The numbered `[[vendor.accounts]]` entries of every key-based vendor
    /// (labels already derived). `None` for vendors whose accounts are not
    /// key-based (zai has its own shape; anthropic/openai are OAuth paths).
    pub(crate) fn key_accounts(&self, vendor: crate::vendor::VendorId) -> Option<&[KeyAccount]> {
        let accounts = match vendor {
            crate::vendor::VendorId::Kimi => &self.kimi.accounts,
            crate::vendor::VendorId::Openrouter => &self.openrouter.accounts,
            crate::vendor::VendorId::Deepseek => &self.deepseek.accounts,
            crate::vendor::VendorId::Kilo => &self.kilo.accounts,
            crate::vendor::VendorId::Novita => &self.novita.accounts,
            crate::vendor::VendorId::Moonshot => &self.moonshot.accounts,
            crate::vendor::VendorId::Grok => &self.grok.accounts,
            crate::vendor::VendorId::Minimax => &self.minimax.accounts,
            crate::vendor::VendorId::OpenCodeGo => &self.opencode_go.accounts,
            crate::vendor::VendorId::AnthropicApi => &self.anthropic_api.accounts,
            crate::vendor::VendorId::OpenaiApi => &self.openai_api.accounts,
            _ => return None,
        };
        Some(accounts)
    }

    /// Whether a vendor's aggregate views include its default key when
    /// numbered accounts exist (the kimi rule).
    pub(crate) fn key_show_default(&self, vendor: crate::vendor::VendorId) -> bool {
        match vendor {
            crate::vendor::VendorId::Kimi => self.kimi.show_default_account,
            crate::vendor::VendorId::Openrouter => self.openrouter.show_default_account,
            crate::vendor::VendorId::Deepseek => self.deepseek.show_default_account,
            crate::vendor::VendorId::Kilo => self.kilo.show_default_account,
            crate::vendor::VendorId::Novita => self.novita.show_default_account,
            crate::vendor::VendorId::Moonshot => self.moonshot.show_default_account,
            crate::vendor::VendorId::Grok => self.grok.show_default_account,
            crate::vendor::VendorId::Minimax => self.minimax.show_default_account,
            crate::vendor::VendorId::OpenCodeGo => self.opencode_go.show_default_account,
            crate::vendor::VendorId::AnthropicApi => self.anthropic_api.show_default_account,
            crate::vendor::VendorId::OpenaiApi => self.openai_api.show_default_account,
            _ => true,
        }
    }

    fn expand_paths(&mut self) {
        expand_tilde_opt(&mut self.context.projects_path);
        expand_tilde_opt(&mut self.anthropic.credentials_path);
        expand_tilde_opt(&mut self.anthropic.accounts_dir);
        expand_tilde_opt(&mut self.anthropic.desktop_profiles_dir);
        expand_tilde_opt(&mut self.openai.codex_auth_path);
        expand_tilde_opt(&mut self.cursor.db_path);
        expand_tilde_opt(&mut self.cursor.agent_auth_path);
        expand_tilde_opt(&mut self.kiro.db_path);
        expand_tilde_opt(&mut self.kimi.credentials_path);
        self.copilot.gh_binary = expand_tilde(&self.copilot.gh_binary);
        self.supergrok.grok_binary = expand_tilde(&self.supergrok.grok_binary);
        expand_tilde_opt(&mut self.supergrok.auth_path);
        expand_tilde_opt(&mut self.supergrok.config_path);
        for account in &mut self.anthropic.accounts {
            account.credentials_path = expand_tilde(&account.credentials_path);
        }
    }

    /// Explicitly enumerate every inline API-key field. Adding a new API-key
    /// vendor must add it here so its config file receives the same protection.
    #[cfg(unix)]
    fn has_inline_api_keys(&self) -> bool {
        [
            self.zai.api_key.as_deref(),
            self.openrouter.api_key.as_deref(),
            self.deepseek.api_key.as_deref(),
            self.kimi.api_key.as_deref(),
            self.kilo.api_key.as_deref(),
            self.novita.api_key.as_deref(),
            self.minimax.api_key.as_deref(),
            self.moonshot.api_key.as_deref(),
            self.grok.api_key.as_deref(),
            self.anthropic_api.api_key.as_deref(),
            self.openai_api.api_key.as_deref(),
            self.opencode_go.api_key.as_deref(),
            self.copilot.token.as_deref(),
        ]
        .into_iter()
        // Every numbered account's inline key gets the same protection as
        // its section's default key — a `[[deepseek.accounts]]` entry is as
        // much a secret as `[deepseek] api_key`. Adding a key-account vendor
        // without listing it here is how a leak slips through.
        .chain(
            [
                &self.kimi.accounts,
                &self.openrouter.accounts,
                &self.deepseek.accounts,
                &self.kilo.accounts,
                &self.novita.accounts,
                &self.moonshot.accounts,
                &self.grok.accounts,
                &self.minimax.accounts,
                &self.opencode_go.accounts,
                &self.anthropic_api.accounts,
            ]
            .into_iter()
            .flatten()
            .map(|account| account.api_key.as_deref()),
        )
        .any(|key| key.is_some_and(|key| !key.is_empty()))
    }

    #[cfg(unix)]
    fn protect_inline_api_keys(&self, path: &Path) -> Result<()> {
        if !self.has_inline_api_keys() {
            return Ok(());
        }

        let metadata = std::fs::metadata(path).map_err(|_| {
            AppError::Credentials(format!(
                "config at {} contains inline api_key values but its permissions could not be checked; fix permissions or move keys to environment variables",
                crate::display::sanitize_untrusted_path(path)
            ))
        })?;
        if inline_key_permission_decision(metadata.mode()) == InlineKeyPermissionDecision::Tighten {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|_| {
                AppError::Credentials(format!(
                    "config at {} contains inline api_key values but is group/other-readable and could not be tightened to 0600; fix permissions or move keys to environment variables",
                    crate::display::sanitize_untrusted_path(path)
                ))
            })?;
        }
        Ok(())
    }

    pub fn is_enabled(&self, id: VendorId) -> bool {
        match id {
            VendorId::Anthropic => self.anthropic.enabled,
            VendorId::AnthropicApi => self.anthropic_api.enabled,
            VendorId::Openai => self.openai.enabled,
            VendorId::Zai => self.zai.enabled,
            VendorId::Openrouter => self.openrouter.enabled,
            VendorId::Deepseek => self.deepseek.enabled,
            VendorId::Kimi => self.kimi.enabled,
            VendorId::Kilo => self.kilo.enabled,
            VendorId::Novita => self.novita.enabled,
            VendorId::Moonshot => self.moonshot.enabled,
            VendorId::Grok => self.grok.enabled,
            VendorId::Supergrok => self.supergrok.enabled,
            VendorId::Antigravity => self.antigravity.enabled,
            VendorId::Cursor => self.cursor.enabled,
            VendorId::Minimax => self.minimax.enabled,
            VendorId::Kiro => self.kiro.enabled,
            VendorId::Copilot => self.copilot.enabled,
            VendorId::OpenCodeGo => self.opencode_go.enabled,
            VendorId::OpenaiApi => self.openai_api.enabled,
        }
    }

    pub fn enabled_vendors(&self) -> Vec<VendorId> {
        VendorId::all()
            .iter()
            .copied()
            .filter(|id| self.is_enabled(*id))
            .collect()
    }

    /// Validate cross-entry constraints that serde cannot express. Account
    /// labels are both CLI selectors and TUI tab identities, so duplicates
    /// would make either destination ambiguous.
    pub fn validate(&self) -> Result<()> {
        if self.context.context_window_tokens == Some(0) {
            return Err(AppError::Other(
                "[context] context_window_tokens must be greater than zero".into(),
            ));
        }
        for (model, tokens) in &self.context.model_context_window_tokens {
            if model.trim().is_empty() {
                return Err(AppError::Other(
                    "[context] model_context_window_tokens keys must not be empty".into(),
                ));
            }
            if *tokens == 0 {
                return Err(AppError::Other(format!(
                    "[context] model_context_window_tokens entry {model:?} must be greater than zero"
                )));
            }
        }
        if let Some(limit) = self.anthropic_api.monthly_limit
            && (!limit.is_finite() || limit <= 0.0)
        {
            return Err(AppError::Other(
                "[anthropic_api] monthly_limit must be finite and greater than zero; \
                 remove it to show spend without a limit"
                    .into(),
            ));
        }
        if crate::kimi::oauth::Region::parse(&self.kimi.region).is_none()
            && !self.kimi.region.eq_ignore_ascii_case("auto")
        {
            return Err(AppError::Other(format!(
                "[kimi] region must be \"auto\", \"cn\", or \"global\", got {:?}",
                self.kimi.region
            )));
        }
        if !self.minimax.region.eq_ignore_ascii_case("global")
            && !self.minimax.region.eq_ignore_ascii_case("cn")
        {
            return Err(AppError::Other(format!(
                "[minimax] region must be \"global\" or \"cn\", got {:?}",
                self.minimax.region
            )));
        }
        if self.supergrok.grok_binary.as_os_str().is_empty() {
            return Err(AppError::Other(
                "[supergrok] grok_binary must not be empty".into(),
            ));
        }
        if self.copilot.gh_binary.as_os_str().is_empty() {
            return Err(AppError::Other(
                "[copilot] gh_binary must not be empty".into(),
            ));
        }
        let mut labels = HashSet::new();
        for account in &self.anthropic.accounts {
            validate_account_label(&account.label)?;
            if !labels.insert(&account.label) {
                return Err(AppError::Credentials(format!(
                    "duplicate anthropic account label {:?}",
                    account.label
                )));
            }
        }
        // Key-account vendors carry no label constraints (labels are
        // positional, derived at load) and no key-presence requirement at
        // load — a keyless account simply reads as the calm "not configured
        // yet" state at fetch time. Spend limits are the one hard rule:
        // they feed arithmetic that NaN/zero would poison.
        for account in &self.anthropic_api.accounts {
            if let Some(limit) = account.monthly_limit
                && (!limit.is_finite() || limit <= 0.0)
            {
                return Err(AppError::Other(format!(
                    "[anthropic_api] account {} monthly_limit must be finite and greater \
                     than zero; remove it to show spend without a limit",
                    account.label
                )));
            }
        }
        if let Some(limit) = self.openai_api.monthly_limit
            && (!limit.is_finite() || limit <= 0.0)
        {
            return Err(AppError::Other(
                "[openai_api] monthly_limit must be finite and greater than zero; \
                 remove it to show spend without a limit"
                    .into(),
            ));
        }
        for account in &self.openai_api.accounts {
            if let Some(limit) = account.monthly_limit
                && (!limit.is_finite() || limit <= 0.0)
            {
                return Err(AppError::Other(format!(
                    "[openai_api] account {} monthly_limit must be finite and greater \
                     than zero; remove it to show spend without a limit",
                    account.label
                )));
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InlineKeyPermissionDecision {
    Ok,
    Tighten,
}

#[cfg(unix)]
fn inline_key_permission_decision(mode: u32) -> InlineKeyPermissionDecision {
    if mode & 0o077 == 0 {
        InlineKeyPermissionDecision::Ok
    } else {
        InlineKeyPermissionDecision::Tighten
    }
}

pub fn default_path() -> Option<PathBuf> {
    let proj = directories::ProjectDirs::from("", "", crate::APP_DIR)?;
    Some(proj.config_dir().join("config.toml"))
}

/// Config files that predate the `ai-usagebar` → `ai-usagebar-omarchy` fork
/// rename, in precedence order. Both spellings existed in the wild: the
/// upstream ProjectDirs location (`~/Library/Application Support/ai-usagebar`
/// on macOS, `~/.config/ai-usagebar-omarchy` on Linux — and its `$XDG_CONFIG_HOME`
/// equivalent) and the Unix-conventional `~/.config/ai-usagebar-omarchy` that every
/// doc and the config example always pointed at. On Linux the two coincide,
/// which is harmless.
fn pre_rename_config_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(proj) = directories::ProjectDirs::from("", "", crate::LEGACY_APP_DIR) {
        out.push(proj.config_dir().join("config.toml"));
    }
    if let Ok(home) = crate::cache::home_dir() {
        out.push(
            home.join(".config")
                .join(crate::LEGACY_APP_DIR)
                .join("config.toml"),
        );
    }
    out
}

/// The config file actually in effect.
///
/// [`default_path`] stays canonical; a config left at a pre-rename location
/// is honored in place when the canonical one does not exist — normally only
/// briefly, because [`Config::load`] first attempts
/// [`migrate_legacy_layout`] to move the whole directory (config,
/// credentials, accounts) out of the upstream project's reach. The legacy
/// file is never *rewritten*: it may hold API keys, and relocating a secret
/// behind the user's back is not this tool's business — moving the directory
/// is the user's own rename decision, already expressed by installing this
/// fork.
pub fn resolved_path() -> Option<PathBuf> {
    let canonical = default_path();
    if let Some(p) = &canonical
        && p.exists()
    {
        return canonical;
    }
    for legacy in pre_rename_config_paths() {
        if legacy.exists() {
            return Some(legacy);
        }
    }
    canonical
}

/// One-time directory migration for the fork rename: move
/// `~/.config/ai-usagebar-omarchy` → `~/.config/ai-usagebar-omarchy` and
/// `~/.cache/ai-usagebar` → `~/.cache/ai-usagebar-omarchy` so this project
/// stops sharing state with the upstream `ai-usagebar` it forked from (two
/// installs would otherwise overwrite each other's config edits and race the
/// same cache locks with different payload schemas).
///
/// Semantics: runs only when the new directory does not exist and an old one
/// does; renames (same filesystem, instant), falling back to a recursive
/// copy that preserves permissions (0600 credentials stay 0600) when a
/// cross-filesystem home or a running sibling makes the rename fail. Nothing
/// is ever deleted and nothing existing is ever overwritten, so a concurrent
/// process that already migrated makes this a no-op. Best-effort: any
/// failure leaves the old layout in place, still read-honored by
/// [`resolved_path`].
pub fn migrate_legacy_layout() {
    if let Some(new_dir) = default_path().and_then(|p| p.parent().map(Path::to_path_buf)) {
        for old in pre_rename_config_paths() {
            let Some(old_dir) = old.parent() else { continue };
            match migrate_dir_at(old_dir, &new_dir) {
                Ok(true) => {
                    crate::diag::event(
                        "config/migrate",
                        &format!("{} -> {}", old_dir.display(), new_dir.display()),
                    );
                    break;
                }
                Err(e) => crate::diag::event(
                    "config/migrate",
                    &format!("{} failed: {}", old_dir.display(), e.user_message()),
                ),
                _ => {}
            }
        }
    }
    if let Ok(new_cache) = crate::cache::app_cache_root()
        && let Ok(old_cache) = crate::cache::legacy_app_cache_root()
    {
        let _ = migrate_dir_at(&old_cache, &new_cache);
    }
}

/// Move `old` to `new` once: `false` when `new` already exists (migrated, or
/// a fresh install's own directory — never clobber it) or `old` is absent.
/// Rename first; copy recursively as the fallback, leaving `old` in place.
fn migrate_dir_at(old: &Path, new: &Path) -> Result<bool> {
    if new.exists() || !old.exists() {
        return Ok(false);
    }
    if std::fs::rename(old, new).is_ok() {
        return Ok(true);
    }
    // Rename refused (cross-filesystem home is the usual reason; a sibling
    // process having just completed the migration shows up as `old` gone).
    if !old.exists() {
        return Ok(false);
    }
    copy_dir_recursively(old, new).map(|_| true)
}

fn copy_dir_recursively(old: &Path, new: &Path) -> Result<()> {
    std::fs::create_dir_all(new).map_err(|e| AppError::io_at(new, e))?;
    let entries = match std::fs::read_dir(old) {
        Ok(entries) => entries,
        // The source vanished mid-migration (a concurrent rename won): the
        // files that mattered already reached `new`.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(AppError::io_at(old, e)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| AppError::io_at(old, e))?;
        let from = entry.path();
        let to = new.join(entry.file_name());
        // `fs::copy` preserves permissions, so 0600 credential files and the
        // config the user already chmod'd keep their modes.
        if entry.file_type().map_err(|e| AppError::io_at(&from, e))?.is_dir() {
            copy_dir_recursively(&from, &to)?;
        } else if !to.exists() {
            std::fs::copy(&from, &to).map_err(|e| AppError::io_at(&from, e))?;
        }
    }
    Ok(())
}

/// Expand a leading `~` (or `~/`) against the user's home directory. Anything
/// else — including `~user` — is left untouched.
fn expand_tilde(p: &std::path::Path) -> PathBuf {
    let Some(s) = p.to_str() else {
        return p.to_path_buf();
    };
    let rest = if s == "~" {
        ""
    } else if let Some(r) = s.strip_prefix("~/") {
        r
    } else {
        return p.to_path_buf();
    };
    match crate::cache::home_dir() {
        Ok(home) if rest.is_empty() => home,
        Ok(home) => home.join(rest),
        Err(_) => p.to_path_buf(),
    }
}

fn expand_tilde_opt(p: &mut Option<PathBuf>) {
    if let Some(inner) = p.as_ref() {
        *p = Some(expand_tilde(inner));
    }
}

/// The annotated example configuration shipped with the source, embedded so
/// every install method (Omarchy plugin, AUR, crates.io, `cargo install`)
/// can seed a fillable template without distributing a second copy of the
/// file. Behavioral contract: it must parse to exactly [`Config::default`]
/// — a guarded by test — so seeding it changes documentation, never
/// behavior.
pub const EXAMPLE_CONFIG: &str = include_str!("../config.example.toml");

/// Write [`EXAMPLE_CONFIG`] to `path` when (and only when) nothing exists
/// there yet — a fresh install's first run turns "where do I configure
/// this?" into an annotated file to fill in. Never overwrites: the file may
/// hold the user's API keys. Created 0600 because that is what it is for.
/// Returns whether the template was written.
pub fn seed_example_config(path: &Path) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| AppError::io_at(dir, e))?;
    // `tempfile` creates 0600 and `persist` keeps the mode — start tight,
    // the template's own header still tells users to keep it that way.
    let mut tmp = tempfile::Builder::new()
        .prefix(".config.")
        .tempfile_in(dir)
        .map_err(|e| AppError::io_at(dir, e))?;
    std::io::Write::write_all(&mut tmp, EXAMPLE_CONFIG.as_bytes())
        .map_err(|e| AppError::io_at(tmp.path(), e))?;
    tmp.persist(path)
        .map_err(|e| AppError::io_at(path, e.error))?;
    Ok(true)
}

/// Resolved `config.toml` path as a string for user-facing messages. Uses the
/// platform's config dir (`directories::ProjectDirs`), so it reads correctly on
/// Linux, macOS, and Windows instead of hard-coding the Unix `~/.config` path.
/// Falls back to the bare filename if the path can't be resolved.
pub fn config_path_hint() -> String {
    resolved_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "config.toml".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[cfg(unix)]
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn write_toml(s: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(s.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    /// Z.AI and Kimi accounts are named by POSITION on load ("1", "2", … —
    /// the settings panel's top-to-bottom order); the config file carries no
    /// labels, and a legacy hand-written label is ignored. Anthropic and
    /// OpenRouter labels are external identities and stay verbatim.
    #[test]
    fn zai_and_kimi_accounts_are_named_by_position() {
        let file = write_toml(
            r#"
            [[zai.accounts]]
            label = "work"
            api_key = "k1"

            [[zai.accounts]]
            api_key = "k2"

            [[kimi.accounts]]
            label = "legacy"
            api_key = "k3"
            "#,
        );
        let cfg = Config::load_from(file.path()).unwrap();
        let zai: Vec<&str> = cfg.zai.accounts.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(zai, vec!["1", "2"], "positions replace stored labels");
        let kimi: Vec<&str> = cfg.kimi.accounts.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(kimi, vec!["1"]);

        // The derived names address accounts everywhere the runtime does.
        assert_eq!(cfg.zai.account("2").unwrap().api_key.as_deref(), Some("k2"));
        assert!(cfg.zai.account("work").is_err(), "legacy labels no longer address");
        assert!(cfg.zai.account("3").is_err(), "out-of-range position refused");
    }

    /// A label-less TOML parse (no load_from normalization) leaves labels
    /// empty — derive_account_labels is what names them, so direct parses
    /// (settings snapshot) call it too.
    #[test]
    fn derive_account_labels_numbers_only_zai_and_kimi() {
        let mut cfg: Config = toml::from_str(
            r#"
            [[zai.accounts]]
            api_key = "k1"

            [[zai.accounts]]
            api_key = "k2"

            [[anthropic.accounts]]
            label = "work"
            credentials_path = "/tmp/c.json"
            "#,
        )
        .unwrap();
        assert!(cfg.zai.accounts.iter().all(|a| a.label.is_empty()));
        cfg.derive_account_labels();
        let zai: Vec<&str> = cfg.zai.accounts.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(zai, vec!["1", "2"]);
        // Anthropic labels are external identities (login directories) —
        // never renumbered.
        assert_eq!(cfg.anthropic.accounts[0].label, "work");
    }

    /// The back-compat guarantee #134 asks for: a config with no
    /// `[[openai.accounts]]` resolves exactly what it resolved before, whether
    /// it sets `codex_auth_path` or leaves it to the default.
    #[test]
    fn openai_without_accounts_resolves_the_singular_path() {
        let explicit = OpenAiConfig {
            codex_auth_path: Some(PathBuf::from("/tmp/codex/auth.json")),
            ..OpenAiConfig::default()
        };
        assert_eq!(
            explicit.resolve_auth_path(None).unwrap(),
            PathBuf::from("/tmp/codex/auth.json")
        );

        let bare = OpenAiConfig::default();
        assert_eq!(
            bare.resolve_auth_path(None).unwrap(),
            crate::openai::creds::default_path().unwrap(),
            "no codex_auth_path must still mean ~/.codex/auth.json"
        );
    }

    /// Each named account resolves its own file, and the default login is still
    /// reachable alongside them.
    #[test]
    fn openai_named_accounts_resolve_their_own_auth_file() {
        let config: Config = toml::from_str(
            r#"
            [openai]
            codex_auth_path = "/tmp/personal/auth.json"
            [[openai.accounts]]
            label = "work"
            codex_auth_path = "/tmp/work/auth.json"
            "#,
        )
        .unwrap();

        assert_eq!(
            config.openai.resolve_auth_path(Some("work")).unwrap(),
            PathBuf::from("/tmp/work/auth.json")
        );
        assert_eq!(
            config.openai.resolve_auth_path(None).unwrap(),
            PathBuf::from("/tmp/personal/auth.json")
        );
    }

    /// An unknown label must fail rather than quietly fall back to the default
    /// login — reporting the wrong subscription's usage is worse than an error.
    #[test]
    fn an_unknown_openai_account_is_an_error_not_a_fallback() {
        let config = OpenAiConfig {
            codex_auth_path: Some(PathBuf::from("/tmp/personal/auth.json")),
            accounts: vec![OpenAiAccount {
                label: "work".into(),
                codex_auth_path: PathBuf::from("/tmp/work/auth.json"),
            }],
            ..OpenAiConfig::default()
        };
        let err = config
            .resolve_auth_path(Some("nope"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("nope"), "{err}");
        assert!(err.contains("[[openai.accounts]]"), "{err}");
    }

    #[test]
    fn defaults_enable_only_the_four_core_vendors() {
        let c = Config::default();
        assert!(c.is_enabled(VendorId::Anthropic));
        assert!(c.is_enabled(VendorId::Openai));
        assert!(c.is_enabled(VendorId::Zai));
        assert!(c.is_enabled(VendorId::Openrouter));
        for opt_in in [
            VendorId::AnthropicApi,
            VendorId::Deepseek,
            VendorId::Kimi,
            VendorId::Kilo,
            VendorId::Novita,
            VendorId::Moonshot,
            VendorId::Grok,
            VendorId::Supergrok,
            VendorId::Cursor,
            VendorId::Minimax,
            VendorId::Kiro,
        ] {
            assert!(!c.is_enabled(opt_in), "{opt_in:?}");
        }
        assert_eq!(c.enabled_vendors().len(), 4);
    }

    #[test]
    fn new_provider_defaults_are_opt_in_and_use_exact_auth_contracts() {
        let config = Config::default();
        assert!(!config.is_enabled(VendorId::OpenCodeGo));
        assert_eq!(config.opencode_go.api_key_env, "OPENCODE_GO_API_KEY");
        assert!(config.opencode_go.api_key.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn opencode_go_inline_key_is_protected_like_other_api_keys() {
        let mut config = Config::default();
        config.opencode_go.api_key = Some("<redacted>".to_string());
        assert!(config.has_inline_api_keys());
    }

    #[cfg(unix)]
    #[test]
    fn openrouter_named_inline_keys_receive_config_file_protection() {
        let mut config = Config::default();
        config.openrouter.accounts.push(OpenRouterAccount {
            label: "work".into(),
            api_key_env: None,
            api_key: Some("<redacted>".into()),
            monthly_limit: None,
        });
        assert!(config.has_inline_api_keys());
    }

    #[test]
    fn missing_file_uses_defaults() {
        let path = std::path::Path::new("/tmp/does-not-exist-ai-usagebar-test");
        let c = Config::load_from(path).unwrap();
        assert!(c.is_enabled(VendorId::Anthropic));
    }

    #[test]
    fn parses_full_config() {
        let f = write_toml(
            r#"
            [anthropic]
            enabled = true

            [openai]
            enabled = false
            admin_key_env = "MY_ADMIN_KEY"

            [zai]
            enabled = true
            api_key_env = "MY_ZAI"
            plan_tier = "pro"

            [openrouter]
            enabled = false
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        assert!(c.is_enabled(VendorId::Anthropic));
        assert!(!c.is_enabled(VendorId::Openai));
        assert!(c.is_enabled(VendorId::Zai));
        assert!(!c.is_enabled(VendorId::Openrouter));
        assert_eq!(c.openai.admin_key_env, "MY_ADMIN_KEY");
        assert_eq!(c.zai.api_key_env, "MY_ZAI");
        assert_eq!(c.zai.plan_tier.as_deref(), Some("pro"));
        assert!(c.openrouter.accounts.is_empty());
        assert!(c.openrouter.show_default_account);
    }

    #[test]
    fn partial_config_falls_back_to_defaults() {
        let f = write_toml(
            r#"[openai]
enabled = false
"#,
        );
        let c = Config::load_from(f.path()).unwrap();
        assert!(!c.is_enabled(VendorId::Openai));
        // Other vendors keep their defaults.
        assert!(c.is_enabled(VendorId::Anthropic));
        assert_eq!(c.openai.admin_key_env, "OPENAI_ADMIN_KEY");
    }

    #[test]
    fn malformed_toml_returns_error() {
        let f = write_toml("this is not = = valid");
        assert!(Config::load_from(f.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn load_from_tightens_world_readable_config_with_inline_api_key() {
        let file = write_toml("[zai]\napi_key = \"test-inline-key\"\n");
        std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o644)).unwrap();

        Config::load_from(file.path()).unwrap();

        assert_eq!(
            std::fs::metadata(file.path()).unwrap().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn load_from_leaves_world_readable_config_without_inline_api_keys_unchanged() {
        let file = write_toml("[zai]\napi_key_env = \"TEST_ZAI_API_KEY\"\n");
        std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o644)).unwrap();

        Config::load_from(file.path()).unwrap();

        assert_eq!(
            std::fs::metadata(file.path()).unwrap().mode() & 0o777,
            0o644
        );
    }

    #[cfg(unix)]
    #[test]
    fn inline_key_permission_decision_requires_tightening_for_group_or_other_bits() {
        assert_eq!(
            inline_key_permission_decision(0o600),
            InlineKeyPermissionDecision::Ok
        );
        assert_eq!(
            inline_key_permission_decision(0o640),
            InlineKeyPermissionDecision::Tighten
        );
        assert_eq!(
            inline_key_permission_decision(0o604),
            InlineKeyPermissionDecision::Tighten
        );
    }

    #[test]
    fn anthropic_api_monthly_limit_must_be_positive_and_finite() {
        for value in ["0", "-1", "inf", "nan"] {
            let file = write_toml(&format!("[anthropic_api]\nmonthly_limit = {value}\n"));
            let error = Config::load_from(file.path()).unwrap_err().to_string();
            assert!(error.contains("monthly_limit"), "value {value}: {error}");
        }

        let file = write_toml("[anthropic_api]\nmonthly_limit = 1000\n");
        assert_eq!(
            Config::load_from(file.path())
                .unwrap()
                .anthropic_api
                .monthly_limit,
            Some(1000.0)
        );
    }

    #[test]
    fn minimax_region_accepts_only_known_instances() {
        for region in ["global", "GLOBAL", "cn", "CN"] {
            let file = write_toml(&format!("[minimax]\nregion = {region:?}\n"));
            assert_eq!(
                Config::load_from(file.path()).unwrap().minimax.region,
                region
            );
        }

        for region in ["", "china", "us"] {
            let file = write_toml(&format!("[minimax]\nregion = {region:?}\n"));
            let error = Config::load_from(file.path()).unwrap_err().to_string();
            assert!(error.contains("[minimax] region"), "{error}");
        }
    }

    #[test]
    fn kimi_region_accepts_auto_and_both_deployments() {
        for region in ["auto", "AUTO", "cn", "mainland-cn", "global"] {
            let file = write_toml(&format!("[kimi]\nregion = {region:?}\n"));
            assert_eq!(Config::load_from(file.path()).unwrap().kimi.region, region);
        }

        for region in ["", "us", "oversea"] {
            let file = write_toml(&format!("[kimi]\nregion = {region:?}\n"));
            let error = Config::load_from(file.path()).unwrap_err().to_string();
            assert!(error.contains("[kimi] region"), "{error}");
        }
    }

    #[test]
    fn kimi_defaults_to_auto_region_and_no_credential_override() {
        let defaults = KimiConfig::default();
        assert_eq!(defaults.region, "auto");
        assert_eq!(defaults.credentials_path, None);
        assert!(!defaults.enabled);
    }

    #[test]
    fn kimi_credentials_path_expands_a_tilde() {
        let file = write_toml("[kimi]\ncredentials_path = \"~/kimi/creds.json\"\n");
        let path = Config::load_from(file.path())
            .unwrap()
            .kimi
            .credentials_path
            .unwrap();
        assert!(!path.starts_with("~"), "{}", path.display());
        assert!(path.ends_with("kimi/creds.json"), "{}", path.display());
    }

    #[test]
    fn optional_api_key_reports_absence_instead_of_failing() {
        assert_eq!(
            optional_api_key("KIMI_API_KEY_DEFINITELY_UNSET", Some("inline")),
            Some("inline".to_string())
        );
        assert_eq!(
            optional_api_key("KIMI_API_KEY_DEFINITELY_UNSET", None),
            None
        );
        assert_eq!(optional_api_key("KIMI_API_KEY_UNSET", Some("")), None);
        // An unusable `api_key_env` still lets an inline key through, exactly
        // as `resolve_api_key` does.
        assert_eq!(
            optional_api_key("9INVALID", Some("inline")),
            Some("inline".to_string())
        );
    }

    #[test]
    fn context_monitor_is_opt_in_and_window_sizes_are_explicit() {
        let defaults = Config::default();
        assert!(!defaults.context.enabled);
        assert_eq!(
            defaults.context.window_tokens_for(Some("claude-test")),
            None
        );

        let file = write_toml(
            r#"
            [context]
            enabled = true
            context_window_tokens = 200000

            [context.model_context_window_tokens]
            claude-opus-1m = 1000000
            "claude exact id" = 300000
            "#,
        );
        let config = Config::load_from(file.path()).unwrap();
        assert!(config.context.enabled);
        assert_eq!(
            config.context.window_tokens_for(Some("claude-opus-1m")),
            Some(1_000_000)
        );
        assert_eq!(
            config.context.window_tokens_for(Some("claude exact id")),
            Some(300_000)
        );
        assert_eq!(
            config.context.window_tokens_for(Some("another-model")),
            Some(200_000)
        );
    }

    #[test]
    fn context_layout_defaults_to_full_and_parses_each_variant() {
        assert_eq!(Config::default().context.layout, ContextLayout::Full);
        for (text, want) in [
            ("full", ContextLayout::Full),
            ("split", ContextLayout::Split),
            ("bottom", ContextLayout::Bottom),
        ] {
            let file = write_toml(&format!("[context]\nlayout = \"{text}\"\n"));
            assert_eq!(Config::load_from(file.path()).unwrap().context.layout, want);
        }
        let file = write_toml("[context]\nlayout = \"floating\"\n");
        assert!(
            Config::load_from(file.path()).is_err(),
            "an unknown layout must be rejected, not silently defaulted"
        );
    }

    #[test]
    fn vendor_box_defaults_to_sidebar_and_parses_each_variant() {
        assert_eq!(Config::default().ui.vendor_box(), VendorBoxStyle::Sidebar);
        for (text, want) in [
            ("sidebar", VendorBoxStyle::Sidebar),
            ("navbar", VendorBoxStyle::Navbar),
            ("none", VendorBoxStyle::None),
        ] {
            let file = write_toml(&format!("[ui]\nvendor_box = \"{text}\"\n"));
            assert_eq!(
                Config::load_from(file.path()).unwrap().ui.vendor_box(),
                want
            );
        }
        let file = write_toml("[ui]\nvendor_box = \"floating\"\n");
        assert!(
            Config::load_from(file.path()).is_err(),
            "an unknown vendor_box style must be rejected, not silently defaulted"
        );
    }

    #[test]
    fn context_window_sizes_must_be_nonzero_and_model_ids_nonempty() {
        for source in [
            "[context]\ncontext_window_tokens = 0\n",
            "[context.model_context_window_tokens]\nclaude = 0\n",
            "[context.model_context_window_tokens]\n\" \" = 200000\n",
        ] {
            let file = write_toml(source);
            let error = Config::load_from(file.path()).unwrap_err().to_string();
            assert!(error.contains("context"), "{error}");
        }
    }

    // serial guard for env-var manipulation tests so they don't race
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static M: std::sync::Mutex<()> = std::sync::Mutex::new(());
        M.lock().unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn resolve_api_key_prefers_env_over_inline() {
        let _g = env_guard();
        // Use a unique env var name so we don't clobber test parallelism.
        let var = "AI_USAGEBAR_TEST_ENV_WINS";
        // SAFETY: tests are single-threaded under env_guard.
        unsafe { std::env::set_var(var, "from-env") };
        let got = resolve_api_key("Zai", var, Some("from-inline")).unwrap();
        unsafe { std::env::remove_var(var) };
        assert_eq!(got, "from-env");
    }

    #[test]
    fn resolve_api_key_falls_back_to_inline() {
        let _g = env_guard();
        let var = "AI_USAGEBAR_TEST_INLINE_FALLBACK";
        unsafe { std::env::remove_var(var) };
        let got = resolve_api_key("Zai", var, Some("inline-key")).unwrap();
        assert_eq!(got, "inline-key");
    }

    #[test]
    fn resolve_api_key_errors_when_both_missing() {
        let _g = env_guard();
        let var = "AI_USAGEBAR_TEST_BOTH_MISSING";
        unsafe { std::env::remove_var(var) };
        let err = resolve_api_key("Zai", var, None).unwrap_err();
        match err {
            crate::error::AppError::Credentials(msg) => {
                assert!(
                    msg.contains("api_key"),
                    "error should suggest config field: {msg}"
                );
            }
            other => panic!("expected Credentials error, got {other:?}"),
        }
    }

    #[test]
    fn resolve_api_key_uses_exact_opencode_go_section_name() {
        let _g = env_guard();
        unsafe { std::env::remove_var("OPENCODE_GO_API_KEY") };
        let err = resolve_api_key("OpenCode Go", "OPENCODE_GO_API_KEY", None).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("[opencode-go]"),
            "wrong section hint: {message}"
        );
        assert!(
            !message.contains("[opencode go]"),
            "wrong section hint: {message}"
        );
    }

    #[test]
    fn config_path_hint_ends_with_config_toml() {
        // Platform-resolved (Linux/macOS/Windows), but always ends in the
        // config filename — the trailing segment is what messages rely on.
        assert!(config_path_hint().ends_with("config.toml"));
    }

    #[test]
    fn resolve_api_key_treats_empty_env_as_unset() {
        let _g = env_guard();
        let var = "AI_USAGEBAR_TEST_EMPTY_ENV";
        unsafe { std::env::set_var(var, "") };
        let got = resolve_api_key("OpenRouter", var, Some("inline")).unwrap();
        unsafe { std::env::remove_var(var) };
        assert_eq!(got, "inline");
    }

    #[test]
    fn resolve_api_key_rejects_invalid_env_var_name_without_leaking_it() {
        let _g = env_guard();
        // Simulates a user accidentally pasting the key into api_key_env.
        let bad = "sk-kimi-very-real-looking-pasted-secret";
        let err = resolve_api_key("Kimi", bad, None).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("invalid") && msg.contains("api_key_env"),
            "error should explain misconfiguration: {msg}"
        );
        assert!(
            !msg.contains(bad),
            "error must not echo the misconfigured value: {msg}"
        );
        assert!(msg.contains("valid environment variable name"));
        assert!(
            msg.contains("[kimi]"),
            "error should point at the lowercase TOML section: {msg}"
        );
    }

    #[test]
    fn resolve_api_key_invalid_env_name_falls_back_to_inline() {
        let _g = env_guard();
        let got = resolve_api_key("Kimi", "sk-pasted-secret", Some("inline-key")).unwrap();
        assert_eq!(got, "inline-key");
    }

    #[test]
    fn resolve_api_key_never_leaks_valid_looking_configured_env_name() {
        let _g = env_guard();
        // This is syntactically a valid environment variable name, but could
        // be a pasted secret and must not be reflected in the error.
        let pasted_secret = "sk_pasted_secret";
        unsafe { std::env::remove_var(pasted_secret) };
        let err = resolve_api_key("Kimi", pasted_secret, None).unwrap_err();
        assert!(
            !err.to_string().contains(pasted_secret),
            "error must not echo configured api_key_env values"
        );
    }

    #[test]
    fn is_valid_env_var_name_rules() {
        // Valid: alphabetic or underscore first, then alnum/underscore.
        for valid in ["KIMI_API_KEY", "_PRIVATE", "a", "Z9", "MY_ZAI_2"] {
            assert!(is_valid_env_var_name(valid), "{valid} should be valid");
        }
        // Invalid: empty, digit-first, or shell-illegal characters.
        for invalid in ["", "9LIVES", "sk-kimi", "MY KEY", "A.B", "sk/k"] {
            assert!(
                !is_valid_env_var_name(invalid),
                "{invalid} should be invalid"
            );
        }
    }

    #[test]
    fn config_parses_with_inline_api_key_and_primary() {
        let f = write_toml(
            r#"
            [ui]
            primary = "openrouter"

            [zai]
            enabled = true
            api_key_env = "MY_ZAI"
            api_key = "sk-zai-inline"

            [openrouter]
            enabled = true
            api_key = "sk-or-inline"
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        assert_eq!(c.ui.primary, Some(VendorId::Openrouter));
        assert_eq!(c.zai.api_key.as_deref(), Some("sk-zai-inline"));
        assert_eq!(c.openrouter.api_key.as_deref(), Some("sk-or-inline"));
    }

    #[test]
    fn openrouter_named_accounts_preserve_the_default_contract() {
        let f = write_toml(
            r#"
            [openrouter]
            enabled = true
            api_key_env = "AI_USAGEBAR_TEST_OR_DEFAULT"
            api_key = "default-inline"
            show_default_account = false

            [[openrouter.accounts]]
            label = "work"
            api_key_env = "OPENROUTER_WORK_API_KEY"

            [[openrouter.accounts]]
            api_key = "personal-inline"
            "#,
        );
        let _g = env_guard();
        unsafe { std::env::remove_var("AI_USAGEBAR_TEST_OR_DEFAULT") };
        let config = Config::load_from(f.path()).unwrap();
        assert!(!config.openrouter.show_default_account);
        assert_eq!(config.openrouter.accounts.len(), 2);
        assert_eq!(
            config.openrouter.resolve_api_key(None).unwrap(),
            "default-inline"
        );
        // Positional addressing: the hand-written "work" label is ignored,
        // "1"/"2" address the entries in file order.
        let missing = config
            .openrouter
            .resolve_api_key(Some("1"))
            .unwrap_err()
            .to_string();
        assert!(
            missing.contains("OpenRouter account \"1\"") && missing.contains("no API key"),
            "{missing}"
        );
        assert!(!missing.contains("personal-inline"));
        assert_eq!(
            config.openrouter.resolve_api_key(Some("2")).unwrap(),
            "personal-inline"
        );
        assert!(config.openrouter.account("work").is_err(), "legacy labels no longer address");
    }

    #[test]
    fn openrouter_named_accounts_reject_ambiguous_or_unsafe_labels() {
        // Positional naming removed every label hazard: duplicates, path
        // separators, and keyless entries all LOAD now — the position owns
        // the identity, and a keyless account simply reads as not
        // configured at fetch time (the calm state, like every vendor).
        for source in [
            r#"
            [[openrouter.accounts]]
            label = "work"
            api_key = "one"
            [[openrouter.accounts]]
            label = "work"
            api_key = "two"
            "#,
            r#"
            [[openrouter.accounts]]
            label = "../work"
            api_key = "one"
            "#,
            r#"
            [[openrouter.accounts]]
            "#,
        ] {
            let f = write_toml(source);
            let config = Config::load_from(f.path()).unwrap();
            for account in &config.openrouter.accounts {
                assert!(
                    !account.label.contains('/') && !account.label.is_empty(),
                    "derived labels are safe positions: {:?}",
                    account.label
                );
            }
        }
    }

    #[test]
    fn openrouter_unknown_account_never_falls_back_to_default_key() {
        let mut config = OpenRouterConfig {
            api_key: Some("default-secret".into()),
            ..OpenRouterConfig::default()
        };
        config.accounts.push(OpenRouterAccount {
            label: "work".into(),
            api_key_env: None,
            api_key: Some("work-secret".into()),
            monthly_limit: None,
        });
        let message = config
            .resolve_api_key(Some("missing"))
            .unwrap_err()
            .to_string();
        assert!(message.contains("missing") && message.contains("work"));
        assert!(!message.contains("default-secret"));
        assert!(!message.contains("work-secret"));
    }

    #[test]
    fn openrouter_account_key_errors_do_not_echo_configured_values() {
        let config = OpenRouterConfig {
            accounts: vec![OpenRouterAccount {
                label: "work".into(),
                api_key_env: Some("sk_pasted_secret".into()),
                api_key: None,
                monthly_limit: None,
            }],
            ..OpenRouterConfig::default()
        };
        let _g = env_guard();
        unsafe { std::env::remove_var("sk_pasted_secret") };
        let message = config
            .resolve_api_key(Some("work"))
            .unwrap_err()
            .to_string();
        assert!(message.contains("[[openrouter.accounts]]"));
        assert!(!message.contains("sk_pasted_secret"));
    }

    #[test]
    fn enabled_vendors_preserves_canonical_order() {
        // DeepSeek and Kimi are disabled by default (require explicit API key
        // config), so they are absent from the enabled list unless enabled.
        let c = Config::default();
        assert_eq!(
            c.enabled_vendors(),
            vec![
                VendorId::Anthropic,
                VendorId::Openai,
                VendorId::Zai,
                VendorId::Openrouter,
            ]
        );
    }

    #[test]
    fn deepseek_appears_when_enabled() {
        let f = write_toml(
            r#"
            [deepseek]
            enabled = true
            api_key = "sk-test"
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        assert!(c.is_enabled(VendorId::Deepseek));
        assert!(c.enabled_vendors().contains(&VendorId::Deepseek));
        assert_eq!(c.deepseek.api_key.as_deref(), Some("sk-test"));
    }

    #[test]
    fn tilde_paths_are_expanded_on_load() {
        // `PathBuf` keeps `~` literally, so the documented
        // `credentials_path = "~/..."` used to resolve to a directory named
        // `~` relative to the process's cwd.
        let f = write_toml(
            r#"
            [context]
            projects_path = "~/.claude/projects"

            [anthropic]
            credentials_path = "~/.claude/.credentials.json"

            [[anthropic.accounts]]
            label = "work"
            credentials_path = "~/work.json"
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        let home = crate::cache::home_dir().unwrap();

        assert_eq!(c.context.projects_path, Some(home.join(".claude/projects")));
        let got = c.anthropic.credentials_path.unwrap();
        assert_eq!(got, home.join(".claude/.credentials.json"));
        assert!(!got.to_string_lossy().contains('~'));
        assert_eq!(
            c.anthropic.accounts[0].credentials_path,
            home.join("work.json")
        );
    }

    #[test]
    fn absolute_and_relative_paths_are_left_alone() {
        let f = write_toml(
            r#"
            [anthropic]
            credentials_path = "/etc/creds.json"
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        assert_eq!(
            c.anthropic.credentials_path.unwrap(),
            std::path::Path::new("/etc/creds.json")
        );

        // `~user` is not ours to interpret.
        let f2 = write_toml(
            r#"
            [anthropic]
            credentials_path = "~someone/creds.json"
            "#,
        );
        let c2 = Config::load_from(f2.path()).unwrap();
        assert_eq!(
            c2.anthropic.credentials_path.unwrap(),
            std::path::Path::new("~someone/creds.json")
        );
    }

    /// The seeded template is documentation, never behavior: every explicit
    /// (uncommented) value in `config.example.toml` must equal the code
    /// default, or a fresh install with the seed would behave differently
    /// from one without a config at all.
    #[test]
    fn the_example_config_parses_to_pure_defaults() {
        let seeded: Config = toml::from_str(EXAMPLE_CONFIG)
            .expect("config.example.toml must parse");
        assert_eq!(
            serde_json::to_value(&seeded).unwrap(),
            serde_json::to_value(Config::default()).unwrap(),
            "config.example.toml drifted from Config::default()"
        );
    }

    #[test]
    fn seeding_writes_the_annotated_template_once_and_tight() {
        let td = tempfile::TempDir::new().unwrap();
        let path = td.path().join("nested").join("config.toml");

        assert!(seed_example_config(&path).unwrap(), "first call seeds");
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, EXAMPLE_CONFIG);
        assert!(content.contains("[zai]"), "template carries sections");
        assert!(
            content.contains("account_type"),
            "template documents the team fields"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "the template starts 0600");
        }

        // Second call: never rewrites — the user may have pasted keys by now.
        std::fs::write(&path, "[ui]\nprimary = \"zai\"\n").unwrap();
        assert!(!seed_example_config(&path).unwrap(), "existing file is kept");
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("primary = \"zai\""));

        // What got seeded parses.
        std::fs::remove_file(&path).unwrap();
        seed_example_config(&path).unwrap();
        Config::load_from(&path).unwrap();
    }

    #[test]
    fn resolved_path_is_the_canonical_one_and_names_the_config_file() {
        // Hermetic: only asserts the shape, never which file happens to exist
        // on the machine running the tests.
        let p = resolved_path().expect("a config path must resolve");
        assert!(p.ends_with("config.toml"));
        let mut candidates = vec![default_path().unwrap()];
        candidates.extend(pre_rename_config_paths());
        assert!(
            candidates.contains(&p),
            "resolved to an unexpected location: {}",
            p.display()
        );
        // The canonical name carries the fork's own directory, and every
        // fallback is a pre-rename spelling — the two installs never meet.
        assert!(default_path().unwrap().ends_with("ai-usagebar-omarchy/config.toml"));
        for legacy in pre_rename_config_paths() {
            assert!(legacy.ends_with("ai-usagebar/config.toml"), "{legacy:?}");
        }
    }

    /// The rename migration, hermetically: old layout moves to the new name,
    /// modes ride along, nothing is deleted or clobbered.
    #[test]
    fn migrate_moves_the_old_directory_once_and_preserves_modes() {
        let td = tempfile::TempDir::new().unwrap();
        let root = td.path();
        let old = root.join("ai-usagebar");
        let new = root.join("ai-usagebar-omarchy");
        std::fs::create_dir_all(old.join("accounts").join("work")).unwrap();
        std::fs::write(old.join("config.toml"), "[ui]\nprimary = \"zai\"\n").unwrap();
        std::fs::write(old.join("credentials.json"), "{}").unwrap();
        std::fs::write(old.join("accounts").join("work").join(".credentials.json"), "{}").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                old.join("credentials.json"),
                std::fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }

        assert!(migrate_dir_at(&old, &new).unwrap(), "first call migrates");
        assert!(new.join("config.toml").exists());
        assert!(new.join("credentials.json").exists());
        assert!(new.join("accounts/work/.credentials.json").exists());
        assert!(!old.exists(), "rename removes the old directory");
        assert_eq!(
            std::fs::read_to_string(new.join("config.toml")).unwrap(),
            "[ui]\nprimary = \"zai\"\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(new.join("credentials.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "credential mode rides along");
        }

        // Second call: the new directory exists — never touched again.
        std::fs::write(new.join("config.toml"), "[ui]\nprimary = \"kimi\"\n").unwrap();
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("config.toml"), "upstream lives here now").unwrap();
        assert!(!migrate_dir_at(&old, &new).unwrap(), "existing new is kept");
        assert!(std::fs::read_to_string(new.join("config.toml"))
            .unwrap()
            .contains("kimi"));

        // Nothing to migrate → false (no old directory).
        std::fs::remove_dir_all(&old).unwrap();
        let nowhere = root.join("nowhere");
        assert!(!migrate_dir_at(&old, &nowhere).unwrap());
        assert!(!nowhere.exists());
    }

    /// When rename is refused the copy fallback runs: files are duplicated to
    /// the new name, the old directory is left in place for the sibling that
    /// still uses it.
    #[test]
    fn migrate_copy_fallback_leaves_the_old_directory_in_place() {
        let td = tempfile::TempDir::new().unwrap();
        let old = td.path().join("ai-usagebar");
        let new = td.path().join("nest").join("ai-usagebar-omarchy");
        std::fs::create_dir_all(old.join("zai").join("team")).unwrap();
        std::fs::write(old.join("config.toml"), "[zai]\nenabled = true\n").unwrap();
        std::fs::write(old.join("zai").join("team").join("usage.json"), "{}").unwrap();

        copy_dir_recursively(&old, &new).unwrap();
        assert_eq!(
            std::fs::read_to_string(new.join("config.toml")).unwrap(),
            "[zai]\nenabled = true\n"
        );
        assert!(new.join("zai/team/usage.json").exists());
        assert!(old.exists(), "copy never deletes the source");
    }

    #[test]
    fn misspelled_section_is_rejected_not_ignored() {
        // The regression this guards: `[openrouer]` used to parse fine, leave
        // OpenRouter on its defaults, and give the user no hint at all.
        let f = write_toml(
            r#"
            [openrouer]
            enabled = true
            api_key = "sk-or-v1-typo"
            "#,
        );
        let err = Config::load_from(f.path()).unwrap_err().to_string();
        assert!(
            err.contains("openrouer"),
            "error should name the typo: {err}"
        );
    }

    #[test]
    fn invalid_toml_is_an_error_not_silent_defaults() {
        let f = write_toml("[zai\nenabled = true\n");
        assert!(Config::load_from(f.path()).is_err());
    }

    #[test]
    fn a_missing_file_is_still_just_defaults() {
        // Absence stays the legitimate "use defaults" case — only real parse
        // and I/O failures are errors.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope").join("config.toml");
        let c = Config::load_from(&missing).unwrap();
        assert!(c.is_enabled(VendorId::Anthropic));
    }

    #[test]
    fn kimi_appears_when_enabled() {
        let f = write_toml(
            r#"
            [kimi]
            enabled = true
            api_key = "sk-test"
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        assert!(c.is_enabled(VendorId::Kimi));
        assert!(c.enabled_vendors().contains(&VendorId::Kimi));
        assert_eq!(c.kimi.api_key.as_deref(), Some("sk-test"));
    }

    #[test]
    fn enabled_deepseek_and_kimi_appear_in_canonical_order_ending_with_them() {
        let f = write_toml(
            r#"
            [deepseek]
            enabled = true
            api_key = "sk-ds"

            [kimi]
            enabled = true
            api_key = "sk-kimi"
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        assert_eq!(
            c.enabled_vendors(),
            vec![
                VendorId::Anthropic,
                VendorId::Openai,
                VendorId::Zai,
                VendorId::Openrouter,
                VendorId::Deepseek,
                VendorId::Kimi,
            ]
        );
    }

    #[test]
    fn parses_anthropic_accounts_and_looks_them_up() {
        let f = write_toml(
            r#"
            [anthropic]
            enabled = true

            [[anthropic.accounts]]
            label = "personal"
            credentials_path = "/creds/personal.json"

            [[anthropic.accounts]]
            label = "work"
            credentials_path = "/creds/work.json"
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        assert_eq!(c.anthropic.accounts.len(), 2);
        let work = c.anthropic.account("work").unwrap();
        assert_eq!(work.credentials_path, PathBuf::from("/creds/work.json"));
        // A typo names the offending label and lists the known ones.
        let err = format!("{:?}", c.anthropic.account("missing").unwrap_err());
        assert!(err.contains("missing") && err.contains("work"), "{err}");
    }

    #[test]
    fn duplicate_anthropic_account_labels_are_rejected_on_load() {
        let f = write_toml(
            r#"
            [[anthropic.accounts]]
            label = "work"
            credentials_path = "/creds/work-one.json"

            [[anthropic.accounts]]
            label = "work"
            credentials_path = "/creds/work-two.json"
            "#,
        );
        let err = Config::load_from(f.path()).unwrap_err().to_string();
        assert!(
            err.contains("duplicate anthropic account label \"work\""),
            "{err}"
        );
    }

    #[test]
    fn account_label_rejects_path_like_names() {
        let cfg = AnthropicConfig::default();
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            r"a\b",
            "C:work",
            "line\nbreak",
            "tab\tname",
            "usage.json",
            ".stale",
            ".last_error",
            ".fetch.lock",
        ] {
            let err = cfg.account(bad).unwrap_err();
            assert!(
                format!("{err:?}").contains("invalid anthropic account label"),
                "{bad:?} should be rejected as a label"
            );
        }
    }

    #[test]
    fn anthropic_accounts_default_to_empty() {
        // No [[anthropic.accounts]] → the single default account, empty list,
        // nothing to migrate (issue #14, back-compat rule 1).
        assert!(Config::default().anthropic.accounts.is_empty());
        assert!(Config::default().anthropic.accounts_dir.is_none());
    }

    // --- accounts_dir: CLAUDE_CONFIG_DIR-style auto-discovery ----------------
    // All hermetic: discovery reads a TempDir, never the user's real config.

    /// Create `<root>/<label>/.credentials.json` (contents irrelevant here —
    /// discovery keys on the file existing, the fetch path parses it).
    fn seed_account_dir(root: &std::path::Path, label: &str) {
        let dir = root.join(label);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".credentials.json"), "{}").unwrap();
    }

    #[test]
    fn discovers_account_dirs_in_claude_config_dir_layout() {
        let td = tempfile::tempdir().unwrap();
        seed_account_dir(td.path(), "work");
        seed_account_dir(td.path(), "personal");
        // Keychain-backed macOS logins may not write .credentials.json; their
        // config directories are still account entries.
        std::fs::create_dir_all(td.path().join("keychain-only")).unwrap();
        // A loose file (not a dir) is ignored.
        std::fs::write(td.path().join("stray.json"), "{}").unwrap();

        let cfg = AnthropicConfig {
            accounts_dir: Some(td.path().to_path_buf()),
            ..Default::default()
        };
        let all = cfg.all_accounts();
        let labels: Vec<&str> = all.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, vec!["keychain-only", "personal", "work"]);
        assert_eq!(
            all[2].credentials_path,
            td.path().join("work").join(".credentials.json")
        );
    }

    #[test]
    fn explicit_account_wins_over_a_discovered_one_with_the_same_label() {
        let td = tempfile::tempdir().unwrap();
        seed_account_dir(td.path(), "work");
        let cfg = AnthropicConfig {
            accounts: vec![AnthropicAccount {
                label: "work".into(),
                credentials_path: "/explicit/work.json".into(),
            }],
            accounts_dir: Some(td.path().to_path_buf()),
            ..Default::default()
        };
        let all = cfg.all_accounts();
        assert_eq!(all.len(), 1, "no duplicate label");
        assert_eq!(
            all[0].credentials_path,
            std::path::Path::new("/explicit/work.json"),
            "explicit entry wins"
        );
        // A discovered account is still reachable through `account()`.
        seed_account_dir(td.path(), "other");
        assert_eq!(cfg.account("other").unwrap().label, "other");
    }

    #[test]
    fn missing_accounts_dir_is_silently_empty_not_an_error() {
        let cfg = AnthropicConfig {
            accounts_dir: Some("/nonexistent/ai-usagebar-accounts".into()),
            ..Default::default()
        };
        assert!(cfg.all_accounts().is_empty());
    }

    #[test]
    fn accounts_dir_is_tilde_expanded_on_load() {
        let f = write_toml(
            r#"
            [anthropic]
            accounts_dir = "~/.config/ai-usagebar-omarchy/accounts"
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        let home = crate::cache::home_dir().unwrap();
        assert_eq!(
            c.anthropic.accounts_dir,
            Some(home.join(".config/ai-usagebar-omarchy/accounts"))
        );
    }

    #[test]
    fn desktop_profiles_dir_is_tilde_expanded_on_load() {
        let f = write_toml(
            r#"
            [anthropic]
            desktop_profiles_dir = "~/.claude-acc/profiles"
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        let home = crate::cache::home_dir().unwrap();
        assert_eq!(
            c.anthropic.desktop_profiles_dir,
            Some(home.join(".claude-acc/profiles"))
        );
    }

    #[test]
    fn the_live_cli_account_is_read_from_the_default_credential_slot() {
        let cfg = AnthropicConfig {
            accounts: vec![
                AnthropicAccount {
                    label: "work".into(),
                    credentials_path: "/tmp/accounts/work/.credentials.json".into(),
                },
                AnthropicAccount {
                    label: "personal".into(),
                    credentials_path: "/tmp/accounts/personal/.credentials.json".into(),
                },
            ],
            ..Default::default()
        };

        let (idle, idle_cache) = cfg.account_target_with("work", Some("personal")).unwrap();
        assert!(
            matches!(&idle, CredsTarget::Named { config_dir, .. }
                if config_dir == std::path::Path::new("/tmp/accounts/work")),
            "{idle:?}"
        );

        // Same label, but it is the login `claude` itself is using: one lineage.
        let (live, live_cache) = cfg.account_target_with("work", Some("work")).unwrap();
        assert!(matches!(live, CredsTarget::Default(_)), "{live:?}");

        // The cache must not move, or a switch would silently orphan the tab's
        // usage history and show "Loading…" until the next fetch.
        assert_eq!(idle_cache.dir(), live_cache.dir());
    }

    #[test]
    fn no_live_cli_account_keeps_every_account_on_its_own_slot() {
        let cfg = AnthropicConfig {
            accounts: vec![AnthropicAccount {
                label: "work".into(),
                credentials_path: "/tmp/accounts/work/.credentials.json".into(),
            }],
            ..Default::default()
        };
        let (target, _) = cfg.account_target_with("work", None).unwrap();
        assert!(matches!(target, CredsTarget::Named { .. }), "{target:?}");
    }

    /// The shipped example, which `make install` puts in
    /// `share/ai-usagebar/config.example.toml`. Repo-relative, so this stays
    /// hermetic — it never touches the user's real config.
    fn config_example() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.example.toml")
    }

    #[test]
    fn shipped_example_parses_as_a_real_config() {
        // The example is documentation users copy verbatim, but nothing used
        // to parse it — so a renamed section or field could rot there
        // unnoticed, and `deny_unknown_fields` would reject the copy on the
        // user's machine instead of in CI.
        let c = Config::load_from(&config_example()).unwrap();
        assert!(!c.context.enabled);
        assert!(c.is_enabled(VendorId::Anthropic));
        assert!(c.is_enabled(VendorId::Openai));
        assert!(!c.is_enabled(VendorId::AnthropicApi));
        assert!(!c.is_enabled(VendorId::Deepseek));
        assert!(!c.is_enabled(VendorId::Kimi));
        assert!(!c.is_enabled(VendorId::Kilo));
        assert!(!c.is_enabled(VendorId::Novita));
        assert!(!c.is_enabled(VendorId::Moonshot));
        assert!(!c.is_enabled(VendorId::Grok));
        assert!(!c.is_enabled(VendorId::Cursor));
        assert!(!c.is_enabled(VendorId::Minimax));
    }

    #[test]
    fn shipped_example_does_not_advertise_admin_key_env_as_working() {
        // The regression: the example shipped an *uncommented*
        // `admin_key_env = "OPENAI_ADMIN_KEY"`, indistinguishable from a live
        // setting. Nothing reads it, so a user could set it, skip
        // `codex login`, and wait for usage that never arrives.
        let text = std::fs::read_to_string(config_example()).unwrap();
        let live: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| l.contains("admin_key_env") && !l.starts_with('#'))
            .collect();
        assert!(
            live.is_empty(),
            "admin_key_env must stay commented out while it is inert: {live:?}"
        );
        // Still documented, though — silently dropping it would leave users
        // who already set it with no explanation of where that job moved.
        assert!(
            text.contains("admin_key_env") && text.contains("[openai_api]"),
            "the example should keep pointing admin_key_env readers at [openai_api]"
        );
    }

    #[test]
    fn admin_key_env_is_accepted_but_changes_nothing() {
        // The field survives because the API-key-only path is still intended.
        // What has to hold today is narrower: setting it loads without error
        // and moves nothing the code actually acts on.
        let f = write_toml(
            r#"
            [openai]
            admin_key_env = "SOME_ADMIN_KEY"
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        assert_eq!(c.openai.admin_key_env, "SOME_ADMIN_KEY");
        // Nothing else moved: OpenAI still resolves through Codex OAuth only.
        let default = OpenAiConfig::default();
        assert_eq!(c.openai.enabled, default.enabled);
        assert_eq!(c.openai.codex_auth_path, default.codex_auth_path);
        assert_eq!(c.enabled_vendors(), Config::default().enabled_vendors());
    }

    #[test]
    fn config_example_documents_every_vendor_without_secrets() {
        let raw = std::fs::read_to_string(config_example()).unwrap();
        let cfg = Config::load_from(&config_example()).unwrap();
        // Every vendor the binary can dispatch needs a documented section, or
        // users have no way to discover how to turn it on.
        for id in VendorId::all() {
            let section = id.slug();
            assert!(
                raw.contains(&format!("[{section}]")),
                "config.example.toml has no [{section}] section"
            );
        }

        // The example must not ship anything enabled-by-key-only, and must not
        // carry a real secret.
        assert!(!cfg.anthropic_api.enabled && cfg.anthropic_api.api_key.is_none());
        assert!(!cfg.kilo.enabled && cfg.kilo.api_key.is_none());
        assert!(!cfg.novita.enabled && cfg.novita.api_key.is_none());
        assert!(!cfg.moonshot.enabled && cfg.moonshot.api_key.is_none());
        assert!(!cfg.grok.enabled && cfg.grok.api_key.is_none());
        assert!(!cfg.supergrok.enabled);
        assert_eq!(cfg.supergrok.grok_binary, default_grok_binary());
        assert_eq!(
            cfg.supergrok
                .grok_binary
                .file_name()
                .and_then(|p| p.to_str()),
            Some(if cfg!(windows) { "grok.exe" } else { "grok" })
        );
        assert!(cfg.supergrok.auth_path.is_none());
        assert!(cfg.supergrok.config_path.is_none());
        assert!(!cfg.cursor.enabled && cfg.cursor.db_path.is_none());
        assert!(!cfg.kiro.enabled && cfg.kiro.db_path.is_none());
    }

    #[test]
    fn supergrok_binary_must_not_be_empty() {
        let file = write_toml(
            r#"
            [supergrok]
            enabled = true
            grok_binary = ""
            "#,
        );
        let error = Config::load_from(file.path()).unwrap_err().to_string();
        assert!(error.contains("grok_binary must not be empty"));
    }

    #[test]
    fn supergrok_paths_are_tilde_expanded() {
        let file = write_toml(
            r#"
            [supergrok]
            grok_binary = "~/bin/grok"
            auth_path = "~/.grok/auth.json"
            config_path = "~/.grok/config.toml"
            "#,
        );
        let config = Config::load_from(file.path()).unwrap();
        let home = crate::cache::home_dir().unwrap();
        assert_eq!(config.supergrok.grok_binary, home.join("bin/grok"));
        assert_eq!(
            config.supergrok.auth_path,
            Some(home.join(".grok/auth.json"))
        );
        assert_eq!(
            config.supergrok.config_path,
            Some(home.join(".grok/config.toml"))
        );
    }

    #[test]
    fn kiro_db_path_is_tilde_expanded() {
        let f = write_toml(
            r#"
            [kiro]
            db_path = "~/kiro-data.sqlite3"
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        let home = crate::cache::home_dir().unwrap();
        assert_eq!(c.kiro.db_path, Some(home.join("kiro-data.sqlite3")));
    }

    #[test]
    fn kiro_appears_when_enabled() {
        let f = write_toml(
            r#"
            [kiro]
            enabled = true
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        assert!(c.is_enabled(VendorId::Kiro));
        assert!(c.enabled_vendors().contains(&VendorId::Kiro));
    }

    #[test]
    fn cursor_db_path_is_tilde_expanded() {
        let f = write_toml(
            r#"
            [cursor]
            db_path = "~/cursor-state.vscdb"
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        let home = crate::cache::home_dir().unwrap();
        assert_eq!(c.cursor.db_path, Some(home.join("cursor-state.vscdb")));
    }

    #[test]
    fn cursor_agent_auth_path_is_tilde_expanded() {
        let f = write_toml(
            r#"
            [cursor]
            agent_auth_path = "~/cursor-agent-auth.json"
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        let home = crate::cache::home_dir().unwrap();
        assert_eq!(
            c.cursor.agent_auth_path,
            Some(home.join("cursor-agent-auth.json"))
        );
    }

    #[test]
    fn cursor_appears_when_enabled() {
        let f = write_toml(
            r#"
            [cursor]
            enabled = true
            "#,
        );
        let c = Config::load_from(f.path()).unwrap();
        assert!(c.is_enabled(VendorId::Cursor));
        assert!(c.enabled_vendors().contains(&VendorId::Cursor));
    }

    #[test]
    fn add_account_appends_and_preserves_existing() {
        let mut doc: toml_edit::DocumentMut = r#"
# keep me
[anthropic]
enabled = true

[[anthropic.accounts]]
label = "personal"
credentials_path = "~/.config/ai-usagebar-omarchy/accounts/personal/.credentials.json"
"#
        .parse()
        .unwrap();
        add_anthropic_account_to_doc(
            &mut doc,
            "work",
            "~/.config/ai-usagebar-omarchy/accounts/work/.credentials.json",
        )
        .unwrap();
        let rendered = doc.to_string();
        assert!(rendered.contains("# keep me"), "comment must survive");
        // Round-trips through the real loader with both accounts intact and ordered.
        let f = write_toml(&rendered);
        let c = Config::load_from(f.path()).unwrap();
        let labels: Vec<&str> = c
            .anthropic
            .accounts
            .iter()
            .map(|a| a.label.as_str())
            .collect();
        assert_eq!(labels, vec!["personal", "work"]);
    }

    #[test]
    fn add_account_to_empty_doc_is_loadable() {
        let mut doc = toml_edit::DocumentMut::new();
        add_anthropic_account_to_doc(&mut doc, "solo", "~/x/.credentials.json").unwrap();
        let f = write_toml(&doc.to_string());
        let c = Config::load_from(f.path()).unwrap();
        assert_eq!(c.anthropic.accounts.len(), 1);
        assert_eq!(c.anthropic.accounts[0].label, "solo");
    }

    #[test]
    fn add_account_rejects_duplicate_label() {
        let mut doc: toml_edit::DocumentMut = r#"
[[anthropic.accounts]]
label = "work"
credentials_path = "~/w/.credentials.json"
"#
        .parse()
        .unwrap();
        assert!(
            add_anthropic_account_to_doc(&mut doc, "work", "~/other/.credentials.json").is_err(),
            "a duplicate label must be rejected, not appended"
        );
    }

    #[test]
    fn add_account_rejects_bad_label() {
        let mut doc = toml_edit::DocumentMut::new();
        assert!(add_anthropic_account_to_doc(&mut doc, "a/b", "~/x/.credentials.json").is_err());
        assert!(add_anthropic_account_to_doc(&mut doc, "", "~/x/.credentials.json").is_err());
    }

    #[test]
    fn tildify_collapses_home_only() {
        let home = Path::new("/Users/me");
        assert_eq!(tildify(&home.join("a/b"), home), "~/a/b");
        assert_eq!(tildify(Path::new("/etc/hosts"), home), "/etc/hosts");
    }

    #[test]
    fn default_account_credentials_path_nests_under_config_dir() {
        let cfg = Path::new("/home/u/.config/ai-usagebar/config.toml");
        assert_eq!(
            default_account_credentials_path(cfg, "work"),
            Path::new("/home/u/.config/ai-usagebar/accounts/work/.credentials.json"),
        );
    }
}
