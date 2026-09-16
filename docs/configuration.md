# Configuration reference

The config file is `~/.config/ai-usagebar-omarchy/config.toml` (mode 0600).
All fields are optional. Keys are configured inline — `api_key = "…"`
directly in the section — no environment variables needed (an optional
`api_key_env` per section still lets one override the inline value).
Claude, Codex, Z.AI, and OpenRouter are enabled by default; other providers are
opt-in. The commented example shows the defaults and provider-specific
settings.

```toml
[ui]
# Which vendor the widget shows when --vendor is omitted, AND which tab
# is selected when the TUI opens. Defaults to anthropic when not set.
# Only a vendor that is enabled can be primary.
# primary = "anthropic"   # anthropic | anthropic_api | openai | openai_api
#                         # | zai | openrouter | deepseek | kimi | kilo
#                         # | novita | moonshot | grok | supergrok | antigravity
#                         # | cursor | minimax | kiro | copilot | opencode-go

[context]
enabled = false           # opt in, then press c in ai-usagebar-omarchy-tui
# projects_path = "~/.claude/projects"
# context_window_tokens = 200000  # optional fallback denominator
# [context.model_context_window_tokens]
# "claude-opus-4-6" = 1000000    # exact model id overrides the fallback

[anthropic]
enabled = true
# credentials_path = "/home/you/.claude/.credentials.json"

[anthropic_api]
enabled = true             # disabled by default; requires an organization Admin key

# monthly_limit = 1000     # optional positive, finite USD display limit
# Extra Admin keys, each with its own optional monthly_limit:
# [[anthropic_api.accounts]]
# api_key = "paste-another-admin-key"
# monthly_limit = 500

[openai_api]
enabled = true             # disabled by default; requires an organization ADMIN key
                           # (platform.openai.com → Settings → Organization;
                           #  regular project sk- keys are rejected here).
                           # Trailing-30-day spend from the Costs API — the
                           # OpenAI twin of [anthropic_api] above.

# monthly_limit = 1000     # optional positive, finite USD display limit
# [[openai_api.accounts]]
# api_key = "paste-another-admin-key"
# monthly_limit = 500

[openai]
enabled = true
# codex_auth_path = "/home/you/.codex/auth.json"

[zai]
enabled = true

# plan_tier = "lite"       # lite | pro | max — display-only
# account_type = "personal"  # personal | team | usage (see the Z.AI guide below)
# organization_id = "org-…"  # required for team; bigmodel.cn console → F12 →
# project_id = "proj-…"      # Local Storage → Bigmodel-Organization/Project
# site = "global"           # global → api.z.ai | cn → open.bigmodel.cn
#                           # (team/usage default to cn, personal to global)

# Extra Z.AI/BigModel keys — same fields as [zai] above; auto-named by
# position (1, 2, …), no label field.
# [[zai.accounts]]
# account_type = "team"
# organization_id = "org-…"
# project_id = "proj-…"

[openrouter]
enabled = true

# show_default_account = false  # hide default when named accounts exist

# [[openrouter.accounts]]
# label = "work"
# api_key = "paste-your-key-here"

[deepseek]
enabled = true             # disabled by default; enable once you add an API key
# Extra keys: [[deepseek.accounts]] with just api_key — the shared
# multi-account shape every key vendor takes.


[kimi]
enabled = true             # disabled by default; a Kimi Code CLI login is enough
# Log in with `kimi` and ai-usagebar-omarchy reads the OAuth session the CLI already
# stored, refreshing it in place when it expires — no key to create or paste.
# An API key still wins when one is set; a Kimi For Coding subscription can
# issue one at kimi.com/code/console, and a platform key works too.

# credentials_path = "~/.kimi-code/credentials/kimi-code.json"  # CLI login file
# region = "auto"          # auto follows ~/.kimi-code/region
#                          # cn -> api.kimi.com | global -> api.kimi.ai
# Extra subscriptions — API-key based (one key per subscription); the CLI
# login stays the single default account. Auto-named by position, like Z.AI.
# [[kimi.accounts]]
# api_key = "paste-your-key-here"

[minimax]
enabled = true             # disabled by default; enable once you add an API key

# region = "global"        # global -> api.minimax.io | cn -> api.minimaxi.com
# Extra keys, each its own entry everywhere: [[minimax.accounts]] with just
# api_key (auto-named 1, 2, … by position) — the shared multi-account shape
# every key vendor takes.

# --- Account-balance vendors (all opt-in) ---

[kilo]
enabled = true             # disabled by default; enable once you add an API key

# organization_id = "org_..."   # team balance; omit for the personal balance
# Extra keys, each its own entry everywhere: [[kilo.accounts]] with just
# api_key (auto-named 1, 2, … by position) — the shared multi-account shape
# every key vendor takes.

[novita]
enabled = true             # disabled by default; enable once you add an API key
# Extra keys, each its own entry everywhere: [[novita.accounts]] with just
# api_key (auto-named 1, 2, … by position) — the shared multi-account shape
# every key vendor takes.


[moonshot]
enabled = true             # disabled by default; enable once you add an API key

# region = "global"        # global → api.moonshot.ai (USD) | cn → api.moonshot.cn (CNY)
# Extra keys, each its own entry everywhere: [[moonshot.accounts]] with just
# api_key (auto-named 1, 2, … by position) — the shared multi-account shape
# every key vendor takes.

[grok]
enabled = true             # disabled by default; enable once you add an API key
# The xAI *Management* key, NOT the inference key.

# Required for organization-scoped keys; auto-resolved for team-scoped ones.
# team_id = "..."
# Extra keys, each its own entry everywhere: [[grok.accounts]] with just
# api_key (auto-named 1, 2, … by position) — the shared multi-account shape
# every key vendor takes.

[copilot]
enabled = true             # disabled by default; enable once `gh auth login` has run
# No API key. The token is resolved in the Copilot CLI's own order:
# COPILOT_GITHUB_TOKEN, GH_TOKEN, GITHUB_TOKEN, then `gh auth token`.
# `gh` has no single canonical install path, so this is a PATH lookup by
# default; pin it when more than one `gh` can appear on PATH.
# gh_binary = "/usr/bin/gh"
# Last resort; prefer `gh auth login` or an environment variable.
# token = "gho_..."

[supergrok]
enabled = true             # disabled by default; enable once you've run `grok login`
# No API key: billing comes from the official Grok Build ACP process.
# Defaults to $GROK_HOME/bin/grok or ~/.grok/bin/grok. Override only when the
# trusted official binary was installed elsewhere.
# grok_binary = "/opt/grok/bin/grok"
# Opaque cache-scope fingerprint inputs; neither file is parsed or copied.
# auth_path = "/home/you/.grok/auth.json"
# config_path = "/home/you/.grok/config.toml"

[cursor]
enabled = true             # disabled by default; enable once you've signed in to Cursor
# No API key: reads the session token the Cursor IDE already wrote to its own
# state.vscdb after you signed in there. No desktop IDE (headless machine)?
# Sign in to the cursor-agent CLI once instead — its own auth.json is the
# fallback when the IDE database is absent.
# db_path = "/home/you/.config/Cursor/User/globalStorage/state.vscdb"
# agent_auth_path = "/home/you/.config/cursor/auth.json"

[opencode-go]
enabled = true             # disabled by default; enable once you add an API key
# Extra keys: [[opencode-go.accounts]] with just api_key — the shared
# multi-account shape every key vendor takes.

[kiro]
enabled = true             # disabled by default; enable once you've run `kiro-cli login`
# No API key: reads the AWS SSO OIDC session kiro-cli already wrote to its own
# data.sqlite3 after you logged in there.
# db_path = "/home/you/.local/share/kiro-cli/data.sqlite3"
```

For more than one OpenRouter key, see the
[OpenRouter account guide](openrouter-accounts.md). The existing singular
`[openrouter]` key remains the default account and needs no migration.

For more than one Codex login, add `[[openai.accounts]]` — a label and that
login's own `auth.json`, the same shape `[[anthropic.accounts]]` uses:

```toml
[[openai.accounts]]
label = "work"
codex_auth_path = "~/.config/ai-usagebar-omarchy/accounts/work-codex/auth.json"
```

Create the second login with `CODEX_HOME=~/.codex-work codex login` and point
`codex_auth_path` at the file it writes. Select it with `--account work`; each
account caches separately under `~/.cache/ai-usagebar-omarchy/openai/<label>`. The
singular `codex_auth_path` remains the default account and needs no migration.

## Z.AI / BigModel account types

Each Z.AI key — the singular `[zai]` section or any `[[zai.accounts]]` entry —
is billed one of three ways, and `account_type` says which:

- `personal` (default): a personal coding-plan subscription. Queried on
  `api.z.ai` (or `site = "cn"` for a bigmodel.cn personal key).
- `team`: an enterprise/team coding-plan key. The team plan only exists on the
  BigModel China site, so these keys query `open.bigmodel.cn` with `?type=2`
  plus the `bigmodel-organization` / `bigmodel-project` headers. Both ids are
  required — get them from the bigmodel.cn console (F12 → Application → Local
  Storage → `Bigmodel-Organization` / `Bigmodel-Project`). The response shape
  is identical to the personal plan.
- `usage`: a pay-as-you-go key with no coding subscription. The quota endpoint
  answers "not subscribed" for it, so ai-usagebar-omarchy skips quota and reports the
  7-day aggregate (prompts · tokens) from the model-usage endpoint instead.

```toml
[[zai.accounts]]
account_type = "team"
organization_id = "org-…"
project_id = "proj-…"

[[zai.accounts]]
account_type = "usage"
```

Accounts carry no `label`: each is auto-named by its POSITION in the list
("1", "2", … — the order the settings panel shows), which doubles as the
`--account 2` selector; caches are isolated per account under
`~/.cache/ai-usagebar-omarchy/zai/<n>`. The site (z.ai / bigmodel.cn) is
auto-detected from the account type — team & usage keys query
bigmodel.cn, personal keys z.ai — so no `site` is needed.

**Every key-based vendor takes the same multi-account shape** — Z.AI,
Kimi, OpenRouter, DeepSeek, Kilo, Novita, Moonshot, Grok, MiniMax,
OpenCode Go, Anthropic API and OpenAI API:

```toml
[[deepseek.accounts]]
api_key = "paste-another-key"

[[anthropic_api.accounts]]
api_key = "paste-another-admin-key"
monthly_limit = 500      # per-account spend ceiling (spend vendors only)
```

`show_default_account = false` under a section hides its default key's
tab once numbered accounts exist. The settings panel manages all of them
as grouped cards with a per-vendor **Add … Account** button; OpenRouter's
legacy hand-written labels are ignored (positional naming owns it now).

## Seeing every account at once

Two ways to tile all accounts in one bar, the way the old GNOME panel did:

- Omarchy Quattro panel: on by default — every working account appears side
  by side, each tile led by its provider logo (`claude 42% 5.3h │ kimi 18% 1d`);
  there is no module icon in front of the tiles. Toggle **Tile every account
  in the top bar** off for the opposite minimalism: the robot icon alone,
  with the numbers one hover or click away.
- Waybar: run the widget with `--vendor all` for the same tiling, with a
  combined hover tooltip covering every account.
