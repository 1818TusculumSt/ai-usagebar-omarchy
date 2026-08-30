import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Native Omarchy Quattro popup. BarWidget.qml owns the bar slot and injects
// its button as this panel's anchor; collection stays in the Rust binary.
Panel {
  id: root
  moduleName: "ai-usagebar-omarchy"
  manageIpc: false

  property var anchorItem: null
  property var hostWidget: null
  readonly property var barIdentity: hostWidget || root

  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.45)
  // Quota range colors (One Dark family; the palette ships none of them).
  // Red stays the theme's `urgent`; these read on light and dark bars alike.
  readonly property color quotaGreen: "#98c379"
  readonly property color quotaYellow: "#e5c07b"
  readonly property color quotaOrange: "#d08770"
  readonly property color track: Style.selectedFillFor(foreground, Color.accent)

  // The ONE band palette shared by the bar tiles, the panel's metric rows,
  // and the vertical severity dot — the surfaces can never fork.
  function bandColor(band) {
    if (band === "critical") return root.urgent
    if (band === "high") return root.quotaOrange
    if (band === "mid") return root.quotaYellow
    if (band === "low") return root.quotaGreen
    return root.foreground
  }

  // The worst usage band across READY entries — drives the vertical bar's
  // severity dot (a broken account already flips the icon urgent).
  readonly property string worstBand: Model.worstBand(visibleEntries)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family
  readonly property bool vertical: bar ? bar.vertical : false

  property var entries: []
  property string primaryProvider: ""
  property string selectedEntryId: ""
  property string loadError: ""
  property string commandStderr: ""
  property string commandStdout: ""
  property bool loading: true
  property int lastExitCode: 0
  property bool refreshQueued: false
  property double lastSuccessfulMs: 0
  property double nowMs: Date.now()
  property bool cursorActive: false
  property bool settingsOpen: false

  // Default 60s: one poll per cache TTL — the binary serves cached data
  // within its 60s TTL anyway, so a faster poll would only burn CPU while
  // a slower one leaves the bar up to N minutes behind.
  readonly property int refreshIntervalSec: Math.max(30, Math.min(3600,
    Number(setting("refreshIntervalSec", 60)) || 60))
  readonly property string configuredProvider: String(setting("provider", "") || "").trim()
  readonly property string rememberedEntryId: String(setting("lastSelectedEntryId", "") || "").trim()
  // The one remaining display toggle: tiles show what is LEFT of each
  // window by default; this flips them to the USED percentage.
  readonly property bool showRemaining: Model.booleanSetting(setting("showRemaining", true), true)
  // Tile every working account side by side (default), or collapse the bar
  // to the SELECTED account alone — the wheel cycle and tabs stay the
  // selector. Off is the pre-tiling single-account presentation.
  readonly property bool barTiled: Model.booleanSetting(setting("barTiled", true), true)
  readonly property var visibleEntries: Model.filteredEntries(entries, configuredProvider)
  // Panel tabs show only agents that actually read; unconfigured/broken
  // ones keep their status hint but no tab.
  readonly property var tabEntries: Model.readyEntries(visibleEntries)
  readonly property int tabIndexOfSelected: Model.selectedIndex(tabEntries, selectedEntryId)
  readonly property int entryIndex: Model.selectedIndex(visibleEntries, selectedEntryId)
  readonly property var entry: entryIndex >= 0 ? visibleEntries[entryIndex] : null
  readonly property string entryFetchedAt: {
    if (!entry) return ""
    return String(entry.fetched_at || "")
  }
  readonly property var summary: Model.headline(entry)
  readonly property var entrySections: entry ? entry.sections : []
  readonly property bool filterMiss: configuredProvider !== "" && entries.length > 0 && visibleEntries.length === 0
  readonly property bool alarming: Model.isAlarming(entry) || loadError !== "" || filterMiss
  // Fresh install: every entry is "not configured yet" — the panel opens
  // straight into Settings (once; closing it is a choice, not a loop).
  property bool autoSettingsDone: false

  function alpha(color, opacity) {
    return Qt.rgba(color.r, color.g, color.b, opacity)
  }

  function clamp(value, low, high) {
    return Math.max(low, Math.min(high, value))
  }

  function syncSelection() {
    if (visibleEntries.length === 0) {
      selectedEntryId = ""
      return
    }
    for (var i = 0; i < visibleEntries.length; i++)
      if (visibleEntries[i].id === selectedEntryId) return
    selectedEntryId = Model.preferredEntryId(visibleEntries, primaryProvider, rememberedEntryId)
  }

  function restoreRememberedSelection() {
    if (visibleEntries.length === 0) return
    selectedEntryId = Model.preferredEntryId(visibleEntries, primaryProvider, rememberedEntryId)
  }

  function persistWidgetSettings(values) {
    // Quattro persists inline widget settings in shell.json and pushes them
    // live to every monitor. Keep every existing setting, including settings
    // introduced by future versions, and apply only the requested overrides.
    var entry = Model.settingsWithOverrides(root.settings, root.moduleName, values)
    if (!entry) return false

    // Apply locally first so controls remain responsive. Older compatible hosts
    // without updateEntryInline still retain the choice for this session.
    root.settings = entry
    if (hostWidget && "settings" in hostWidget) hostWidget.settings = entry
    if (bar && bar.shell && typeof bar.shell.updateEntryInline === "function")
      bar.shell.updateEntryInline(root.moduleName, entry)
    return true
  }

  function persistSelection(entryId) {
    if (String(entryId || "").trim() === rememberedEntryId) return
    persistWidgetSettings({ lastSelectedEntryId: entryId })
  }

  function setShowRemaining(enabled) {
    var next = enabled === true
    if (next === showRemaining) return
    persistWidgetSettings({ showRemaining: next })
  }

  function setBarTiled(enabled) {
    var next = enabled === true
    if (next === barTiled) return
    persistWidgetSettings({ barTiled: next })
  }

  function selectEntryById(id) {
    var index = Model.selectedIndex(visibleEntries, id)
    if (index >= 0) selectEntry(index)
  }

  function selectEntry(index) {
    if (visibleEntries.length === 0) return
    var wrapped = ((index % visibleEntries.length) + visibleEntries.length) % visibleEntries.length
    selectedEntryId = visibleEntries[wrapped].id
    persistSelection(selectedEntryId)
    if (providerList.visible) providerList.positionViewAtIndex(wrapped, ListView.Contain)
    if (panelFlick) panelFlick.contentY = 0
  }

  function startRefresh() {
    if (usageProcess.running) {
      refreshQueued = true
      return
    }
    refreshQueued = false
    commandStdout = ""
    commandStderr = ""
    if (entries.length === 0) loading = true
    usageProcess.running = true
  }

  function finishRefresh() {
    var parsed = Model.parseReport(commandStdout)
    if (parsed.ok) {
      primaryProvider = parsed.primary
      entries = parsed.entries
      loadError = ""
      lastSuccessfulMs = Date.now()
      syncSelection()
      maybeAutoOpenSettings()
    } else {
      var detail = commandStderr.trim()
      loadError = lastExitCode === 127
        ? Model.launchErrorMessage(lastExitCode, detail)
        : (detail !== "" ? Model.errorMessage(detail) : parsed.error)
    }
    loading = false
    if (refreshQueued) Qt.callLater(startRefresh)
  }

  // A fresh install reports every entry as "not configured yet": no tab to
  // show, nothing to read — the settings form IS the panel's content until
  // the first key lands. One-shot per panel lifetime; a deliberate close
  // stays closed, and broken (as opposed to unconfigured) entries keep the
  // status-hint flow.
  function maybeAutoOpenSettings() {
    if (autoSettingsDone || settingsOpen || entries.length === 0) return
    for (var i = 0; i < entries.length; i++)
      if (entries[i].status !== "unconfigured") return
    autoSettingsDone = true
    openSettings()
  }

  function refresh() { startRefresh() }

  function openSettings() {
    settingsOpen = true
    cursorActive = false
    if (panelFlick) panelFlick.contentY = 0
    Qt.callLater(function() { settingsView.forceActiveFocus() })
  }

  function closeSettings() {
    // Explicitly leaving the form discards pending secrets; the panel merely
    // closing (outside click, focus switch) must not.
    if (settingsView) settingsView.discard()
    settingsOpen = false
    savedSettingsScrollY = 0
    if (panelFlick) panelFlick.contentY = 0
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function openTerminalSettings() {
    if (hostWidget && typeof hostWidget.launchDashboard === "function")
      hostWidget.launchDashboard()
    else if (bar)
      bar.run("omarchy-launch-floating-terminal-with-presentation ai-usagebar-omarchy-tui")
  }

  function switchPanel(direction) {
    if (bar && typeof bar.switchPanelFrom === "function")
      return bar.switchPanelFrom(barIdentity, direction)
    return false
  }

  function statusMessage() {
    if (filterMiss) return "No configured entry matches ‘" + configuredProvider + "’. Clear the provider setting or use an id from ai-usagebar-omarchy usage --json."
    if (entry && entry.error !== "") return entry.error
    if (loadError !== "") return entries.length > 0
      ? "Refresh failed; showing the previous report. " + loadError
      : loadError
    if (entry && entry.stale) return "Cached data · the provider could not supply a fresh response."
    return ""
  }

  function statusIsUrgent() {
    // "Not configured yet" is a hint, not an alarm: routine absence keeps
    // the calm palette; only genuine errors go urgent.
    return filterMiss || (entry && entry.status === "error") || loadError !== ""
  }

  function heroMeta() {
    if (!entry) return loading ? "Loading providers" : "Usage report"
    if (entry.status === "error") return "Provider unavailable"
    if (entry.status === "unconfigured") return "Not configured yet"
    var text = entry.plan || "Usage and limits"
    if (entry.stale) text += " · cached"
    return Model.autoTextSafe(text)
  }

  // The bar label as one model per part — each part its own object, so
  // per-part styling is a plain color property (no rich-text CSS to fight
  // with). Items carry {text, color, separator, role, bold, logo, logoLabel}:
  // role is "icon" / "tag" / "figure" / "sep" so BarWidget can space
  // tile-internal parts tighter than tile boundaries; bold marks a critical
  // figure for colorblind aid; `logo` is the vendor's asset id — BarWidget
  // renders the image when the asset ships and falls back to `text`.
  // `logoLabel` is the account suffix a NAMED account keeps beside its
  // vendor logo. Tiles come from `Model.barTileEntries`: every working,
  // non-exhausted account while tiling, the selected one when tiling is off.
  //
  // Icon policy: the module robot is the widget's ONE glyph — it never
  // swaps to an alert mark, not even when usage is maxed to zero remaining;
  // alarms travel through COLOR (the urgent red) and the per-figure styling,
  // so the bar never looks like a different app when things run out.
  // TILED mode does not show the robot at all — every tile leads with its
  // vendor logo, which makes the icon pure redundancy; single-account mode
  // (tiling off) is the opposite minimalism: the robot ALONE, a status
  // indicator whose figures live in the tooltip and the click popup.
  // Fallback states (loading, nothing tileable, vertical) keep the robot.
  function barLabelModels() {
    var items = []
    var push = function(text, color, role, bold, separator, logo, logoLabel) {
      items.push({ text: text, color: color, separator: separator === true,
        role: role || "", bold: bold === true,
        logo: logo || "", logoLabel: logoLabel || "" })
    }
    var robot = function() {
      push("󰚩", alarming ? root.urgent : root.foreground, "icon")
    }
    if (vertical) {
      robot()
      return items
    }
    if (visibleEntries.length === 0) {
      if (loading) push("󰚩  …", alarming ? root.urgent : root.foreground, "icon")
      else robot()
      return items
    }
    var working = Model.barTileEntries(visibleEntries, selectedEntryId, barTiled)
    if (working.length === 0) {
      // Nothing tileable — every account is broken, unconfigured, or
      // exhausted. The robot stays calm unless something actually alarms:
      // routine absence (fresh install) keeps the theme foreground.
      robot()
      return items
    }
    if (!barTiled) {
      // Not tiling: the robot alone — no tiles, no figures. The selected
      // account's numbers stay one hover/click away.
      robot()
      return items
    }
    // Tiled: straight to the tiles (each leads with its vendor logo).
    // A provider's ONLY key drops its account suffix — a lone "1" beside
    // the logo has nothing to distinguish it from.
    var counts = Model.providerEntryCounts(visibleEntries)
    for (var w = 0; w < working.length; w++) {
      var solo = counts[Model.baseProvider(working[w].id)] <= 1
      var parts = Model.tileParts(working[w], true, showRemaining, nowMs, solo)
      if (parts.length === 0) continue
      if (items.length > 0) push("│", root.dim, "sep")
      // The tag part is always neutral (theme foreground); only the window
      // figures carry their own remaining-band class — critical ones also
      // go bold so the alert survives color blindness.
      for (var p = 0; p < parts.length; p++)
        push(parts[p].text, root.bandColor(parts[p].cls),
          p === 0 ? "tag" : "figure", parts[p].cls === "critical",
          false, parts[p].logo, parts[p].logoLabel)
    }
    return items
  }

  function tooltipText() {
    if (!entry) return Model.autoTextSafe(statusMessage() || "AI usage")
    var text = Model.providerName(entry)
    if (summary.text !== "") text += " · " + Model.autoTextSafe(summary.text)
    if (entry.stale) text += " · cached"
    return text
  }

  onEntriesChanged: Qt.callLater(syncSelection)
  onConfiguredProviderChanged: Qt.callLater(syncSelection)
  onRememberedEntryIdChanged: Qt.callLater(restoreRememberedSelection)
  property real savedSettingsScrollY: 0

  onOpenedChanged: {
    if (opened) {
      cursorActive = false
      nowMs = Date.now()
      if (panelFlick) panelFlick.contentY = 0
      // Returning to a still-open settings form goes back to where it was
      // scrolled — with many accounts the top is several screens away from
      // the card being edited. Applied post-layout so the Flickable has a
      // contentHeight to scroll within.
      if (settingsOpen) {
        Qt.callLater(function() {
          if (!root.opened || !panelFlick) return
          panelFlick.contentY = Math.min(root.savedSettingsScrollY,
            Math.max(0, panelFlick.contentHeight - panelFlick.height))
        })
      }
      if (lastSuccessfulMs === 0 || nowMs - lastSuccessfulMs >= refreshIntervalSec * 1000)
        startRefresh()
      Qt.callLater(function() { keyCatcher.forceActiveFocus() })
    } else {
      // Dismissal (focus loss): remember the scroll, keep the form.
      if (panelFlick) savedSettingsScrollY = panelFlick.contentY
    }
    // NOTE: the panel CLOSES on focus loss (so other windows receive
    // clicks), but `settingsOpen`, the form's pending input, and the scroll
    // position survive — reopening returns exactly where the user left off,
    // which is what makes the close-and-paste workflow usable.
  }

  Timer {
    interval: root.refreshIntervalSec * 1000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.startRefresh()
  }

  Timer {
    interval: 30000
    running: true
    repeat: true
    onTriggered: root.nowMs = Date.now()
  }

  Process {
    id: usageProcess
    running: false
    // /usr/bin/env always starts on Omarchy and reports a missing ai-usagebar-omarchy
    // as exit 127. Keep the command as structured argv: no shell is needed.
    command: ["/usr/bin/env", "ai-usagebar-omarchy", "usage", "--json"]

    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        root.commandStdout = text
      }
    }

    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.commandStderr = text
    }

    onExited: function(exitCode) {
      root.lastExitCode = exitCode
      // Let both waitForEnd collectors publish their buffers first.
      Qt.callLater(function() { root.finishRefresh() })
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    owner: root.barIdentity
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(390))
    contentHeight: panel.fittedContentHeight(column.implicitHeight, Style.space(640))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      // Native form controls own Tab/Enter/Esc while settings are open.
      blocked: root.settingsOpen

      onMoveRequested: function(dx, dy) {
        if (!root.settingsOpen && dx !== 0) {
          root.cursorActive = true
          root.selectEntry(root.entryIndex + dx)
        }
        if (dy !== 0)
          panelFlick.contentY = root.clamp(panelFlick.contentY + dy * Style.space(56), 0,
            Math.max(0, panelFlick.contentHeight - panelFlick.height))
      }
      onActivateRequested: if (!root.settingsOpen) root.refresh()
      onCloseRequested: root.settingsOpen ? root.closeSettings() : root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }
      onTextKey: function(text) {
        if (!root.settingsOpen && (text === "r" || text === "R")) root.refresh()
        else if (!root.settingsOpen && (text === "s" || text === "S")) root.openSettings()
      }

      Flickable {
        id: panelFlick
        anchors.fill: parent
        contentWidth: width
        contentHeight: column.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick
        interactive: contentHeight > height
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

        Column {
          id: column
          width: panelFlick.width
          spacing: Style.space(12)

          PanelHero {
            width: parent.width
            title: root.settingsOpen ? "Settings"
              : (root.entry ? Model.providerName(root.entry) : "AI usage")
            meta: root.settingsOpen ? "Display, provider & API keys" : root.heroMeta()
            detail: root.settingsOpen
              ? "Existing configuration stays in place until you save."
              : (root.entry && root.summary.text !== "Ready" ? Model.autoTextSafe(root.summary.text) : "")
            foreground: root.foreground
            fontFamily: root.fontFamily

            iconComponent: Component {
              Text {
                text: root.settingsOpen ? "󰒓" : "󰚩"
                color: root.alarming ? root.urgent : root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.display
              }
            }

            trailingControl: Component {
              Row {
                spacing: Style.space(4)

                PanelActionButton {
                  visible: !root.settingsOpen
                  iconText: "󰑐"
                  tooltipText: "Refresh usage"
                  foreground: root.foreground
                  fontFamily: root.fontFamily
                  enabled: !usageProcess.running
                  onClicked: root.refresh()
                }

                PanelActionButton {
                  iconText: root.settingsOpen ? "󰁍" : "󰒓"
                  tooltipText: root.settingsOpen ? "Back to usage" : "Settings"
                  foreground: root.foreground
                  fontFamily: root.fontFamily
                  onClicked: root.settingsOpen ? root.closeSettings() : root.openSettings()
                }

                // The Flickable's scrollbar overlays the panel's right edge;
                // without this spacer the last button sits under it — hard
                // to see, harder to click.
                Item {
                  width: Style.space(6)
                  height: 1
                }
              }
            }
          }

          SettingsView {
            id: settingsView
            visible: root.settingsOpen
            width: parent.width
            foreground: root.foreground
            urgent: root.urgent
            fontFamily: root.fontFamily
            showRemaining: root.showRemaining
            barTiled: root.barTiled
            onSaved: root.startRefresh()
            onShowRemainingRequested: function(enabled) { root.setShowRemaining(enabled) }
            onBarTiledRequested: function(enabled) { root.setBarTiled(enabled) }
            onFallbackRequested: root.openTerminalSettings()
            onCloseRequested: root.closeSettings()
          }

          ListView {
            id: providerList
            visible: !root.settingsOpen && root.tabEntries.length > 1
            width: parent.width
            height: visible ? Style.spacing.controlHeight : 0
            orientation: ListView.Horizontal
            spacing: Style.spacing.md
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: root.tabEntries

            delegate: Button {
              required property var modelData
              required property int index

              height: providerList.height
              text: Model.providerName(modelData)
              // `tabIndexOfSelected` is -1 exactly when the selected entry
              // has no tab (unconfigured/broken) — highlight nothing then.
              selected: root.tabIndexOfSelected >= 0 && index === root.tabIndexOfSelected
              hasCursor: root.cursorActive && root.tabIndexOfSelected >= 0
                && index === root.tabIndexOfSelected
              bordered: true
              foreground: root.foreground
              fontFamily: root.fontFamily
              fontSize: Style.font.bodySmall
              verticalPadding: Style.spacing.controlPaddingY
              onClicked: {
                root.cursorActive = true
                root.selectEntryById(modelData.id)
              }
              onHovered: function(isHovered) { if (isHovered) root.cursorActive = true }
            }
          }

          BorderSurface {
            readonly property string message: root.statusMessage()
            visible: !root.settingsOpen && message !== ""
            width: parent.width
            implicitHeight: statusText.implicitHeight + Style.spacing.xl * 2
            color: root.alpha(root.statusIsUrgent() ? root.urgent : root.foreground, 0.09)
            borderSpec: Border.flat(root.alpha(root.statusIsUrgent() ? root.urgent : root.foreground, 0.35), 1)
            radius: Style.cornerRadius

            Text {
              id: statusText
              anchors.left: parent.left
              anchors.right: parent.right
              anchors.verticalCenter: parent.verticalCenter
              anchors.leftMargin: Style.space(12)
              anchors.rightMargin: Style.space(12)
              text: parent.message
              textFormat: Text.PlainText
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }
          }

          Column {
            visible: !root.settingsOpen && root.loading && root.entries.length === 0
            width: parent.width
            spacing: Style.space(8)

            PanelSectionHeader {
              text: "USAGE"
              foreground: root.foreground
              fontFamily: root.fontFamily
            }

            Text {
              width: parent.width
              text: "Collecting configured providers…"
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.body
              horizontalAlignment: Text.AlignHCenter
            }
          }

          Column {
            id: usageSection
            visible: !root.settingsOpen && root.entrySections.length > 0
            width: parent.width
            spacing: Style.space(8)

            PanelSeparator {
              width: parent.width
              foreground: root.foreground
            }

            PanelSectionHeader {
              text: "USAGE & BALANCE"
              foreground: root.foreground
              fontFamily: root.fontFamily
            }

            Repeater {
              model: root.entrySections

              Column {
                required property var modelData
                width: usageSection.width

                // `visible` alone is not enough: QML evaluates the bindings of
                // hidden items too, so every row used to be handed to all three
                // components and the two that did not match read fields the row
                // does not carry. A "spacer" row has no label or value, which is
                // what produced the TypeError below on every report.
                MetricRow {
                  visible: modelData.type === "metric"
                  width: parent.width
                  row: modelData.type === "metric" ? modelData : null
                }

                DetailRow {
                  visible: modelData.type === "text"
                  width: parent.width
                  row: modelData.type === "text" ? modelData : null
                }

                BlockRow {
                  visible: modelData.type === "block"
                  width: parent.width
                  row: modelData.type === "block" ? modelData : null
                }

                Item {
                  visible: modelData.type === "spacer"
                  width: 1
                  height: Style.space(4)
                }
              }
            }
          }

          Text {
            visible: !root.settingsOpen && !root.loading && !root.entry && root.statusMessage() === ""
            width: parent.width
            topPadding: Style.space(20)
            text: "No configured provider reported usage."
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
          }

          Text {
            visible: !root.settingsOpen && root.entryFetchedAt !== ""
            width: parent.width
            topPadding: Style.space(2)
            text: Model.formatUpdated(root.entryFetchedAt, root.nowMs)
              + (usageProcess.running ? " · refreshing…" : "")
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            horizontalAlignment: Text.AlignHCenter
            elide: Text.ElideRight
          }
        }
      }
    }
  }

  component MetricRow: Column {
    id: metricRow
    property var row: null
    // Window rows use the SAME remaining bands as the bar tiles; other
    // metrics keep the Rust severity, whose critical still reads urgent.
    readonly property string band: row && (row.window === "session" || row.window === "weekly")
      ? Model.remainingClass(row.percent) : ""
    readonly property bool critical: row
      && (row.severity === "critical" || metricRow.band === "critical")
    readonly property color tone: metricRow.band !== ""
      ? root.bandColor(metricRow.band)
      : (metricRow.critical ? root.urgent : root.foreground)
    readonly property string detailText: Model.metricDetail(row)
    readonly property string resetText: row ? Model.formatReset(row.reset_at, root.nowMs) : ""

    spacing: Style.space(6)

    Item {
      width: parent.width
      implicitHeight: Math.max(metricLabel.implicitHeight, metricValue.implicitHeight)

      Text {
        id: metricLabel
        // The warning glyph rides the label so the alert survives color
        // blindness — red alone is not the only channel.
        text: metricRow.row
          ? (metricRow.critical ? "󰀪 " : "") + metricRow.row.label : ""
        textFormat: Text.PlainText
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        elide: Text.ElideRight
        anchors.left: parent.left
        anchors.right: metricValue.left
        anchors.rightMargin: Style.spacing.sm
        anchors.verticalCenter: parent.verticalCenter
      }

      Text {
        id: metricValue
        text: metricRow.row && metricRow.row.value !== ""
          ? metricRow.row.value : (metricRow.row ? metricRow.row.percent + "%" : "")
        textFormat: Text.PlainText
        color: metricRow.tone
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
        font.bold: true
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
      }
    }

    Item {
      width: parent.width
      implicitHeight: Math.max(Style.space(4), Math.round(Style.spacing.controlHeight * 0.14))

      Rectangle {
        id: meterTrack
        anchors.fill: parent
        radius: height / 2
        color: root.track
      }

      Rectangle {
        anchors.left: meterTrack.left
        anchors.verticalCenter: meterTrack.verticalCenter
        height: meterTrack.height
        radius: meterTrack.radius
        width: meterTrack.width * root.clamp(metricRow.row ? metricRow.row.percent / 100 : 0, 0, 1)
        color: metricRow.tone

        Behavior on width {
          NumberAnimation { duration: 160; easing.type: Easing.OutCubic }
        }

        Behavior on color {
          ColorAnimation { duration: 160 }
        }
      }
    }

    Text {
      visible: text !== ""
      width: parent.width
      text: metricRow.detailText
      textFormat: Text.PlainText
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      wrapMode: Text.WordWrap
    }

    Text {
      visible: text !== ""
      width: parent.width
      text: metricRow.resetText
      textFormat: Text.PlainText
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      // The row now carries a date and clock as well as the countdown, so it
      // can outgrow a narrow panel. Wrap like the detail line above it rather
      // than painting past the panel edge.
      wrapMode: Text.WordWrap
    }
  }

  component DetailRow: Item {
    id: detailRow
    property var row: null
    readonly property bool heading: row && row.label !== "" && row.value === ""

    implicitHeight: heading ? headingLabel.implicitHeight : Math.max(detailLabel.implicitHeight, detailValue.implicitHeight)

    PanelSectionHeader {
      id: headingLabel
      visible: detailRow.heading
      width: parent.width
      text: detailRow.row ? Model.autoTextSafe(String(detailRow.row.label || "").toUpperCase()) : ""
      foreground: root.foreground
      fontFamily: root.fontFamily
    }

    Text {
      id: detailLabel
      visible: !detailRow.heading && text !== ""
      text: detailRow.row ? detailRow.row.label : ""
      textFormat: Text.PlainText
      color: root.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.font.bodySmall
      font.bold: true
      anchors.left: parent.left
      anchors.top: parent.top
      width: Math.min(implicitWidth, parent.width * 0.42)
      elide: Text.ElideRight
    }

    Text {
      id: detailValue
      visible: !detailRow.heading
      text: detailRow.row ? detailRow.row.value : ""
      textFormat: Text.PlainText
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.bodySmall
      horizontalAlignment: detailLabel.visible ? Text.AlignRight : Text.AlignLeft
      wrapMode: Text.WordWrap
      anchors.left: detailLabel.visible ? detailLabel.right : parent.left
      anchors.leftMargin: detailLabel.visible ? Style.spacing.md : 0
      anchors.right: parent.right
      anchors.top: parent.top
    }
  }

  component BlockRow: Column {
    id: blockRow
    property var row: null

    spacing: Style.space(4)

    Text {
      width: parent.width
      text: blockRow.row ? blockRow.row.label : ""
      textFormat: Text.PlainText
      color: root.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.font.bodySmall
      font.bold: true
      elide: Text.ElideRight
    }

    Text {
      width: parent.width
      text: blockRow.row && blockRow.row.body ? blockRow.row.body.join("\n") : ""
      textFormat: Text.PlainText
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      wrapMode: Text.WordWrap
    }
  }
}
