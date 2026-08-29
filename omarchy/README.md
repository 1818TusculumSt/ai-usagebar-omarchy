# Omarchy Quattro plugin

This is the native Omarchy 4 frontend for ai-usagebar-omarchy. It runs inside
Quattro's long-lived Quickshell process and uses the shared Omarchy UI kit for
the bar button, keyboard-aware panel, hero, controls, typography, spacing,
colors, borders, and popup placement.

The plugin is deliberately a frontend. It executes fixed `ai-usagebar-omarchy`
commands; the Rust binary remains the only code that reads or writes
configuration, talks to providers, manages refresh locks, and writes caches.

## Install

**Quick start (two steps):**

```bash
# 1. install the CLI binaries from source
cargo install --git https://github.com/KyleLee/ai-usagebar-omarchy

# 2. install this plugin
omarchy plugin add https://github.com/KyleLee/ai-usagebar-omarchy.git --enable
```

No Rust yet? Install it from <https://rustup.rs> first — `cargo install`
builds both `ai-usagebar-omarchy` and `ai-usagebar-omarchy-tui` into
`~/.cargo/bin` (make sure that directory is on your session PATH so the
Omarchy shell can find the binary). Update later by re-running the same
command with `--force`.

Optionally hide the stock Agents widget to avoid duplicates:

```bash
omarchy plugin disable omarchy.agents
```

That's it — the bar immediately tiles every configured account. Nothing is
configured yet? Open the bar panel (click the icon) → gear (`s`) →
**ACCOUNTS** → **Add account** → pick the provider (Z.AI / Kimi) → paste the
API key (the name is pre-filled, editable) → **Apply**. Z.AI team keys also
select the `team` billing type and paste the two organization ids; everything
else (site, region) is auto-detected.

No config file needs to be written by hand, but one appears anyway: the first
run drops a fully commented template at `~/.config/ai-usagebar-omarchy/config.toml`
(mode 0600, never rewritten) — editing it and the settings form are
interchangeable.

Update the binary by re-running `cargo install --git … --force`. Update or
remove the plugin with the normal plugin commands:

```bash
omarchy plugin update ai-usagebar-omarchy
omarchy plugin remove ai-usagebar-omarchy
```

## Controls

- Bar: left-click opens the native Quattro usage panel; right-click
  intentionally launches `ai-usagebar-omarchy-tui` in a terminal; middle-click or the
  mouse wheel switches provider. The exact provider or named account is saved
  in the widget's inline `shell.json` settings and restored after shell reloads
  and sleep/unlock cycles. Right-click is not the settings shortcut.
- Bar tiles: every account that reads successfully is always shown side by
  side — the name you gave it (or the provider name for unnamed defaults),
  the 5h and weekly windows each with its own reset countdown
  (`kimi-main 42%·2h 05m 15%·2d 3h`). Each figure is its own Text object colored
  by ITS window's remaining band — green ≥50% remaining, yellow <50%,
  orange <20%, red <5% — so a green 5h figure can sit beside an orange
  weekly one; the tag always stays neutral (theme foreground, never changes
  with usage), and a critical figure also goes bold so the alert survives
  color blindness. Countdowns keep two components — minutes on the 5h
  window, hours on the weekly one. Not-yet-started windows show `0%·-` so a
  missing timer never looks like a bug. Unconfigured or broken accounts
  never tile the bar; a vertical bar shows the icon with a small dot in the
  worst band's color.
- Panel: click the gear or press `s` to open the native QML settings page;
  its one display toggle, **Show remaining instead of used** (on by
  default), flips tile figures between what is left and what is used.
  `h`/`l` or Left/Right switches provider, `j`/`k` or Up/Down scrolls, `r`,
  Enter, or Space refreshes, Tab moves to the neighboring bar panel, and Esc
  closes.
- Shell: `omarchy-shell shell summon ai-usagebar-omarchy '{}'` opens the
  panel and `omarchy-shell shell hide ai-usagebar-omarchy` closes it.

The panel keeps the last successful report visible when a refresh fails and
labels it accordingly. Provider-level stale cache responses and hard errors
are shown inline. Absolute reset timestamps are rendered as live countdowns,
so an open panel stays accurate between network refreshes.

## Settings

Open the panel and select the gear, or press `s`, for the native QML settings
form. It changes the same primary provider and API keys as the terminal
Settings overlay; both write the existing ai-usagebar-omarchy config in place, preserve
comments and unrelated fields, and retain the platform-specific config path.
Stored key values are never sent to Quattro. The shell receives presence
booleans only, and changed keys travel to the Rust config owner over stdin
rather than argv or the environment. Leave a field blank to keep its current
value, or use its clear button to remove an inline key. Saving a new key also
enables that provider, matching the terminal overlay.

**ACCOUNTS** manages every Z.AI and Kimi key as its own card — one bar tile
per account, its name the tile tag:

- **Add account** creates a draft; pick the provider (Z.AI / Kimi), the name
  is pre-filled (`Z.AI`, `Kimi`, `Z.AI 2`, …) and freely editable, the API
  key field comes last, and the card's own **Apply** saves it.
- Z.AI cards pick the **billing type** (`personal` / `team` / `usage`).
  `team` is the enterprise subscription: it queries bigmodel.cn with
  `?type=2` and asks for the two organization ids (bigmodel.cn console →
  F12 → Application → Local Storage), which only appear for team keys.
  `usage` is a pay-as-you-go key (7-day stats only). The site
  (z.ai / bigmodel.cn) is auto-detected from the type — nothing to choose.
- Kimi cards take just a name and a key; region and deployment stay
  auto-detected.
- Every card carries its own **Apply**; rename in place, reset the key, or
  delete the account from the card header. Naming the default account moves
  its key into a named account in one step.
- Misconfigured accounts show their remedy inline and never alarm the bar;
  a name collision is flagged the moment you type it.

Closing the panel (focus loss, clicking elsewhere) preserves everything you
typed plus the scroll position — reopen and continue exactly where you left
off; `Esc` or the back button discards pending secrets instead.

Existing installations need no migration: `config.toml`, environment-variable
precedence, the TUI, Waybar, macOS, and Windows behavior are unchanged. If the
plugin is updated before the `ai-usagebar-omarchy` package, the form offers the terminal
settings fallback until the binary has the native settings bridge.

The plugin's display-only options remain in `~/.config/omarchy/shell.json` and
can be changed through Omarchy's bar UI or CLI:

```bash
# Show only one entry. Use an id printed by `ai-usagebar-omarchy usage --json`.
omarchy bar set ai-usagebar-omarchy provider openai
omarchy bar set ai-usagebar-omarchy provider anthropic@work

# Empty means all configured entries, with switching in the panel.
omarchy bar set ai-usagebar-omarchy provider ''

# Numeric values need --json so shell.json stores a number.
omarchy bar set ai-usagebar-omarchy refreshIntervalSec 300 --json

# Booleans also need --json. The default is true for drop-in compatibility.
omarchy bar set ai-usagebar-omarchy showValue false --json

# Opt in to the Waybar-style provider tag. The default is false.
omarchy bar set ai-usagebar-omarchy showProvider true --json
```

The refresh interval is clamped to 30–3600 seconds. The `provider` setting
prefers an exact entry id; if there is no exact match, a base id such as
`anthropic` selects all accounts for that provider. `showValue` and
`showProvider` change only the top-bar label; neither hides report details or
changes provider fetching.

`showProvider` draws the `short_name` the Rust report ships for the selected
entry, so the codes never fork from Waybar's `{vendor_short}`: `cld 29%`,
`gpt 95%`, `agy 81%`. Every account of one provider shares that provider's
code — the panel and tooltip remain the place that tells `Claude · work` from
`Claude · personal`. With both toggles on the bar reads icon + `cld 29%`; with
`showValue` off it is the icon and `cld`. A vertical bar has room for neither
and keeps showing the icon alone. Against an `ai-usagebar-omarchy` older than the
`short_name` field the tag falls back to the entry id's provider half
(`anthropic 29%`) until the binary is updated.

## Development checks

On an Omarchy 4 machine:

```bash
omarchy plugin validate .
node omarchy/model.test.mjs
```

`qmllint` cannot resolve the `qs.*` modules that Omarchy injects at shell
runtime, so it is not a reliable standalone check for plugin entry points.

Saving files under an installed user plugin triggers Quattro's plugin hot
reload. In a source checkout, rerun `omarchy plugin validate .` after changing
the manifest or entry points.
