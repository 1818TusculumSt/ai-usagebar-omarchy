// Pure data shaping for the Omarchy Quattro widget. Keep this file free of
// QML globals so the exact report contract and selection behavior can also be
// exercised by Node in CI.

function cleanText(value, maxLength) {
  var text = value === undefined || value === null ? "" : String(value)
  // The Rust projection already strips terminal controls. This second, cheap
  // boundary keeps hand-authored/older JSON from putting controls in a
  // long-lived shell process.
  text = text.replace(/[\t\r]/g, " ")
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/g, "")
    .replace(/[\u200e\u200f\u202a-\u202e\u2066-\u2069]/g, "")
  var limit = Number(maxLength) || 2048
  if (text.length <= limit) return text
  var end = limit - 1
  var finalCodeUnit = text.charCodeAt(end - 1)
  if (finalCodeUnit >= 0xd800 && finalCodeUnit <= 0xdbff) end--
  return text.slice(0, end) + "…"
}

// Shared Omarchy components use Text.AutoText. Replace angle brackets before
// passing provider-controlled labels into those components so they can never
// be reclassified as rich text (including an image tag with a remote URL).
function autoTextSafe(value) {
  return cleanText(value, 1000)
    .replace(/[\n\u2028\u2029]/g, " ")
    .replace(/</g, "‹")
    .replace(/>/g, "›")
}

function finitePercent(value) {
  var number = Number(value)
  if (!isFinite(number)) return null
  return Math.max(0, Math.min(100, Math.round(number)))
}

function normalizeSection(raw) {
  if (!raw || typeof raw !== "object") return null
  var type = String(raw.type || "")
  if (type === "spacer") return { type: "spacer" }
  if (type === "metric") {
    var percent = finitePercent(raw.percent)
    if (percent === null) return null
    var severity = String(raw.severity || "")
    if (["low", "mid", "high", "critical"].indexOf(severity) < 0)
      severity = percent >= 90 ? "critical" : percent >= 75 ? "high" : percent >= 50 ? "mid" : "low"
    var window = raw.window === "session" || raw.window === "weekly"
      ? raw.window : null
    return {
      type: "metric",
      label: cleanText(raw.label, 160),
      percent: percent,
      value: cleanText(raw.value, 240),
      detail: cleanText(raw.detail, 1000),
      severity: severity,
      reset_at: cleanText(raw.reset_at, 80),
      window: window
    }
  }
  if (type === "text") {
    return {
      type: "text",
      label: cleanText(raw.label, 160),
      value: cleanText(raw.value, 1000)
    }
  }
  if (type === "block") {
    var body = Array.isArray(raw.body) ? raw.body : []
    var lines = []
    for (var i = 0; i < body.length && i < 24; i++) lines.push(cleanText(body[i], 1000))
    return { type: "block", label: cleanText(raw.label, 160), body: lines }
  }
  return null
}

function normalizeEntry(raw) {
  if (!raw || typeof raw !== "object") return null
  var id = cleanText(raw.id, 180).trim()
  if (id === "") return null
  var sourceSections = Array.isArray(raw.sections) ? raw.sections : []
  var sections = []
  for (var i = 0; i < sourceSections.length && i < 96; i++) {
    var section = normalizeSection(sourceSections[i])
    if (section) sections.push(section)
  }
  var error = raw.error === undefined || raw.error === null ? "" : cleanText(raw.error, 1200)
  // "unconfigured" is the routine no-key/no-login state: same remedy text,
  // calmer presentation (no alarm color, no bar alert).
  var status = "ready"
  if (error !== "" || raw.status === "error")
    status = raw.unconfigured === true ? "unconfigured" : "error"
  return {
    id: id,
    name: cleanText(raw.name, 240),
    display_name: cleanText(raw.display_name, 240),
    short_name: cleanText(raw.short_name, 24),
    plan: cleanText(raw.plan, 240),
    status: status,
    error: error,
    stale: raw.stale === true,
    fetched_at: cleanText(raw.fetched_at, 80),
    sections: sections
  }
}

function parseReport(raw) {
  try {
    var parsed = JSON.parse(String(raw || ""))
    if (!parsed || !Array.isArray(parsed.entries))
      return { ok: false, error: "The usage command returned an unsupported report.", primary: "", entries: [] }
    var entries = []
    for (var i = 0; i < parsed.entries.length && i < 64; i++) {
      var entry = normalizeEntry(parsed.entries[i])
      if (entry) entries.push(entry)
    }
    if (parsed.entries.length > 0 && entries.length === 0)
      return { ok: false, error: "The usage report did not contain a valid provider entry.", primary: "", entries: [] }
    return { ok: true, error: "", primary: cleanText(parsed.primary, 180).trim(), entries: entries }
  } catch (error) {
    return { ok: false, error: "The usage command returned invalid JSON.", primary: "", entries: [] }
  }
}

function baseProvider(id) {
  return String(id || "").split("@")[0]
}

function providerName(entry) {
  if (!entry) return "AI usage"
  // The Rust report owns canonical product names. `name` is the compatible
  // fallback for older binaries; the machine id is only a last resort.
  var title = cleanText(entry.display_name || entry.name, 240).trim()
  if (title === "") title = baseProvider(entry.id).replace(/_/g, " ") || "AI usage"
  return autoTextSafe(title)
}

// The Waybar-style provider tag. `VendorId::short_name` in Rust owns the codes
// and ships them as `short_name`; a binary older than that field has none, so
// the machine id's vendor half stands in rather than a table living here.
function providerShort(entry) {
  if (!entry) return ""
  var code = cleanText(entry.short_name, 24).trim()
  if (code === "") code = baseProvider(entry.id).replace(/_/g, "-")
  return autoTextSafe(code).trim()
}

function filteredEntries(entries, configuredProvider) {
  var list = Array.isArray(entries) ? entries : []
  var wanted = String(configuredProvider || "").trim().toLowerCase()
  if (wanted === "") return list.slice()
  var exact = list.filter(function(entry) { return String(entry.id).toLowerCase() === wanted })
  if (exact.length > 0) return exact
  return list.filter(function(entry) { return baseProvider(entry.id).toLowerCase() === wanted })
}

// Panel tabs list only agents that actually read: unconfigured and broken
// entries stay reachable through the bar's wheel cycle and the status hint,
// but they get no tab.
function readyEntries(entries) {
  var list = Array.isArray(entries) ? entries : []
  return list.filter(function(entry) { return entry.status === "ready" })
}

function selectedIndex(entries, selectedId) {
  var list = Array.isArray(entries) ? entries : []
  for (var i = 0; i < list.length; i++) if (list[i].id === selectedId) return i
  return list.length > 0 ? 0 : -1
}

function preferredEntryId(entries, primaryProvider, rememberedEntryId) {
  var list = Array.isArray(entries) ? entries : []
  if (list.length === 0) return ""
  var byId = function(id) {
    var wanted = cleanText(id, 180).trim().toLowerCase()
    for (var k = 0; k < list.length; k++)
      if (String(list[k].id).toLowerCase() === wanted) return list[k]
    return null
  }
  var entry = byId(rememberedEntryId)
  if (entry && entry.status === "ready") return entry.id
  var primary = String(primaryProvider || "").toLowerCase()
  entry = byId(primary)
  if (entry && entry.status === "ready") return entry.id
  for (var j = 0; j < list.length; j++)
    if (baseProvider(list[j].id).toLowerCase() === primary
      && list[j].status === "ready") return list[j].id
  // Nothing remembered or primary that reads: the first working account.
  for (var r = 0; r < list.length; r++)
    if (list[r].status === "ready") return list[r].id
  return list[0].id
}

function settingsWithOverrides(settings, moduleName, overrides) {
  var moduleId = cleanText(moduleName, 180).trim()
  if (moduleId === "" || !overrides || typeof overrides !== "object" || Array.isArray(overrides))
    return null

  var next = { id: moduleId }
  var current = settings && typeof settings === "object" && !Array.isArray(settings)
    ? settings : {}
  for (var key in current) {
    if (key === "id" || key === "__proto__" || key === "constructor" || key === "prototype")
      continue
    next[key] = current[key]
  }
  for (var overrideKey in overrides) {
    if (overrideKey === "id" || overrideKey === "__proto__" || overrideKey === "constructor"
        || overrideKey === "prototype") continue
    next[overrideKey] = overrides[overrideKey]
  }
  return next
}

function settingsWithSelectedEntry(settings, moduleName, entryId) {
  var selected = cleanText(entryId, 180).trim()
  if (selected === "") return null
  return settingsWithOverrides(settings, moduleName, { lastSelectedEntryId: selected })
}

function booleanSetting(value, fallback) {
  if (value === true || value === false) return value
  var normalized = String(value === undefined || value === null ? "" : value).trim().toLowerCase()
  if (["true", "1", "yes", "on"].indexOf(normalized) >= 0) return true
  if (["false", "0", "no", "off"].indexOf(normalized) >= 0) return false
  return fallback === true
}

// `providerLabel` is already resolved by the caller: empty when the opt-in
// provider switch is off, so the icon-and-value label is unchanged for everyone
// who never turns it on. A vertical bar has no width for either field and
// keeps showing the icon alone.
function barLabel(alarming, vertical, showValue, loading, hasEntry, summaryText,
                  providerLabel) {
  var icon = "󰚩"
  if (vertical) return alarming ? "󰅙" : icon
  if (loading && !hasEntry) return icon + "  …"
  if (!hasEntry) return alarming ? "󰅙" : icon
  var provider = autoTextSafe(providerLabel).trim()
  var summary = showValue ? autoTextSafe(summaryText).trim() : ""
  if (provider === "") return summary === "" ? icon : icon + "  " + summary
  // One space between tag and value, matching Waybar's
  // `{vendor_short} {session_pct}%`; the wider gap stays next to the icon.
  return summary === "" ? icon + "  " + provider
    : icon + "  " + provider + " " + summary
}

// The per-account tag of a bar tile. A named account shows its own name
// ONLY (kimi-main 37% — the provider stays in the popup); an unnamed default
// account shows the provider name (kimi 37%), taken from the report id the
// Rust side owns. Same rule the Rust `--vendor all` bar applies, so the two
// presentations can never disagree about a tile's name.
function tileTag(entry) {
  if (!entry) return ""
  var id = String(entry.id || "")
  var at = id.indexOf("@")
  if (at >= 0 && at + 1 < id.length) return autoTextSafe(id.slice(at + 1)).trim()
  return autoTextSafe(baseProvider(entry.id)).trim()
}

// The tile's color class: the least REMAINING across its shown windows,
// mapped to the four-step range every tile carries (so usage ranges read at
// a glance, not just alerts):
//   remaining < 5%  → "critical" (red)
//   remaining < 10% → "high"     (orange — the user-specified alert band)
//   remaining < 50% → "mid"      (yellow)
//   otherwise       → "low"      (green)
function tileRemainingClass(entry) {
  if (!entry || entry.status !== "ready") return ""
  var rows = tileWindows(entry)
  if (rows.length === 0) {
    var sections = entry.sections || []
    for (var i = 0; i < sections.length; i++)
      if (sections[i].type === "metric") { rows.push(sections[i]); break }
  }
  var minRemaining = null
  for (var r = 0; r < rows.length; r++) {
    if (rows[r].percent === null || rows[r].percent === undefined) continue
    var remaining = 100 - rows[r].percent
    if (minRemaining === null || remaining < minRemaining) minRemaining = remaining
  }
  if (minRemaining === null) return ""
  if (minRemaining < 5) return "critical"
  if (minRemaining < 10) return "high"
  if (minRemaining < 50) return "mid"
  return "low"
}

// One-component countdown ("2h", "3d", "45m") for a bar tile — the old GNOME
// panel's reset column, compressed to what a one-line bar has room for.
function compactReset(resetAt, nowMs) {
  if (!resetAt) return ""
  var resetMs = new Date(String(resetAt)).getTime()
  if (!isFinite(resetMs)) return ""
  var remaining = resetMs - Number(nowMs)
  if (remaining <= 0) return "due"
  var minutes = Math.floor(remaining / 60000)
  var hours = Math.floor(minutes / 60)
  var days = Math.floor(hours / 24)
  if (days > 0) return days + "d"
  if (hours > 0) return hours + "h"
  return Math.max(1, minutes) + "m"
}

// The two windows a tile leads with: the rolling ~5h session and the weekly
// quota. The classification is Rust-owned (`window` on each metric row);
// this only picks the rows, so the rule can never fork between frontends.
function tileWindows(entry) {
  var session = null, weekly = null
  var sections = entry && entry.sections ? entry.sections : []
  for (var i = 0; i < sections.length; i++) {
    var section = sections[i]
    if (section.type !== "metric") continue
    if (!session && section.window === "session") session = section
    else if (!weekly && section.window === "weekly") weekly = section
  }
  var out = []
  if (session) out.push(session)
  if (weekly) out.push(weekly)
  return out
}

// One account's tile: tag + the 5h and weekly figures, each with its own
// reset countdown ("kimi 38%·2h 37%·2d"), falling back to the single
// headline figure for accounts without windows (balances). `showRemaining`
// flips percentages to what is LEFT of the window. `showValue` off
// degrades every tile to its tag.
function tileLabel(entry, showValue, showRemaining, nowMs) {
  if (!entry) return ""
  var tag = tileTag(entry)
  if (!showValue) return tag
  var windows = tileWindows(entry)
  if (windows.length > 0) {
    var figures = []
    for (var i = 0; i < windows.length; i++) {
      var metric = windows[i]
      var pct = showRemaining ? 100 - metric.percent : metric.percent
      // A window without a countdown (not started yet) keeps its slot with
      // a dash placeholder — `0%·-` lines up with `38%·2h` instead of
      // looking like the timer failed to load.
      var reset = compactReset(metric.reset_at, nowMs)
      if (reset === "") reset = "-"
      figures.push(pct + "%·" + reset)
    }
    return tag === "" ? figures.join(" ") : tag + " " + figures.join(" ")
  }
  var summary = headline(entry)
  var text = autoTextSafe(summary.text).trim()
  if (summary.percent !== null && showRemaining) text = (100 - summary.percent) + "%"
  if (text === "" || text === "Ready" || text === "Error") return tag
  return tag === "" ? text : tag + " " + text
}

// The tiled bar: every WORKING account side by side, the presentation the
// old GNOME panel used (`key1 42% 4h │ key2 80% 1d`). Misconfigured entries
// (no key, broken config) stay off the bar — the click popup keeps their
// rows and remedies; when every account is broken the bar shows the alert
// icon alone so the panel is where it gets fixed.
function tiledBarLabel(alarming, vertical, showValue, loading, entries,
                       showRemaining, nowMs) {
  var icon = "󰚩"
  var all = Array.isArray(entries) ? entries : []
  var working = all.filter(function(entry) { return entry.status === "ready" })
  if (vertical) return alarming ? "󰅙" : icon
  if (all.length === 0) return loading ? icon + "  …" : (alarming ? "󰅙" : icon)
  if (working.length === 0) return "󰅙"
  var parts = []
  for (var i = 0; i < working.length; i++) {
    var tile = tileLabel(working[i], showValue, showRemaining, nowMs)
    if (tile !== "") parts.push(tile)
  }
  if (parts.length === 0) return icon
  return icon + "  " + parts.join("  │  ")
}

function headline(entry) {
  if (!entry) return { text: "", percent: null, severity: "low", label: "", reset_at: "" }
  var best = null
  var sections = entry.sections || []
  for (var i = 0; i < sections.length; i++) {
    var section = sections[i]
    if (section.type === "metric" && (!best || section.percent > best.percent)) best = section
  }
  if (best) {
    var bestText = /balance/i.test(best.label) && best.value !== ""
      ? best.value : best.percent + "%"
    return {
      text: bestText,
      percent: best.percent,
      severity: best.severity,
      label: best.label,
      reset_at: best.reset_at || ""
    }
  }
  for (var j = 0; j < sections.length; j++) {
    var row = sections[j]
    if (row.type === "text" && /(balance|available|spend|prepaid)/i.test(row.label) && row.value !== "")
      return { text: row.value, percent: null, severity: "low", label: row.label, reset_at: "" }
  }
  return { text: entry.status === "error" ? "Error" : "Ready", percent: null, severity: "low", label: "", reset_at: "" }
}

function isAlarming(entry) {
  if (!entry) return false
  if (entry.status === "unconfigured") return false
  var summary = headline(entry)
  return entry.status === "error" || entry.stale === true || summary.severity === "critical"
}

function formatDuration(milliseconds) {
  if (!(milliseconds > 0)) return "now"
  var minutes = Math.floor(milliseconds / 60000)
  var hours = Math.floor(minutes / 60)
  var days = Math.floor(hours / 24)
  if (days > 0) return days + "d " + (hours % 24) + "h"
  if (hours > 0) return hours + "h " + (minutes % 60) + "m"
  return Math.max(1, minutes) + "m"
}

var MONTH_NAMES = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                   "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]

function pad2(value) {
  return ("0" + value).slice(-2)
}

function isSameLocalDay(a, b) {
  return a.getFullYear() === b.getFullYear()
    && a.getMonth() === b.getMonth()
    && a.getDate() === b.getDate()
}

function formatReset(resetAt, nowMs) {
  if (!resetAt) return ""
  var resetMs = new Date(String(resetAt)).getTime()
  if (!isFinite(resetMs)) return ""
  var remaining = resetMs - Number(nowMs)
  if (remaining <= 0) return "Reset due"
  // Show the real local clock time the window reopens, so the absolute
  // reset moment is visible next to the countdown. Date it whenever it
  // lands on another day: a bare "03:00" on an 18h countdown reads as a
  // time that has already passed, which is the ambiguity this row exists
  // to remove. Keyed on the calendar day rather than on "is it 24h away",
  // because tonight's reset crosses midnight long before it crosses 24h.
  var at = new Date(resetMs)
  var clock = pad2(at.getHours()) + ":" + pad2(at.getMinutes())
  if (!isSameLocalDay(at, new Date(Number(nowMs))))
    clock = MONTH_NAMES[at.getMonth()] + " " + at.getDate() + " " + clock
  return "Resets in " + formatDuration(remaining) + " · " + clock
}

function formatUpdated(fetchedAt, nowMs) {
  if (!fetchedAt) return "Updated time unavailable"
  var fetchedMs = new Date(String(fetchedAt)).getTime()
  if (!isFinite(fetchedMs)) return "Updated time unavailable"
  var elapsed = Math.max(0, Number(nowMs) - fetchedMs)
  if (elapsed < 60000) return "Updated just now"
  return "Updated " + formatDuration(elapsed) + " ago"
}

function metricDetail(row) {
  var detail = cleanText(row && row.detail, 1000)
  if (!row || !row.reset_at) return detail
  // Older human-readable reset text remains useful to CLI consumers. Strip
  // just that fragment in the native panel, which renders a live countdown.
  detail = detail.replace(/^Resets in [^·]+\s*(?:·\s*)?/i, "")
  detail = detail.replace(/\s*·\s*reset\s+[^·]+$/i, "")
  return detail.trim()
}

function errorMessage(value) {
  var message = cleanText(value, 500).trim()
  return message === "" ? "The usage command failed without an error message." : message
}

// The panel launches ai-usagebar-omarchy through /usr/bin/env, so a missing binary
// comes back as exit 127 instead of the process simply never starting.
// Quickshell does not emit `exited` when it cannot launch a binary directly --
// it only logs an internal warning -- which used to leave the widget stuck on
// its loading state with no way to explain that the binary was not installed.
function launchErrorMessage(exitCode, stderrText) {
  if (Number(exitCode) === 127)
    return "ai-usagebar-omarchy is not installed. The plugin is only the display frontend. Install Rust from https://rustup.rs if needed, then: cargo install --git https://github.com/KyleLee/ai-usagebar-omarchy"
  return errorMessage(stderrText)
}

function settingsId(value) {
  var id = cleanText(value, 80).trim()
  if (!/^[a-z0-9_-]+$/.test(id)
      || id === "__proto__" || id === "constructor" || id === "prototype") return ""
  return id
}

// The Rust bridge deliberately returns only key-presence booleans. Keep this
// parser strict so a compromised/older helper cannot smuggle rich text or an
// unbounded model into the long-lived shell process.
// Field ids share the vendor-id alphabet (settingsId already allows _).
function fieldId(value) {
  return settingsId(value)
}

// A per-vendor editable field on an account card. `text` renders a line
// edit, `choice` a dropdown (with optional display labels — the canonical
// product names live in Rust), `secret` a password row whose value never
// travels (presence rides the account's configured flags).
function normalizeField(raw) {
  if (!raw || typeof raw !== "object") return null
  var id = fieldId(raw.id)
  if (id === "") return null
  var kind = raw.kind === "choice" ? "choice" : raw.kind === "secret" ? "secret" : "text"
  var choices = []
  if (kind === "choice" && Array.isArray(raw.choices)) {
    for (var i = 0; i < raw.choices.length && i < 12; i++) {
      var choice = cleanText(raw.choices[i], 40)
      if (choice !== "") choices.push(choice)
    }
  }
  var labels = {}
  if (raw.labels && typeof raw.labels === "object" && !Array.isArray(raw.labels)) {
    for (var key in raw.labels) {
      if (key === "__proto__" || key === "constructor" || key === "prototype") continue
      var value = cleanText(raw.labels[key], 60)
      if (value !== "") labels[key] = value
    }
  }
  return {
    id: id,
    label: cleanText(raw.label, 120) || id,
    kind: kind,
    value: cleanText(raw.value, 200),
    choices: choices,
    labels: labels
  }
}

// One account of a multi-account vendor (Z.AI today): a labeled bundle of
// fields plus key-presence booleans. label "" is the vendor's default
// section.
function normalizeAccount(raw) {
  if (!raw || typeof raw !== "object") return null
  var vendor = settingsId(raw.vendor)
  var label = cleanText(raw.label, 120)
  if (vendor === "") return null
  var fields = []
  var source = Array.isArray(raw.fields) ? raw.fields : []
  for (var i = 0; i < source.length && i < 16; i++) {
    var field = normalizeField(source[i])
    if (field) fields.push(field)
  }
  return {
    vendor: vendor,
    label: label,
    display: cleanText(raw.display, 120) || (label === "" ? "Default" : label),
    environment: cleanText(raw.environment, 160),
    configured: raw.configured === true,
    inline_configured: raw.inline_configured === true,
    environment_configured: raw.environment_configured === true,
    fields: fields
  }
}

function parseSettingsSnapshot(raw) {
  try {
    var parsed = JSON.parse(String(raw || ""))
    if (!parsed || Number(parsed.schema_version) !== 1
        || !Array.isArray(parsed.primary_choices) || !Array.isArray(parsed.keys))
      return { ok: false, error: "The settings command returned an unsupported response.", primary: "", primary_choices: [], keys: [], accounts: [] }

    var choices = []
    for (var i = 0; i < parsed.primary_choices.length && i < 64; i++) {
      var choice = parsed.primary_choices[i]
      var choiceId = settingsId(choice && choice.id)
      if (choiceId === "") continue
      choices.push({ id: choiceId, value: choiceId, label: cleanText(choice.label, 120) || choiceId })
    }

    var keys = []
    for (var j = 0; j < parsed.keys.length && j < 32; j++) {
      var key = parsed.keys[j]
      var keyId = settingsId(key && key.id)
      if (keyId === "") continue
      var fields = []
      var rawFields = key && Array.isArray(key.fields) ? key.fields : []
      for (var f = 0; f < rawFields.length && f < 12; f++) {
        var field = normalizeField(rawFields[f])
        if (field) fields.push(field)
      }
      keys.push({
        id: keyId,
        label: cleanText(key.label, 120) || keyId,
        environment: cleanText(key.environment, 160),
        note: cleanText(key.note, 240),
        configured: key.configured === true,
        inline_configured: key.inline_configured === true,
        environment_configured: key.environment_configured === true,
        fields: fields
      })
    }

    var primary = settingsId(parsed.primary)
    var primaryAvailable = false
    for (var k = 0; k < choices.length; k++) {
      if (choices[k].id === primary) {
        primaryAvailable = true
        break
      }
    }
    var accounts = []
    var rawAccounts = Array.isArray(parsed.accounts) ? parsed.accounts : []
    for (var a = 0; a < rawAccounts.length && a < 16; a++) {
      var account = normalizeAccount(rawAccounts[a])
      if (account) accounts.push(account)
    }

    if (!primaryAvailable) primary = choices.length > 0 ? choices[0].id : ""
    return {
      ok: true, error: "", primary: primary,
      primary_choices: choices, keys: keys, accounts: accounts
    }
  } catch (error) {
    return { ok: false, error: "The settings command returned invalid JSON.", primary: "", primary_choices: [], keys: [], accounts: [] }
  }
}

// accountChanges: [{action:"update",vendor,label,fields,apiKey:{action,value}} |
//                  {action:"add",vendor,name,fields,apiKey} |
//                  {action:"remove",vendor,label}]
function buildSettingsPatch(primary, changes, accountChanges) {
  var primaryId = settingsId(primary)
  var rawPrimary = String(primary || "").trim()
  if (rawPrimary !== "" && primaryId === "")
    return { ok: false, error: "Choose a valid primary provider.", payload: "" }
  var keys = {}
  var list = Array.isArray(changes) ? changes : []
  var seen = []
  for (var i = 0; i < list.length; i++) {
    var change = list[i] || {}
    var id = settingsId(change.id)
    if (id === "" || seen.indexOf(id) >= 0)
      return { ok: false, error: "A settings row has an invalid provider id.", payload: "" }
    seen.push(id)
    var fields = null
    if (change.fields && typeof change.fields === "object"
        && !Array.isArray(change.fields)) {
      fields = {}
      for (var fieldKey in change.fields) {
        if (fieldKey === "__proto__" || fieldKey === "constructor" || fieldKey === "prototype")
          continue
        var fid = fieldId(fieldKey)
        var value = String(change.fields[fieldKey] || "")
        if (fid === "")
          return { ok: false, error: "A settings field has an invalid id.", payload: "" }
        if (value.length > 200)
          return { ok: false, error: "A settings field is too long.", payload: "" }
        fields[fid] = value
      }
      if (Object.keys(fields).length === 0) fields = null
    }
    if (change.action === "clear") {
      keys[id] = fields ? { action: "clear", fields: fields } : { action: "clear" }
    } else if (change.action === "set") {
      var value = String(change.value || "").trim()
      if (value === "") return { ok: false, error: "An edited API key is empty.", payload: "" }
      if (value.length > 16384) return { ok: false, error: "An API key is too long.", payload: "" }
      keys[id] = fields ? { action: "set", value: value, fields: fields }
        : { action: "set", value: value }
    } else if (change.action === "fields") {
      if (!fields)
        return { ok: false, error: "A field-only change has no fields.", payload: "" }
      keys[id] = { action: "fields", fields: fields }
    } else return { ok: false, error: "A settings row has an invalid action.", payload: "" }
  }
  var accountsByVendor = {}
  var list = Array.isArray(accountChanges) ? accountChanges : []
  for (var n = 0; n < list.length; n++) {
    var change = list[n] || {}
    var vendor = settingsId(change.vendor)
    if (vendor === "")
      return { ok: false, error: "An account change has an invalid vendor.", payload: "" }
    var mutation = { action: change.action }
    if (change.action === "update" || change.action === "remove") {
      if (typeof change.label !== "string")
        return { ok: false, error: "An account change is missing its label.", payload: "" }
      mutation.label = cleanText(change.label, 120)
    } else if (change.action === "add") {
      var name = String(change.name || "").trim()
      if (name === "")
        return { ok: false, error: "A new account needs a name.", payload: "" }
      if (name.length > 120)
        return { ok: false, error: "An account name is too long.", payload: "" }
      mutation.name = name
    } else {
      return { ok: false, error: "An account change has an invalid action.", payload: "" }
    }
    if (change.action !== "remove") {
      if (change.fields && typeof change.fields === "object" && !Array.isArray(change.fields)) {
        var fields = {}
        for (var fk in change.fields) {
          if (fk === "__proto__" || fk === "constructor" || fk === "prototype") continue
          var fid = fieldId(fk)
          // Trimmed: pasted ids/names carry stray whitespace far more often
          // than any of these fields can meaningfully contain it.
          var fvalue = String(change.fields[fk] || "").trim()
          if (fid === "")
            return { ok: false, error: "An account field has an invalid id.", payload: "" }
          if (fvalue.length > 200)
            return { ok: false, error: "An account field is too long.", payload: "" }
          fields[fid] = fvalue
        }
        if (Object.keys(fields).length > 0) mutation.fields = fields
      }
      if (change.apiKey && typeof change.apiKey === "object") {
        if (change.apiKey.action === "set") {
          var keyValue = String(change.apiKey.value || "").trim()
          if (keyValue === "")
            return { ok: false, error: "An edited account API key is empty.", payload: "" }
          if (keyValue.length > 16384)
            return { ok: false, error: "An account API key is too long.", payload: "" }
          mutation.api_key = { action: "set", value: keyValue }
        } else if (change.apiKey.action === "clear") {
          mutation.api_key = { action: "clear" }
        }
      }
    }
    if (!accountsByVendor[vendor]) accountsByVendor[vendor] = []
    accountsByVendor[vendor].push(mutation)
  }

  if (primaryId === "" && seen.length === 0 && Object.keys(accountsByVendor).length === 0)
    return { ok: false, error: "There are no settings changes to save.", payload: "" }
  var patch = { schema_version: 1, keys: keys }
  if (Object.keys(accountsByVendor).length > 0) patch.accounts = accountsByVendor
  if (primaryId !== "") patch.primary = primaryId
  return {
    ok: true,
    error: "",
    payload: JSON.stringify(patch)
  }
}

function parseSettingsApplyResult(raw) {
  try {
    var parsed = JSON.parse(String(raw || ""))
    return parsed && parsed.ok === true
  } catch (error) {
    return false
  }
}
