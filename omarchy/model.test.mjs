import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';

const source = fs.readFileSync(new URL('./Model.js', import.meta.url), 'utf8');
const model = {};
vm.createContext(model);
vm.runInContext(source, model, {filename: 'Model.js'});

// Keep the marketplace/runtime shape in CI. The marketplace's structural
// validator only checks that the declared file exists; Quattro additionally
// needs the bar entry point to forward its nested panel lifecycle.
const manifest = JSON.parse(fs.readFileSync(new URL('../manifest.json', import.meta.url), 'utf8'));
assert.deepEqual(manifest.kinds, ['bar-widget']);
assert.equal(manifest.entryPoints.barWidget, 'omarchy/BarWidget.qml');
// The value/provider toggles stay gone; tiling is back as a boolean
// (default on — the tiled bar every user already has) next to the
// used/remaining switch.
assert.equal(manifest.barWidget.defaults.showValue, undefined);
assert.equal(manifest.barWidget.schema.find(row => row.key === 'showValue'), undefined);
assert.equal(manifest.barWidget.defaults.showProvider, undefined);
assert.equal(manifest.barWidget.schema.find(row => row.key === 'showProvider'), undefined);
assert.equal(manifest.barWidget.defaults.showRemaining, true);
const showRemainingSchema = manifest.barWidget.schema.find(row => row.key === 'showRemaining');
assert.equal(showRemainingSchema.type, 'boolean');
assert.equal(showRemainingSchema.defaultValue, true);
assert.equal(manifest.barWidget.defaults.barTiled, true);
const barTiledSchema = manifest.barWidget.schema.find(row => row.key === 'barTiled');
assert.equal(barTiledSchema.type, 'boolean');
assert.equal(barTiledSchema.defaultValue, true);

const barWidgetSource = fs.readFileSync(new URL('./BarWidget.qml', import.meta.url), 'utf8');
assert.match(barWidgetSource, /^BarWidget\s*\{/m);
for (const method of ['open', 'close', 'toggle', 'closeForPopoutSwitch'])
  assert.match(barWidgetSource, new RegExp(`function\\s+${method}\\s*\\(`));
assert.match(barWidgetSource, /source:\s*Qt\.resolvedUrl\("Panel\.qml"\)/);
assert.match(barWidgetSource, /target\.anchorItem\s*=\s*button/);
assert.match(barWidgetSource, /target\.hostWidget\s*=\s*root/);
assert.match(barWidgetSource, /buttonCode\s*===\s*Qt\.RightButton\)\s*root\.launchDashboard\(\)/);
assert.doesNotMatch(barWidgetSource, /\bIpcHandler\s*\{/);

const panelSource = fs.readFileSync(new URL('./Panel.qml', import.meta.url), 'utf8');
assert.match(panelSource, /^Panel\s*\{/m);
assert.match(panelSource, /property\s+var\s+anchorItem:\s*null/);
assert.match(panelSource, /property\s+var\s+hostWidget:\s*null/);
assert.match(panelSource, /SettingsView\s*\{/);
assert.match(panelSource, /function\s+openSettings\s*\(/);
assert.match(panelSource, /setting\("lastSelectedEntryId",\s*""\)/);
assert.doesNotMatch(panelSource, /showProvider/);
assert.match(panelSource, /setting\("showRemaining",\s*true\)/);
assert.match(panelSource, /setting\("showRemaining",\s*true\)/);
assert.match(panelSource, /onShowRemainingRequested/);
assert.doesNotMatch(panelSource, /setting\("showValue"/);
// Tiling is a setting again, default on; the panel reads it and can flip it.
assert.match(panelSource, /setting\("barTiled",\s*true\)/);
assert.match(panelSource, /function\s+setBarTiled\s*\(/);
assert.match(panelSource, /onBarTiledRequested/);
assert.match(panelSource, /function\s+persistSelection\s*\(/);
assert.match(panelSource, /Model\.settingsWithOverrides\(root\.settings,\s*root\.moduleName,\s*values\)/);
assert.match(panelSource, /bar\.shell\.updateEntryInline\(root\.moduleName,\s*entry\)/);
assert.match(panelSource, /persistSelection\(selectedEntryId\)/);

const settingsViewSource = fs.readFileSync(new URL('./SettingsView.qml', import.meta.url), 'utf8');
assert.match(settingsViewSource, /command:\s*\["ai-usagebar-omarchy",\s*"settings",\s*"show"\]/);
assert.match(settingsViewSource, /command:\s*\["ai-usagebar-omarchy",\s*"settings",\s*"apply"\]/);
assert.match(settingsViewSource, /stdinEnabled:\s*true/);
assert.match(settingsViewSource, /write\(root\.pendingPayload\s*\+\s*"\\n"\)/);
// The hero's trailing buttons keep clear of the scrollbar overlay.
assert.match(panelSource, /width: Style\.space\(6\)\n\s+height: 1/);
// Focus-loss dismissal remembers the settings scroll; reopening restores it.
assert.match(panelSource, /property real savedSettingsScrollY: 0/);
// Translucent backdrop behind the tiles: on a transparent bar, busy or
// light wallpapers wash the text out.
assert.match(barWidgetSource, /Util\.alpha\(Color\.background, 0\.62\)/);
// The panel must CLOSE on focus loss (no settingsOpen guard in close) —
// keeping it open swallowed the dismissal click and blocked other windows.
assert.doesNotMatch(barWidgetSource, /settingsOpen\) return/);
assert.match(panelSource, /savedSettingsScrollY = panelFlick\.contentY/);
assert.match(panelSource, /panelFlick\.contentY = Math\.min\(root\.savedSettingsScrollY/);
// Each draft carries its own Apply button right under the key field — the
// global save at the page bottom is below the fold exactly when it matters.
assert.match(settingsViewSource, /text: root\.saving \? "Saving…" : "Apply"/);
// Drafts no longer name accounts client-side: the form asks for no name and
// the server auto-names on add (Z.AI, Z.AI 2 …). The old pre-naming
// machinery must not creep back.
assert.doesNotMatch(settingsViewSource, /function nextDraftName/);
assert.doesNotMatch(settingsViewSource, /draftName/);
assert.match(settingsViewSource, /action: "add", vendor: draftVendor, fields: fields/);
// Accounts render in ONE root-level section (zai's proven shape): no
// per-vendor delegate sections — those hid `accountRepeater` from root
// functions and silently killed every save (and the back button via
// scrubSecrets). Both vendors add from their own button.
assert.equal(settingsViewSource.indexOf('id: vendorSection'), -1,
  'per-vendor delegate sections must not come back');
assert.match(settingsViewSource, /text: "ACCOUNTS"/);
// ONE add button PER VENDOR, right under that vendor's last key — the
// button label comes from Rust's vendor_display, never a frontend table.
assert.doesNotMatch(settingsViewSource, /text: "Add account"/);
assert.match(settingsViewSource, /"Add " \+ row\.modelData\.vendorDisplay \+ " Account"/);
assert.match(settingsViewSource, /onClicked: root\.addDraftAccount\(row\.modelData\.vendor\)/);
// The draft's vendor is fixed by its button — no in-draft provider picker.
assert.doesNotMatch(settingsViewSource, /function switchVendor/);
// Card mutations carry the card's own vendor — the hardcoded "zai" here
// once wrote kimi keys into the [zai] section.
assert.doesNotMatch(settingsViewSource, /vendor: shape\.card\.vendor \|\|/);
assert.match(settingsViewSource, /vendor: shape\.card\.vendor/);
// Card titles render Rust's vendor_display — no zai/kimi string table here.
assert.doesNotMatch(settingsViewSource, /"Kimi \. " : "Z\.AI \. "/);
// Drafts key by vendor section id; the array only changes on clicks.
assert.match(settingsViewSource, /drafts = drafts\.concat\(\[vendor\]\)/);
assert.match(settingsViewSource, /function removeDraftAt\(index\)/);
// Zero accounts still shows the section (its Add buttons are the entry
// point when nothing is configured).
{
  const at = settingsViewSource.indexOf('text: "ACCOUNTS"');
  assert.match(settingsViewSource.slice(at - 400, at), /visible: !root\.loading/,
    'accounts section must not require accounts to exist');
}
assert.match(settingsViewSource, /label:\s*"Show remaining instead of used"/);
// Tiling is a visible toggle again (default on); the value toggle stays out.
assert.match(settingsViewSource, /label:\s*"Tile every account in the top bar"/);
assert.doesNotMatch(settingsViewSource, /label:\s*"Show usage value in the top bar"/);
// The plain API KEYS section is gone — every key vendor is an account card.
assert.doesNotMatch(settingsViewSource, /text: "API KEYS"/);
assert.doesNotMatch(settingsViewSource, /snapshot\.keys/);
// One vendor-grouped model drives the whole section; group prefix bars
// alternate two colors by GROUP INDEX (Z.AI blue, Kimi purple, then
// alternating down the list) — not by a per-vendor table.
assert.match(settingsViewSource, /function accountsModel\(\)/);
assert.match(settingsViewSource, /model: root\.accountsModel\(\)/);
assert.match(settingsViewSource, /g % 2 === 0 \? "#61afef" : "#c678dd"/);
assert.match(settingsViewSource, /color: row\.modelData\.accent/);
assert.doesNotMatch(settingsViewSource, /function vendorAccent/);
assert.doesNotMatch(settingsViewSource, /Nous/);
assert.doesNotMatch(panelSource, /openNousLogin/);
assert.doesNotMatch(settingsViewSource, /command:\s*\[[^\]]*(?:api.?key|secret|pendingPayload)/i);

const raw = JSON.stringify({primary: 'openai', entries: [
  {
    id: 'anthropic@work',
    name: 'anthropic · work',
    display_name: 'Claude · work',
    short_name: 'cld',
    plan: 'Claude Max 20x',
    status: 'ready',
    error: null,
    stale: true,
    fetched_at: '2026-08-14T12:00:00Z',
    sections: [
      {type: 'spacer'},
      {type: 'metric', label: 'Session (5h)', percent: 29, value: '29%',
       detail: 'Resets in 2h 0m · 60% elapsed · 31pts under', severity: 'low',
       reset_at: '2026-08-14T14:00:00Z', window: 'session'},
      {type: 'text', label: 'Balance', value: '$12.00'},
      {type: 'block', label: 'Credits', body: ['balance: 20', '≈ 10 messages']}
    ]
  },
  {
    id: 'openai', name: 'openai', display_name: 'Codex', short_name: 'gpt', plan: 'Plus', error: null,
    sections: [{type: 'metric', label: 'Codex weekly', percent: 95, value: '95%', detail: '', severity: 'critical', window: 'weekly'}]
  }
]});

const parsed = model.parseReport(raw);
assert.equal(parsed.ok, true);
assert.equal(parsed.primary, 'openai');
assert.equal(parsed.entries.length, 2);
assert.equal(parsed.entries[0].stale, true);
assert.equal(parsed.entries[0].sections[1].reset_at, '2026-08-14T14:00:00Z');
assert.equal(model.providerName(parsed.entries[0]), 'Claude · work');
assert.equal(model.providerName(parsed.entries[1]), 'Codex');
assert.deepEqual(Array.from(model.filteredEntries(parsed.entries, '')).map(entry => entry.id), ['anthropic@work', 'openai']);
assert.deepEqual(Array.from(model.filteredEntries(parsed.entries, 'anthropic')).map(entry => entry.id), ['anthropic@work']);
assert.deepEqual(Array.from(model.filteredEntries(parsed.entries, 'openai')).map(entry => entry.id), ['openai']);
assert.equal(model.selectedIndex(parsed.entries, 'openai'), 1);
assert.equal(model.selectedIndex(parsed.entries, 'missing'), 0);
assert.equal(model.preferredEntryId(parsed.entries, parsed.primary), 'openai');
assert.equal(model.preferredEntryId(parsed.entries, 'anthropic'), 'anthropic@work');
assert.equal(model.preferredEntryId(parsed.entries, 'missing'), 'anthropic@work');
assert.equal(model.preferredEntryId(parsed.entries, parsed.primary, 'anthropic@work'), 'anthropic@work');
assert.equal(model.preferredEntryId(parsed.entries, parsed.primary, '  ANTHROPIC@WORK  '), 'anthropic@work');
assert.equal(model.preferredEntryId(parsed.entries, parsed.primary, 'missing'), 'openai');

const openRouterAccounts = model.parseReport(JSON.stringify({entries: [{
  id: 'openrouter@work', name: 'openrouter · work', display_name: 'OpenRouter · work',
  error: null, sections: []
}, {
  id: 'openrouter@personal', name: 'openrouter · personal', display_name: 'OpenRouter · personal',
  error: null, sections: []
}]})).entries;
assert.deepEqual(Array.from(model.filteredEntries(openRouterAccounts, 'openrouter')).map(entry => entry.id),
  ['openrouter@work', 'openrouter@personal']);
assert.equal(model.providerName(openRouterAccounts[0]), 'OpenRouter · work');
assert.equal(model.preferredEntryId(openRouterAccounts, 'openrouter', 'openrouter@personal'),
  'openrouter@personal');
assert.equal(model.preferredEntryId(openRouterAccounts, 'openrouter', 'openrouter@missing'),
  'openrouter@work');

const priorWidgetSettings = {
  provider: '', refreshIntervalSec: 90, futureSetting: {keep: true}, id: 'stale-id'
};
const selectedWidgetSettings = model.settingsWithSelectedEntry(
  priorWidgetSettings, 'ai-usagebar-omarchy', 'openrouter@personal');
assert.deepEqual(JSON.parse(JSON.stringify(selectedWidgetSettings)), {
  id: 'ai-usagebar-omarchy',
  provider: '',
  refreshIntervalSec: 90,
  futureSetting: {keep: true},
  lastSelectedEntryId: 'openrouter@personal'
});
assert.equal(priorWidgetSettings.lastSelectedEntryId, undefined);
assert.equal(model.settingsWithSelectedEntry({}, 'ai-usagebar-omarchy', ''), null);
const hiddenValueSettings = model.settingsWithOverrides(
  selectedWidgetSettings, 'ai-usagebar-omarchy', {showValue: false});
assert.equal(hiddenValueSettings.showValue, false);
assert.equal(hiddenValueSettings.lastSelectedEntryId, 'openrouter@personal');
assert.equal(selectedWidgetSettings.showValue, undefined);
const shownProviderSettings = model.settingsWithOverrides(
  hiddenValueSettings, 'ai-usagebar-omarchy', {showProvider: true});
assert.equal(shownProviderSettings.showProvider, true);
assert.equal(shownProviderSettings.showValue, false);
assert.equal(shownProviderSettings.lastSelectedEntryId, 'openrouter@personal');
assert.equal(hiddenValueSettings.showProvider, undefined);
const protectedSettings = model.settingsWithOverrides({}, 'ai-usagebar-omarchy', {
  id: 'wrong-id', constructor: 'ignored', prototype: 'ignored', showValue: false
});
assert.equal(protectedSettings.id, 'ai-usagebar-omarchy');
assert.notEqual(protectedSettings.constructor, 'ignored');
assert.equal(protectedSettings.prototype, undefined);
assert.equal(model.booleanSetting(undefined, true), true);
assert.equal(model.booleanSetting(false, true), false);
assert.equal(model.booleanSetting('false', true), false);
assert.equal(model.booleanSetting('true', false), true);
assert.equal(model.booleanSetting('invalid', true), true);

assert.equal(model.barLabel(false, false, true, false, true, '29%'), '󰚩  29%');
assert.equal(model.barLabel(false, false, false, false, true, '29%'), '󰚩');
assert.equal(model.barLabel(true, false, true, false, true, '95%'), '󰚩  95%');
assert.equal(model.barLabel(true, false, false, false, true, '95%'), '󰚩');
assert.equal(model.barLabel(true, false, true, false, false, ''), '󰚩');
assert.equal(model.barLabel(false, true, true, false, true, '29%'), '󰚩');
assert.equal(model.barLabel(true, true, true, false, true, '95%'), '󰚩');
assert.equal(model.barLabel(false, false, true, true, false, ''), '󰚩  …');

// The provider tag is opt-in and arrives already resolved, so every call
// above — no seventh argument at all — has to keep its historical label.
assert.equal(model.barLabel(false, false, true, false, true, '29%', 'gpt'), '󰚩  gpt 29%');
// Tag on, value off: the icon-only label grows the tag and nothing else.
assert.equal(model.barLabel(false, false, false, false, true, '29%', 'gpt'), '󰚩  gpt');
assert.equal(model.barLabel(true, false, true, false, true, '95%', 'cld'), '󰚩  cld 95%');
// An entry with no headline still names its provider.
assert.equal(model.barLabel(false, false, true, false, true, '', 'agy'), '󰚩  agy');
// A vertical bar has no width for either field.
assert.equal(model.barLabel(false, true, true, false, true, '29%', 'gpt'), '󰚩');
// Before the first report there is no provider to name.
assert.equal(model.barLabel(false, false, true, true, false, '', 'gpt'), '󰚩  …');
assert.equal(model.barLabel(true, false, true, false, false, '', 'gpt'), '󰚩');
// A tag that sanitizes down to nothing degrades to the label without one.
assert.equal(model.barLabel(false, false, true, false, true, '29%', '   '), '󰚩  29%');
assert.equal(model.barLabel(false, false, true, false, true, '29%', undefined), '󰚩  29%');

// --- Tiled bar (every account at once, the old GNOME panel layout) --------
// Fixed clock for the countdown assertions.
const tileNow = Date.parse('2026-08-29T12:00:00Z');
// A window resetting 2h05m from tileNow → compact "2h" (2.08h truncated).
const resetAt2h = '2026-08-29T14:05:00Z';

// Tags: named accounts use their own label, default accounts the vendor code.
assert.equal(model.tileTag(parsed.entries[0]), 'work'); // anthropic@work
assert.equal(model.tileTag(parsed.entries[1]), 'openai');  // unnamed default → provider name
assert.equal(model.tileTag({id: 'zai@team'}), 'team');
assert.equal(model.tileTag({id: 'zai', short_name: 'zai'}), 'zai');
assert.equal(model.tileTag({id: 'kimi'}), 'kimi');
assert.equal(model.tileTag(null), '');
// A tile leads with its headline figure plus that window's reset countdown.
// The fixture's reset (2026-08-14) is in the fixed clock's past → "due".
assert.equal(model.tileLabel(parsed.entries[0], true, false, tileNow), 'work 29%·due');
assert.equal(model.tileLabel(parsed.entries[1], true, false, tileNow), 'openai 95%·-');
const kimiTile = model.parseReport(JSON.stringify({entries: [{
  id: 'kimi', short_name: 'kmi', error: null, sections: [
    {type: 'metric', label: 'Rolling window (5h)', percent: 38, value: '38%',
     detail: '', severity: 'low', reset_at: resetAt2h, window: 'session'}
  ]}
]})).entries[0];
assert.equal(model.tileLabel(kimiTile, true, false, tileNow), 'kimi 38%·2h');
// showRemaining flips the figure to what is left of the same window.
assert.equal(model.tileLabel(kimiTile, true, true, tileNow), 'kimi 62%·2h');
// headline carries the winning metric's reset for the tile to use.
assert.equal(model.headline(kimiTile).reset_at, resetAt2h);
// An unstarted window (no reset) keeps the countdown slot with a dash.
const unstarted = model.parseReport(JSON.stringify({entries: [{
  id: 'kimi', error: null, sections: [
    {type: 'metric', label: 'Rolling window (5h)', percent: 0, value: '0%',
     detail: '', severity: 'low', reset_at: '', window: 'session'},
    {type: 'metric', label: 'Weekly quota', percent: 37, value: '37%',
     detail: '', severity: 'mid', reset_at: resetAt2h, window: 'weekly'}
  ]}
]})).entries[0];
assert.equal(model.tileLabel(unstarted, true, false, tileNow), 'kimi 0%·- 37%·2h');
// Both windows tile, each with its own reset: 5h first, weekly second.
const twoWindow = model.parseReport(JSON.stringify({entries: [{
  id: 'kimi', error: null, sections: [
    {type: 'metric', label: 'Rolling window (5h)', percent: 38, value: '38%',
     detail: '', severity: 'low', reset_at: resetAt2h, window: 'session'},
    {type: 'metric', label: 'Weekly quota', percent: 37, value: '37%',
     detail: '', severity: 'mid', reset_at: '2026-09-01T14:05:00Z', window: 'weekly'}
  ]}
]})).entries[0];
assert.equal(model.tileLabel(twoWindow, true, false, tileNow), 'kimi 38%·2h 37%·3d');
assert.equal(model.tileLabel(twoWindow, true, true, tileNow), 'kimi 62%·2h 63%·3d');
// A reset in the past reads as due.
const dueTile = model.parseReport(JSON.stringify({entries: [{
  id: 'zai', short_name: 'zai', error: null, sections: [
    {type: 'metric', label: 'Session (5h)', percent: 40, value: '40%',
     detail: '', severity: 'low', reset_at: '2026-08-29T11:00:00Z', window: 'session'}
  ]}
]})).entries[0];
assert.equal(model.tileLabel(dueTile, true, false, tileNow), 'zai 40%·due');
// Errored accounts degrade to the bare tag (the tiled bar filters them out,
// this only guards direct callers).
assert.equal(model.tileLabel(model.parseReport(JSON.stringify({entries: [{
  id: 'kimi', short_name: 'kmi', error: 'HTTP 401', sections: []
}]})).entries[0], true, false, tileNow), 'kimi');
// No metrics and no error: the bare tag ("Ready" is not a figure).
assert.equal(model.tileLabel(model.parseReport(JSON.stringify({entries: [{
  id: 'antigravity', short_name: 'agy', error: null, sections: []
}]})).entries[0], true, false, tileNow), 'antigravity');
// showValue off degrades every tile to its tag.
assert.equal(model.tileLabel(parsed.entries[0], false, false, tileNow), 'work');
// The tiled label joins one tile per WORKING account with a vertical bar;
// unnamed defaults carry the provider name, named accounts their name only.
assert.equal(model.tiledBarLabel(false, false, true, false, parsed.entries, false, tileNow),
  'anthropic 29%·due│openai 95%·-');
// Balance-style figures ride along like percentages do (no reset appended).
assert.equal(model.tiledBarLabel(false, false, true, false,
  model.parseReport(JSON.stringify({entries: [
    {id: 'zai@team', error: null, sections: [
      {type: 'metric', label: 'Session (5h)', percent: 42, value: '42%', detail: '', severity: 'low', window: 'session'}
    ]},
    {id: 'deepseek', short_name: 'dsk', error: null,
     sections: [{type: 'text', label: 'Balance', value: '$8.42'}]}
  ]})).entries, false, tileNow), 'zai 42%·-│deepseek $8.42',
  'the solo zai key drops its suffix — the logo alone identifies it');
// "unconfigured" is the calm third state: same remedy text, no alarm.
const unconfiguredReport = model.parseReport(JSON.stringify({entries: [{
  id: 'zai', error: 'credentials error: Zai: no API key. Either set an API key …',
  unconfigured: true, sections: []
}, {
  id: 'zai@Z.AI', error: null, sections: [
    {type: 'metric', label: 'Session (5h)', percent: 42, value: '42%', detail: '', severity: 'low', window: 'session'}
  ]
}, {
  id: 'kimi', error: 'HTTP 401: unauthorized', sections: []
}]})).entries;
assert.equal(unconfiguredReport[0].status, 'unconfigured');
assert.equal(unconfiguredReport[1].status, 'ready');
assert.equal(unconfiguredReport[2].status, 'error');
assert.equal(model.isAlarming(unconfiguredReport[0]), false, 'absence is not an alarm');
assert.equal(model.isAlarming(unconfiguredReport[2]), true);

// --- Logos, exhaustion, and the tiling toggle --------------------------------
// The report carries each vendor's logo asset id; entries default to no
// logo (older binaries) and not-exhausted.
{
  const withLogo = model.parseReport(JSON.stringify({entries: [
    {id: 'anthropic@work', logo: 'claude', error: null, sections: []},
    {id: 'zai', error: null, sections: []}
  ]})).entries;
  assert.equal(withLogo[0].logo, 'claude');
  assert.equal(withLogo[1].logo, '');
  assert.equal(withLogo[0].exhausted, false);
  assert.equal(model.logoAssetName('claude'), 'claude.svg');
  assert.equal(model.logoAssetName('nope'), '');
  assert.equal(model.logoAssetName(''), '');
  // Every shipped asset id is a Rust logo_slug — the lockstep set the
  // branding test on the Rust side walks, mirrored here so a new vendor
  // that ships without an asset fails CI on the frontend too.
  assert.deepEqual(Object.keys(model.LOGO_ASSETS).sort(), [
    'anthropic', 'antigravity', 'claude', 'cursor', 'deepseek', 'grok',
    'kilo', 'kimi', 'kiro', 'minimax', 'moonshot', 'novita', 'openai',
    'opencode', 'openrouter', 'zai'
  ]);
  // And every mapped file actually ships with the plugin — a dangling
  // mapping degrades the bar tile silently otherwise.
  for (const [slug, file] of Object.entries(model.LOGO_ASSETS))
    assert.equal(fs.existsSync(new URL(`./logos/${file}`, import.meta.url)), true,
      `logo asset ${slug} → ${file} is missing`);
}

// The tag part carries the logo id and the label that renders beside it:
// the account suffix for named accounts, "" for vendor defaults. `text`
// stays the full tag so a missing asset falls back to the old tile.
{
  const mkEntry = (id, logo) => model.parseReport(JSON.stringify({entries: [{
    id: id, logo: logo, error: null, sections: [
      {type: 'metric', label: 'Session (5h)', percent: 42, value: '42%',
       detail: '', severity: 'low', reset_at: resetAt2h, window: 'session'}
    ]}
  ]})).entries[0];
  const named = model.tileParts(mkEntry('zai@team', 'zai'), true, false, tileNow)[0];
  assert.equal(named.text, 'team');
  assert.equal(named.logo, 'zai');
  assert.equal(named.logoLabel, 'team');
  const unnamed = model.tileParts(mkEntry('zai', 'zai'), true, false, tileNow)[0];
  assert.equal(unnamed.text, 'zai');
  assert.equal(unnamed.logo, 'zai');
  assert.equal(unnamed.logoLabel, '', 'a default account is the logo alone');
  const legacy = model.tileParts(mkEntry('zai', ''), true, false, tileNow)[0];
  assert.equal(legacy.logo, '', 'older binaries carry no logo id');
  assert.equal(legacy.text, 'zai', 'and keep the text tag');
}

// A provider's ONLY key drops its account suffix — a lone "1" beside the
// logo distinguishes it from nothing. Two keys under one provider keep
// their suffixes; an exhausted (bar-hidden) sibling still counts, so the
// suffix returns the moment it does.
{
  const soloEntry = id => model.parseReport(JSON.stringify({entries: [{
    id: id, logo: 'zai', error: null, sections: [
      {type: 'metric', label: 'Session (5h)', percent: 42, value: '42%',
       detail: '', severity: 'low', reset_at: resetAt2h, window: 'session'}
    ]}
  ]})).entries[0];
  // Solo, explicit: the tag degrades to the provider name and the label
  // beside the logo disappears — exactly a default account's tile.
  const solo = model.tileParts(soloEntry('zai@1'), true, false, tileNow, true)[0];
  assert.equal(solo.text, 'zai');
  assert.equal(solo.logo, 'zai');
  assert.equal(solo.logoLabel, '');
  // Without the flag (an older caller) nothing changes.
  const keep = model.tileParts(soloEntry('zai@1'), true, false, tileNow)[0];
  assert.equal(keep.text, '1');
  assert.equal(keep.logoLabel, '1');

  const providerPair = model.parseReport(JSON.stringify({entries: [
    {id: 'zai@1', logo: 'zai', error: null, sections: [
      {type: 'metric', label: 'Session (5h)', percent: 42, value: '42%',
       detail: '', severity: 'low', window: 'session'}
    ]},
    {id: 'zai@2', logo: 'zai', error: null, sections: [
      {type: 'metric', label: 'Session (5h)', percent: 10, value: '10%',
       detail: '', severity: 'low', window: 'session'}
    ]},
    {id: 'kimi@1', logo: 'kimi', error: null, sections: [
      {type: 'metric', label: 'Session (5h)', percent: 30, value: '30%',
       detail: '', severity: 'low', window: 'session'}
    ]}
  ]})).entries;
  // Two zai keys: suffixes stay (they are what distinguishes the tiles);
  // the lone kimi key drops its "1".
  assert.equal(model.tiledBarLabel(false, false, true, false, providerPair, false, tileNow),
    '1 42%·-│2 10%·-│kimi 30%·-');
  // An exhausted sibling still counts: zai@1 hidden, but zai@2 keeps "2".
  const exhaustedPair = model.parseReport(JSON.stringify({entries: [
    {id: 'zai@1', logo: 'zai', error: null, exhausted: true, sections: [
      {type: 'metric', label: 'Session (5h)', percent: 100, value: '100%',
       detail: '', severity: 'critical', window: 'session'}
    ]},
    {id: 'zai@2', error: null, sections: [
      {type: 'metric', label: 'Session (5h)', percent: 10, value: '10%',
       detail: '', severity: 'low', window: 'session'}
    ]}
  ]})).entries;
  assert.equal(model.tiledBarLabel(false, false, true, false, exhaustedPair, false, tileNow),
    '2 10%·-');
}

// Exhausted entries leave the bar but never the tabs: `exhausted` comes
// from Rust (any window maxed / balance spent), `barEntries` applies it,
// `readyEntries` (tabs) ignores it.
{
  const exhaustion = model.parseReport(JSON.stringify({entries: [
    {id: 'zai', error: null, exhausted: true, sections: [
      {type: 'metric', label: 'Session (5h)', percent: 100, value: '100%',
       detail: '', severity: 'critical', window: 'session'}
    ]},
    {id: 'kimi', error: null, sections: [
      {type: 'metric', label: 'Session (5h)', percent: 30, value: '30%',
       detail: '', severity: 'low', window: 'session'}
    ]},
    {id: 'deepseek', error: 'HTTP 401', sections: []}
  ]})).entries;
  assert.deepEqual(Array.from(model.barEntries(exhaustion), e => e.id), ['kimi']);
  assert.deepEqual(Array.from(model.readyEntries(exhaustion), e => e.id), ['zai', 'kimi'],
    'tabs keep the exhausted account');
  assert.deepEqual(Array.from(model.barTileEntries(exhaustion, 'kimi', true), e => e.id), ['kimi']);
  // Tiling off: the SELECTED entry alone — and an exhausted selection is
  // skipped for the first tileable account, never rendered as a tile.
  assert.deepEqual(Array.from(model.barTileEntries(exhaustion, 'zai', false), e => e.id), ['kimi']);
  assert.deepEqual(Array.from(model.barTileEntries(exhaustion, 'kimi', false), e => e.id), ['kimi']);
  assert.deepEqual(Array.from(model.barTileEntries(exhaustion, 'missing', false), e => e.id), ['kimi']);
  assert.equal(model.barTileEntries([], 'x', false).length, 0);
  // The tiled label itself hides the exhausted tile.
  assert.equal(model.tiledBarLabel(false, false, true, false, exhaustion, false, tileNow),
    'kimi 30%·-');
  assert.equal(model.tiledBarLabel(false, false, true, false, [exhaustion[0]], false, tileNow),
    '󰚩', 'every tile gone → the robot alone');
}

// Fresh install: a report where EVERY entry is unconfigured has no tab and
// no tile; the panel opens straight into settings (source assertions below).
{
  const fresh = model.parseReport(JSON.stringify({entries: [
    {id: 'zai', error: 'credentials error: Zai: no API key. Either …', unconfigured: true, sections: []},
    {id: 'openai', error: 'credentials error: Codex: no auth file at …', unconfigured: true, sections: []}
  ]})).entries;
  assert.equal(model.readyEntries(fresh).length, 0);
  assert.equal(model.barEntries(fresh).length, 0);
}

// Per-agent tile colors: the least remaining across shown windows drives
// the class; the colorFor callback renders rich text (HTML-escaped).
{
  const mk = (p1, p2) => model.parseReport(JSON.stringify({entries: [{
    id: 'zai@a', error: null, sections: [
      {type: 'metric', label: 'Session (5h)', percent: p1, value: p1 + '%',
       detail: '', severity: 'low', reset_at: resetAt2h, window: 'session'},
      {type: 'metric', label: 'Weekly', percent: p2, value: p2 + '%',
       detail: '', severity: 'low', reset_at: resetAt2h, window: 'weekly'}
    ]}
  ]})).entries[0];
  assert.equal(model.tileRemainingClass(mk(42, 15)), 'low');
  assert.equal(model.tileRemainingClass(mk(60, 20)), 'mid');
  assert.equal(model.tileRemainingClass(mk(91, 15)), 'high');
  assert.equal(model.tileRemainingClass(mk(30, 96)), 'critical');
  assert.equal(model.tileRemainingClass(mk(50, 50)), 'low');

  // Per-FIGURE classes: the tag is class-less (theme foreground — the key
  // name never changes color with usage), then the 5h and weekly figures
  // each carry their own band — 99% left of the 5h window (green) beside
  // 48% left of the weekly one (yellow) in ONE tile.
  // (Plain-copy first: parts live in the vm context, whose object prototype
  // never deep-equals a host-realm literal.)
  const plain = parts => {
    const out = []
    for (const p of parts) out.push({ text: p.text, cls: p.cls })
    return out
  };
  assert.deepEqual(plain(model.tileParts(mk(1, 52), true, false, tileNow)), [
    { text: 'a', cls: '' },
    { text: '1%·2h', cls: 'low' },
    { text: '52%·2h', cls: 'mid' }
  ]);
  // Band edges on remaining: 13% left → orange (high), 47% left → yellow.
  assert.equal(model.remainingClass(87), 'high'); // 13% remaining
  assert.equal(model.remainingClass(53), 'mid');  // 47% remaining
  assert.equal(model.remainingClass(81), 'high'); // 19% remaining — orange's upper edge
  assert.equal(model.remainingClass(80), 'mid');  // 20% remaining — back to yellow
  // And the mirror: a critical 5h window beside a healthy weekly one —
  // the split goes both ways, never one color for the whole key.
  const mirror = plain(model.tileParts(mk(96, 8), true, false, tileNow));
  assert.deepEqual(mirror[1], { text: '96%·2h', cls: 'critical' });
  assert.deepEqual(mirror[2], { text: '8%·2h', cls: 'low' });
  // tileLabel stays the joined form of the parts.
  assert.equal(model.tileLabel(mk(1, 52), true, false, tileNow), 'a 1%·2h 52%·2h');
  // showRemaining flips the figures; the classes stay remaining-based.
  const flipped = plain(model.tileParts(mk(1, 52), true, true, tileNow));
  assert.deepEqual(flipped[1], { text: '99%·2h', cls: 'low' });
  assert.deepEqual(flipped[2], { text: '48%·2h', cls: 'mid' });
  // Balance-style accounts keep the single headline part, class-less.
  assert.deepEqual(plain(model.tileParts(model.parseReport(JSON.stringify({entries: [
    {id: 'deepseek', error: null, sections: [
      {type: 'text', label: 'Balance', value: '$8.42'}]}
  ]})).entries[0], true, false, tileNow)), [
    { text: 'deepseek', cls: '' },
    { text: '$8.42', cls: '' }
  ]);

  // One-unit countdowns, mirroring Rust's `countdown::format_compact`:
  // minutes under an hour, decimal hours under a day, decimal days above.
  assert.equal(model.compactReset('', tileNow), '');
  assert.equal(model.compactReset('2026-08-29T11:00:00Z', tileNow), 'due');
  assert.equal(model.compactReset('2026-08-29T12:40:00Z', tileNow), '40m');
  assert.equal(model.compactReset('2026-08-29T12:00:30Z', tileNow), '1m');
  assert.equal(model.compactReset('2026-08-29T13:00:00Z', tileNow), '1h');
  assert.equal(model.compactReset('2026-08-29T14:05:00Z', tileNow), '2h');
  assert.equal(model.compactReset('2026-08-29T16:07:00Z', tileNow), '4.1h');
  assert.equal(model.compactReset('2026-08-29T17:12:00Z', tileNow), '5.2h');
  assert.equal(model.compactReset('2026-08-29T11:59:59Z', tileNow), 'due');
  assert.equal(model.compactReset('2026-08-30T11:59:00Z', tileNow), '23.9h');
  assert.equal(model.compactReset('2026-08-30T12:00:00Z', tileNow), '1d');
  assert.equal(model.compactReset('2026-09-01T14:05:00Z', tileNow), '3d');
  assert.equal(model.compactReset('2026-09-04T15:05:00Z', tileNow), '6.1d');

  // The worst band across entries drives the vertical bar's severity dot;
  // non-ready entries never contribute (the icon itself goes urgent there).
  assert.equal(model.worstBand([]), '');
  assert.equal(model.worstBand([mk(10, 5)]), 'low');
  assert.equal(model.worstBand([mk(10, 5), mk(60, 5)]), 'mid');
  assert.equal(model.worstBand([mk(1, 5), mk(60, 96)]), 'critical');
  const brokenEntry = model.parseReport(JSON.stringify({entries: [
    {id: 'kimi', error: 'HTTP 401', sections: []}
  ]})).entries[0];
  assert.equal(model.worstBand([brokenEntry, mk(10, 5)]), 'low');
}
// The bar renders ONE PART PER TILE (Panel.barLabelModels + BarWidget's
// Repeater) — the tag and each window figure are separate objects with
// their own color property, not shared rich text whose spans a CSS quirk
// can drop wholesale (the all-white episode).
assert.match(panelSource, /function barLabelModels\(\)/);
assert.match(panelSource, /Model\.tileParts\(/);
// Bar tiles come from barTileEntries: tiling honors the setting, and an
// exhausted account never tiles.
assert.match(panelSource, /Model\.barTileEntries\(visibleEntries, selectedEntryId, barTiled\)/);
// A provider's only key drops its suffix — counts come from the report.
assert.match(panelSource, /Model\.providerEntryCounts\(visibleEntries\)/);
assert.match(panelSource, /var solo = counts\[Model\.baseProvider\(working\[w\]\.id\)\] <= 1/);
// Icon policy: the tiled bar starts straight at the tiles (every tile leads
// with its vendor logo — a module robot in front is pure redundancy), and
// single-account mode (tiling off) is the robot ALONE.
assert.match(panelSource, /if \(!barTiled\) \{\s*\n\s*\/\/ Not tiling: the robot alone/);
assert.match(panelSource, /if \(items\.length > 0\) push\("│", root\.dim, "sep"\)/);
// The robot glyph NEVER swaps to the alert mark — not even at zero
// remaining; alarms travel through the urgent color only.
assert.doesNotMatch(panelSource, /󰅙/);
// Tile dividers carry almost no padding: 2px each side in BarWidget, and
// the pure-string join has no spaces at all.
assert.match(barWidgetSource, /role === "sep" \? 2/);
// Fresh install: a report where every entry is unconfigured opens the
// settings form once — the form IS the panel content until a key lands.
assert.match(panelSource, /function maybeAutoOpenSettings\(\)/);
assert.match(panelSource, /autoSettingsDone = true/);
assert.match(panelSource, /status !== "unconfigured"/);
assert.match(barWidgetSource, /labelVisible: false/);
assert.match(barWidgetSource, /keepSpace: true/);
assert.match(barWidgetSource, /root\.panelItem\.barLabelModels\(\)/);
assert.match(barWidgetSource, /color: modelData\.color/);
// The tag part renders as the vendor's LOGO when a bundled asset exists
// (named accounts keep their suffix text beside it); anything else is the
// plain text fallback — the tile never blanks out over a missing file.
assert.match(barWidgetSource, /import "Model\.js" as Model/);
assert.match(barWidgetSource, /Model\.logoAssetName\(tilePart\.logo\)/);
assert.match(barWidgetSource, /Qt\.resolvedUrl\("logos\/" \+ Model\.logoAssetName\(tilePart\.logo\)\)/);

// The hero's trailing buttons keep clear of the scrollbar overlay.
assert.match(panelSource, /width: Style\.space\(6\)\n\s+height: 1/);
assert.match(panelSource, /property real savedSettingsScrollY: 0/);
assert.match(panelSource, /savedSettingsScrollY = panelFlick\.contentY/);
assert.match(panelSource, /panelFlick\.contentY = Math\.min\(root\.savedSettingsScrollY/);
// Each draft carries its own Apply button right under the key field — the
// global save at the page bottom is below the fold exactly when it matters.
assert.match(settingsViewSource, /text: root\.saving \? "Saving…" : "Apply"/);
// Drafts no longer name accounts client-side: the form asks for no name and
// the server auto-names on add (Z.AI, Z.AI 2 …). The old pre-naming
// machinery must not creep back.
assert.doesNotMatch(settingsViewSource, /function nextDraftName/);
assert.doesNotMatch(settingsViewSource, /draftName/);
assert.match(settingsViewSource, /action: "add", vendor: draftVendor, fields: fields/);
// Accounts render in ONE root-level section (zai's proven shape): no
// per-vendor delegate sections — those hid `accountRepeater` from root
// functions and silently killed every save (and the back button via
// scrubSecrets). Both vendors add from their own button.
assert.equal(settingsViewSource.indexOf('id: vendorSection'), -1,
  'per-vendor delegate sections must not come back');
assert.match(settingsViewSource, /text: "ACCOUNTS"/);
// ONE add button PER VENDOR, right under that vendor's last key — the
// button label comes from Rust's vendor_display, never a frontend table.
assert.doesNotMatch(settingsViewSource, /text: "Add account"/);
assert.match(settingsViewSource, /"Add " \+ row\.modelData\.vendorDisplay \+ " Account"/);
assert.match(settingsViewSource, /onClicked: root\.addDraftAccount\(row\.modelData\.vendor\)/);
// The draft's vendor is fixed by its button — no in-draft provider picker.
assert.doesNotMatch(settingsViewSource, /function switchVendor/);
// Card mutations carry the card's own vendor — the hardcoded "zai" here
// once wrote kimi keys into the [zai] section.
assert.doesNotMatch(settingsViewSource, /vendor: shape\.card\.vendor \|\|/);
assert.match(settingsViewSource, /vendor: shape\.card\.vendor/);
// Card titles render Rust's vendor_display — no zai/kimi string table here.
assert.doesNotMatch(settingsViewSource, /"Kimi \. " : "Z\.AI \. "/);
// Drafts key by vendor section id; the array only changes on clicks.
assert.match(settingsViewSource, /drafts = drafts\.concat\(\[vendor\]\)/);
assert.match(settingsViewSource, /function removeDraftAt\(index\)/);
// Zero accounts still shows the section (its Add buttons are the entry
// point when nothing is configured).
{
  const at = settingsViewSource.indexOf('text: "ACCOUNTS"');
  assert.match(settingsViewSource.slice(at - 400, at), /visible: !root\.loading/,
    'accounts section must not require accounts to exist');
}
assert.match(settingsViewSource, /label:\s*"Show remaining instead of used"/);
// Tiling is a visible toggle again (default on); the value toggle stays out.
assert.match(settingsViewSource, /label:\s*"Tile every account in the top bar"/);
assert.doesNotMatch(settingsViewSource, /label:\s*"Show usage value in the top bar"/);
// The plain API KEYS section is gone — every key vendor is an account card.
assert.doesNotMatch(settingsViewSource, /text: "API KEYS"/);
assert.doesNotMatch(settingsViewSource, /snapshot\.keys/);
// One vendor-grouped model drives the whole section; group prefix bars
// alternate two colors by GROUP INDEX (Z.AI blue, Kimi purple, then
// alternating down the list) — not by a per-vendor table.
assert.match(settingsViewSource, /function accountsModel\(\)/);
assert.match(settingsViewSource, /model: root\.accountsModel\(\)/);
assert.match(settingsViewSource, /g % 2 === 0 \? "#61afef" : "#c678dd"/);
assert.match(settingsViewSource, /color: row\.modelData\.accent/);
assert.doesNotMatch(settingsViewSource, /function vendorAccent/);
assert.doesNotMatch(settingsViewSource, /Nous/);
assert.doesNotMatch(panelSource, /openNousLogin/);
assert.doesNotMatch(settingsViewSource, /command:\s*\[[^\]]*(?:api.?key|secret|pendingPayload)/i);
// Panel tabs list only agents that read; selection prefers them too.
assert.deepEqual(Array.from(model.readyEntries(unconfiguredReport)).map(e => e.id),
  ['zai@Z.AI']);
// A remembered or primary id that does not read yields to the first
// working account; a ready remembered id still wins.
assert.equal(model.preferredEntryId(unconfiguredReport, 'zai', 'zai'), 'zai@Z.AI');
assert.equal(model.preferredEntryId(unconfiguredReport, 'zai', 'kimi-missing'), 'zai@Z.AI');
const readyFirst = model.parseReport(JSON.stringify({entries: [{
  id: 'zai', error: 'no API key …', unconfigured: true, sections: []
}, {
  id: 'kimi', error: null, sections: []
}]})).entries;
assert.equal(model.preferredEntryId(readyFirst, '', ''), 'kimi');
assert.equal(model.preferredEntryId(readyFirst, '', 'zai'), 'kimi');
// Unconfigured entries tile no more than errors do — the panel keeps them.
assert.equal(model.tiledBarLabel(false, false, true, false, unconfiguredReport, false, tileNow),
  'Z.AI 42%·-');

// Misconfigured entries (no key, broken config) stay OFF the bar…
const mixedEntries = model.parseReport(JSON.stringify({entries: [
  {id: 'kimi', short_name: 'kmi', error: 'HTTP 401', sections: []},
  {id: 'zai', short_name: 'zai', error: null, sections: [
    {type: 'metric', label: 'Session (5h)', percent: 42, value: '42%', detail: '', severity: 'low', window: 'session'}
  ]}
]})).entries;
assert.equal(model.tiledBarLabel(false, false, true, false, mixedEntries, false, tileNow),
  'zai 42%·-');
// …and when every account is broken, the alert icon alone points at the panel.
assert.equal(model.tiledBarLabel(false, false, true, false, [mixedEntries[0]], false, tileNow), '󰚩');
// Empty/loading/vertical states mirror barLabel's.
assert.equal(model.tiledBarLabel(false, false, true, true, [], false, tileNow), '󰚩  …');
assert.equal(model.tiledBarLabel(true, false, true, false, [], false, tileNow), '󰚩');
assert.equal(model.tiledBarLabel(true, true, true, false, parsed.entries, false, tileNow), '󰚩');
assert.equal(model.tiledBarLabel(false, false, false, false, parsed.entries, false, tileNow),
  'anthropic│openai');

// The codes come from Rust's VendorId::short_name via the report; the vendor
// half of the machine id only stands in for a binary that predates the field.
assert.equal(model.providerShort(parsed.entries[0]), 'cld');
assert.equal(model.providerShort(parsed.entries[1]), 'gpt');
assert.equal(model.providerShort({id: 'anthropic@work'}), 'anthropic');
assert.equal(model.providerShort({id: 'anthropic_api'}), 'anthropic-api');
assert.equal(model.providerShort({id: 'zai', short_name: '   '}), 'zai');
assert.equal(model.providerShort(null), '');
// Provider-controlled text can never reach Text.AutoText as markup.
assert.equal(model.providerShort({id: 'x', short_name: '<b>x</b>'}), '‹b›x‹/b›');

assert.equal(model.headline(parsed.entries[0]).text, '29%');
assert.equal(model.headline(parsed.entries[1]).severity, 'critical');
assert.equal(model.isAlarming(parsed.entries[0]), true); // stale
assert.equal(model.isAlarming(parsed.entries[1]), true); // critical
// Reset-row fixtures are built from *local* calendar components, not UTC
// strings, so every expectation below is a literal that holds in any
// timezone the panel might run in. Deriving the expected clock from the same
// getHours()/getMinutes() expression the implementation uses would pass no
// matter what that expression did.
const localReset = (y, mo, d, h, mi) => new Date(y, mo - 1, d, h, mi).toISOString();
const at = (y, mo, d, h, mi) => Date.parse(new Date(y, mo - 1, d, h, mi).toISOString());

// Same local day: the clock alone is unambiguous.
assert.equal(model.formatReset(localReset(2026, 8, 14, 22, 0), at(2026, 8, 14, 8, 0)),
  'Resets in 14h 0m · 22:00');
// Both fields zero-padded.
assert.equal(model.formatReset(localReset(2026, 8, 14, 9, 5), at(2026, 8, 14, 8, 0)),
  'Resets in 1h 5m · 09:05');
// Under 24h but past midnight: the date is what stops "03:00" reading as a
// time that already went by this morning.
assert.equal(model.formatReset(localReset(2026, 8, 15, 3, 0), at(2026, 8, 14, 20, 0)),
  'Resets in 7h 0m · Aug 15 03:00');
// Long windows carry the date too.
assert.equal(model.formatReset(localReset(2026, 9, 14, 14, 30), at(2026, 8, 14, 12, 0)),
  'Resets in 31d 2h · Sep 14 14:30');
// Day-of-month is not padded, matching the rest of the row's typography.
assert.equal(model.formatReset(localReset(2026, 9, 5, 14, 0), at(2026, 8, 14, 12, 0)),
  'Resets in 22d 2h · Sep 5 14:00');
assert.equal(model.formatReset('2026-08-14T12:00:00Z', Date.parse('2026-08-14T12:00:00Z')), 'Reset due');
assert.equal(model.formatReset('', Date.parse('2026-08-14T12:00:00Z')), '');
assert.equal(model.formatReset('not-a-date', Date.parse('2026-08-14T12:00:00Z')), '');
assert.equal(model.formatUpdated('2026-08-14T12:00:00Z', Date.parse('2026-08-14T12:03:00Z')), 'Updated 3m ago');
assert.equal(model.metricDetail(parsed.entries[0].sections[1]), '60% elapsed · 31pts under');

const balance = model.parseReport(JSON.stringify({entries: [{
  id: 'deepseek', error: null,
  sections: [{type: 'text', label: 'Balance', value: '$8.42'}]
}]})).entries[0];
assert.equal(model.headline(balance).text, '$8.42');
const meteredBalance = model.parseReport(JSON.stringify({entries: [{
  id: 'openrouter', error: null,
  sections: [{type: 'metric', label: 'Credit balance', percent: 25, value: '$75.00', detail: ''}]
}]})).entries[0];
assert.equal(model.headline(meteredBalance).text, '$75.00');

assert.equal(model.parseReport('{').ok, false);
assert.equal(model.parseReport('{}').ok, false);
assert.equal(model.parseReport('{"entries":[{"name":"missing id"}]}').ok, false);
assert.equal(model.cleanText('bad\u0000value', 20), 'badvalue');
assert.equal(model.cleanText('tab\tcarriage\rC1\u0085value', 40), 'tab carriage C1value');
assert.equal(model.cleanText('😀😀', 3), '😀…');
assert.equal(model.autoTextSafe('<img src="https://example.test/pixel">'),
  '‹img src="https://example.test/pixel"›');
assert.equal(model.autoTextSafe('line\nspoof\u202eright-to-left'), 'line spoofright-to-left');
assert.equal(model.providerName({id: 'anthropic', display_name: 'Claude · <b>work</b>'}),
  'Claude · ‹b›work‹/b›');
assert.equal(model.providerName({id: 'openai', name: 'openai'}), 'openai');
assert.equal(model.errorMessage(''), 'The usage command failed without an error message.');

// A missing ai-usagebar binary must be reported as such, with the install
// command, instead of surfacing the helper's raw "not found" text or leaving
// the widget silently stuck on its loading state.
assert.match(model.launchErrorMessage(127, 'env: ai-usagebar-omarchy: No such file or directory'),
  /ai-usagebar-omarchy is not installed/);
assert.match(model.launchErrorMessage(127, ''), /cargo install --git https:\/\/github\.com\/KyleLee\/ai-usagebar-omarchy/);
assert.match(model.launchErrorMessage(127, ''), /rustup\.rs/);
// Every other failure keeps the existing behaviour.
assert.equal(model.launchErrorMessage(1, 'boom'), 'boom');
assert.equal(model.launchErrorMessage(0, ''), 'The usage command failed without an error message.');

// The usage command must stay behind a helper that can emit exit 127 when the
// binary is absent, without opening a shell-injection boundary.
assert.match(panelSource,
  /command:\s*\["\/usr\/bin\/env",\s*"ai-usagebar-omarchy",\s*"usage",\s*"--json"\]/);
assert.doesNotMatch(panelSource, /command:\s*\["(?:\/usr\/bin\/)?(?:ba)?sh"/);
assert.match(panelSource, /onExited:\s*function\(exitCode\)/);
assert.match(panelSource, /Model\.launchErrorMessage\(/);

const settingsRaw = JSON.stringify({
  schema_version: 1,
  primary: 'openai',
  primary_choices: [
    {id: 'anthropic', label: 'Claude'},
    {id: 'openai', label: 'Codex'}
  ],
  keys: [
    {id: 'kimi', label: 'Kimi', environment: 'KIMI_API_KEY', note: 'coding-plan usage',
     configured: true, inline_configured: true, environment_configured: false}
  ]
});
const settingsRawWithFields = JSON.stringify({
  schema_version: 1,
  primary: 'zai',
  primary_choices: [{id: 'zai', label: 'Z.AI'}],
  keys: [
    {id: 'zai', label: 'Z.AI', environment: 'ZAI_API_KEY', note: '', configured: true,
     inline_configured: true, environment_configured: false,
     fields: [
       {id: 'account_type', label: 'Account type', kind: 'choice', value: 'team',
        choices: ['personal', 'team', 'usage']},
       {id: 'site', label: 'Site', kind: 'choice', value: '', choices: ['', 'global', 'cn']},
       {id: 'organization_id', label: 'Organization ID (team)', kind: 'text', value: 'org-1'},
       {id: 'project_id', label: 'Project ID (team)', kind: 'text', value: ''}
     ]},
    {id: 'kimi', label: 'Kimi', environment: 'KIMI_API_KEY', note: 'coding-plan usage',
     configured: true, inline_configured: true, environment_configured: false}
  ]
});
const settingsWithFields = model.parseSettingsSnapshot(settingsRawWithFields);
assert.equal(settingsWithFields.ok, true);
assert.equal(settingsWithFields.keys[0].fields.length, 4);
assert.deepEqual(Array.from(settingsWithFields.keys[0].fields[0].choices), ['personal', 'team', 'usage']);
assert.equal(settingsWithFields.keys[0].fields[2].value, 'org-1');
assert.deepEqual(Array.from(settingsWithFields.keys[1].fields), []);
// A malformed field row is dropped, not fatal.
const badField = model.parseSettingsSnapshot(JSON.stringify({
  schema_version: 1, primary_choices: [], keys: [
    {id: 'zai', fields: [{id: 'bad id!', kind: 'text', value: 'x'}, {id: 'site', kind: 'choice', value: 'cn', choices: ['', 'cn']}]}
  ]
}));
assert.equal(badField.ok, true);
assert.deepEqual(Array.from(badField.keys[0].fields).map(f => f.id), ['site']);

// --- Z.AI account list (multi-account settings bridge) ----------------------
const accountsRaw = JSON.stringify({
  schema_version: 1,
  primary: 'zai',
  primary_choices: [{id: 'zai', label: 'Z.AI'}],
  keys: [{id: 'deepseek', label: 'DeepSeek', environment: 'DEEPSEEK_API_KEY', note: '', configured: false,
    inline_configured: false, environment_configured: false}],
  accounts: [
    {vendor: 'zai', vendor_display: 'Z.AI', label: '', display: 'Default', environment: 'ZAI_API_KEY',
     configured: true, inline_configured: true, environment_configured: false,
     fields: [
       {id: 'site', label: 'Site', kind: 'choice', value: '', choices: ['', 'global', 'cn'],
        labels: {'': 'auto', global: 'z.ai', cn: 'bigmodel.cn'}},
       {id: 'account_type', label: 'Account type', kind: 'choice', value: 'personal',
        choices: ['personal', 'team', 'usage']},
       {id: 'api_key', label: 'API key', kind: 'secret', value: ''}
     ]},
    {vendor: 'zai', vendor_display: 'Z.AI', label: 'team', display: 'team', environment: '(per-account env or inline)',
     configured: false, inline_configured: false, environment_configured: false,
     fields: [
       {id: 'name', label: 'Name', kind: 'text', value: 'team'},
       {id: 'site', label: 'Site', kind: 'choice', value: 'cn', choices: ['', 'global', 'cn'],
        labels: {cn: 'bigmodel.cn'}},
       {id: 'account_type', label: 'Account type', kind: 'choice', value: 'team',
        choices: ['personal', 'team', 'usage']},
       {id: 'organization_id', label: 'Organization ID', kind: 'text', value: 'org-1'},
       {id: 'project_id', label: 'Project ID', kind: 'text', value: ''},
       {id: 'api_key', label: 'API key', kind: 'secret', value: ''}
     ]}
  ]
});
const accountsParsed = model.parseSettingsSnapshot(accountsRaw);
assert.equal(accountsParsed.ok, true);
assert.equal(accountsParsed.accounts.length, 2);
assert.equal(accountsParsed.accounts[0].label, '');
assert.equal(accountsParsed.accounts[0].display, 'Default');
assert.equal(accountsParsed.accounts[0].vendor_display, 'Z.AI');
assert.equal(accountsParsed.accounts[0].fields[2].kind, 'secret');
assert.equal(accountsParsed.accounts[1].label, 'team');
assert.equal(accountsParsed.accounts[1].fields[0].id, 'name');
assert.equal(accountsParsed.accounts[1].fields[0].value, 'team');
// Site labels ride along for the dropdowns.
assert.equal(accountsParsed.accounts[0].fields[0].labels.cn, 'bigmodel.cn');
assert.equal(accountsParsed.accounts[0].fields[0].labels.global, 'z.ai');
// A malformed account row is dropped, not fatal.
const badAccount = model.parseSettingsSnapshot(JSON.stringify({
  schema_version: 1, primary_choices: [], keys: [],
  accounts: [{vendor: 'bad id!', label: 'x'}, {vendor: 'zai', label: 'ok'}]
}));
assert.equal(badAccount.ok, true);
assert.deepEqual(Array.from(badAccount.accounts).map(a => a.label), ['ok']);

// Account mutations build a valid patch: update with fields+key, add, remove.
const accountPatch = model.buildSettingsPatch('', [], [
  {action: 'update', vendor: 'zai', label: 'team',
   fields: {organization_id: 'org-2'}, apiKey: {action: 'set', value: 'k'}},
  {action: 'add', vendor: 'zai', name: 'payg', fields: {account_type: 'usage'}},
  {action: 'remove', vendor: 'zai', label: 'old'}
]);
assert.equal(accountPatch.ok, true);
assert.deepEqual(JSON.parse(accountPatch.payload).accounts.zai, [
  {action: 'update', label: 'team', fields: {organization_id: 'org-2'},
   api_key: {action: 'set', value: 'k'}},
  {action: 'add', name: 'payg', fields: {account_type: 'usage'}},
  {action: 'remove', label: 'old'}
]);
// An add without a name (or with whitespace) goes through name-less: the
// server auto-names it. The patch carries no `name` for those.
const namelessAdd = model.buildSettingsPatch('', [], [
  {action: 'add', vendor: 'zai', name: '  '}
]);
assert.equal(namelessAdd.ok, true);
assert.deepEqual(JSON.parse(namelessAdd.payload).accounts.zai,
  [{action: 'add'}]);
const bareAdd = model.buildSettingsPatch('', [], [
  {action: 'add', vendor: 'kimi', apiKey: {action: 'set', value: 'k'}}
]);
assert.equal(bareAdd.ok, true);
assert.deepEqual(JSON.parse(bareAdd.payload).accounts.kimi,
  [{action: 'add', api_key: {action: 'set', value: 'k'}}]);
// Default-section updates use the empty label.
const defaultUpdate = model.buildSettingsPatch('', [], [
  {action: 'update', vendor: 'zai', label: '', fields: {account_type: 'team'}}
]);
assert.deepEqual(JSON.parse(defaultUpdate.payload).accounts.zai,
  [{action: 'update', label: '', fields: {account_type: 'team'}}]);

const settings = model.parseSettingsSnapshot(settingsRaw);
assert.equal(settings.ok, true);
assert.equal(settings.primary, 'openai');
assert.equal(settings.primary_choices[0].id, 'anthropic');
assert.equal(settings.primary_choices[0].value, 'anthropic');
assert.equal(settings.primary_choices[0].label, 'Claude');
assert.equal(settings.primary_choices[1].label, 'Codex');
assert.equal(settings.keys[0].inline_configured, true);
assert.equal(settings.keys[0].environment, 'KIMI_API_KEY');
const opencodeSettings = model.parseSettingsSnapshot(JSON.stringify({
  schema_version: 1,
  primary: 'opencode-go',
  primary_choices: [{id: 'opencode-go', label: 'OpenCode Go'}],
  keys: [{id: 'opencode-go', label: 'OpenCode Go', environment: 'OPENCODE_GO_API_KEY',
    note: 'usage quota', configured: false, inline_configured: false, environment_configured: false}]
}));
assert.equal(opencodeSettings.ok, true);
assert.equal(opencodeSettings.primary, 'opencode-go');
assert.equal(opencodeSettings.keys[0].id, 'opencode-go');
assert.equal(opencodeSettings.keys[0].environment, 'OPENCODE_GO_API_KEY');
assert.equal(model.parseSettingsSnapshot('{').ok, false);
assert.equal(model.parseSettingsSnapshot(JSON.stringify({schema_version: 2, primary_choices: [], keys: []})).ok, false);
const noEnabled = model.parseSettingsSnapshot(JSON.stringify({
  schema_version: 1, primary: 'anthropic', primary_choices: [], keys: []
}));
assert.equal(noEnabled.ok, true);
assert.equal(noEnabled.primary, '');

const patch = model.buildSettingsPatch('openai', [
  {id: 'kimi', action: 'set', value: 'secret-value'},
  {id: 'zai', action: 'clear'}
]);
assert.equal(patch.ok, true);
assert.deepEqual(JSON.parse(patch.payload), {
  schema_version: 1,
  primary: 'openai',
  keys: {
    kimi: {action: 'set', value: 'secret-value'},
    zai: {action: 'clear'}
  }
});
// The Z.AI field enrichment: fields ride along with a key change, or stand
// alone as a fields-only mutation the Rust bridge understands.
const teamPatch = model.buildSettingsPatch('', [
  {id: 'zai', action: 'set', value: 'k', fields: {
    account_type: 'team', site: 'cn', organization_id: 'org-1', project_id: 'proj-1'}}
]);
assert.equal(teamPatch.ok, true);
assert.deepEqual(JSON.parse(teamPatch.payload).keys.zai, {
  action: 'set', value: 'k',
  fields: {account_type: 'team', site: 'cn', organization_id: 'org-1', project_id: 'proj-1'}
});
const fieldsOnly = model.buildSettingsPatch('', [
  {id: 'zai', action: 'fields', fields: {account_type: 'usage'}}
]);
assert.deepEqual(JSON.parse(fieldsOnly.payload).keys.zai,
  {action: 'fields', fields: {account_type: 'usage'}});
// Empty field maps collapse away — no `fields: {}` in the payload.
const noFields = model.buildSettingsPatch('', [
  {id: 'kimi', action: 'set', value: 'k', fields: {}}
]);
assert.deepEqual(JSON.parse(noFields.payload).keys.kimi, {action: 'set', value: 'k'});
// Invalid field ids and oversized values are rejected before stdin.
assert.equal(model.buildSettingsPatch('', [
  {id: 'zai', action: 'fields', fields: {'bad id!': 'x'}}
]).ok, false);
assert.equal(model.buildSettingsPatch('', [
  {id: 'zai', action: 'fields', fields: {organization_id: 'x'.repeat(201)}}
]).ok, false);
assert.equal(model.buildSettingsPatch('', [
  {id: 'zai', action: 'fields', fields: {}}
]).ok, false);
const keyOnlyPatch = model.buildSettingsPatch('', [{id: 'kimi', action: 'clear'}]);
assert.deepEqual(JSON.parse(keyOnlyPatch.payload), {
  schema_version: 1, keys: {kimi: {action: 'clear'}}
});
assert.equal(model.buildSettingsPatch('', []).ok, false);
assert.equal(model.buildSettingsPatch('openai', [{id: '__proto__', action: 'clear'}]).ok, false);
assert.equal(model.buildSettingsPatch('openai', [{id: 'kimi', action: 'set', value: ''}]).ok, false);
assert.equal(model.buildSettingsPatch('openai', [{id: 'kimi', action: 'bogus'}]).ok, false);
assert.equal(model.parseSettingsApplyResult('{"ok":true}'), true);
assert.equal(model.parseSettingsApplyResult('{"ok":false}'), false);

console.log('Omarchy model tests passed');
