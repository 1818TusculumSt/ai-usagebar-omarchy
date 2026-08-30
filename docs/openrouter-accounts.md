# OpenRouter multi-account guide

ai-usagebar-omarchy can report several OpenRouter keys without running
separate config or cache roots. The existing `[openrouter]` key remains the
default account; every extra key is a numbered `[[openrouter.accounts]]`
entry.

> **Every key-based vendor uses this same shape** — Z.AI, Kimi, DeepSeek,
> Kilo, Novita, Moonshot, Grok, MiniMax, OpenCode Go, Anthropic API and
> OpenAI API. This guide uses OpenRouter as the example; substitute the
> section name.

## Add extra keys

Add one entry per extra key to `~/.config/ai-usagebar-omarchy/config.toml`:

```toml
[openrouter]
enabled = true
api_key_env = "OPENROUTER_API_KEY"
# api_key = "sk-or-v1-default"

[[openrouter.accounts]]
api_key_env = "OPENROUTER_WORK_API_KEY"

[[openrouter.accounts]]
api_key = "sk-or-v1-personal"
```

An account can use `api_key_env`, an inline `api_key`, or both. The
environment variable wins when both are set. If you store any key inline,
ai-usagebar-omarchy tightens the config file to mode `0600` on Unix.

## Accounts are named by position

Entries carry **no `label`** — each is auto-named by its position in the
list (`"1"`, `"2"`, … in config order, which is the order the settings
panel groups them in). That name is the account's runtime identity
everywhere:

- `--account 2` selects the second entry
- report/tab ids read `openrouter@2`
- caches isolate under `~/.cache/ai-usagebar-omarchy/openrouter/<n>`

A hand-written `label` from an older version still parses but is ignored
(and the settings panel strips it from the file the next time it saves the
section).

## Showing and hiding the default

`show_default_account = false` under `[openrouter]` hides the default key's
tab once numbered accounts exist. A default with **no credential at all**
(no inline key, no env var) never shows next to numbered accounts — dead
nodes are skipped automatically.

## Managing accounts in the settings panel

The native Omarchy settings form groups cards per vendor, each group ending
in an **Add OpenRouter Account** button: paste a key, Apply. Add, key-clear
and remove address accounts by their position, so the card you see as
"2" is exactly `--account 2`.
