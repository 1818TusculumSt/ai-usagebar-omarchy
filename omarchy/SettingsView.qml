import QtQuick
import QtQuick.Controls
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Native Quattro settings form. Rust remains the sole config owner: this view
// receives only non-secret key-presence metadata and sends changed keys over
// stdin, never argv or the environment.
Column {
  id: root

  property color foreground: Color.foreground
  property color urgent: Color.urgent
  property string fontFamily: Style.font.family
  property bool showRemaining: false
  property bool barTiled: true
  readonly property color dim: Qt.darker(foreground, 1.45)

  property var snapshot: ({ primary_choices: [], keys: [], accounts: [], vendors: [] })
  // Provider toggles the user flipped but has not saved, as {slug: bool}.
  // Only entries in here are sent, so an untouched provider never gets an
  // `enabled` written into config.toml.
  property var pendingVendors: ({})
  // Pending "add account" forms: a count only. Each draft's content lives
  // in its delegate (replacing a model array on every keystroke rebuilds
  // the delegates and drops TextField focus after one character).
  property string selectedPrimary: ""
  property string stateStdout: ""
  property string stateStderr: ""
  property string applyStdout: ""
  property string applyStderr: ""
  property string errorText: ""
  property string statusText: ""
  property string pendingPayload: ""
  property int stateExitCode: -1
  property int applyExitCode: -1
  property bool loading: false
  property bool saving: false
  readonly property bool canSave: !loading && !saving
    && (selectedPrimary !== "" || snapshot.primary_choices.length === 0
      || Object.keys(pendingVendors).length > 0)

  // A provider's effective state: the pending flip if there is one, else what
  // the snapshot reported.
  function vendorEnabled(vendor) {
    return pendingVendors.hasOwnProperty(vendor.id)
      ? pendingVendors[vendor.id] === true
      : vendor.enabled === true
  }

  // Flipping back to the saved value drops the entry entirely rather than
  // sending a no-op write.
  function toggleVendor(vendor) {
    var next = {}
    for (var k in pendingVendors) next[k] = pendingVendors[k]
    var wanted = !vendorEnabled(vendor)
    if (wanted === (vendor.enabled === true)) delete next[vendor.id]
    else next[vendor.id] = wanted
    pendingVendors = next
  }

  signal saved()
  signal fallbackRequested()
  signal showRemainingRequested(bool enabled)
  signal barTiledRequested(bool enabled)
  signal closeRequested()

  spacing: Style.space(12)
  focus: visible
  Keys.onEscapePressed: closeRequested()

  function safe(value) { return Model.autoTextSafe(value) }

  property bool hasLoaded: false

  function load() {
    if (stateProcess.running || applyProcess.running) return
    loading = true
    errorText = ""
    statusText = ""
    stateStdout = ""
    stateStderr = ""
    stateExitCode = -1
    stateProcess.running = true
  }

  function finishLoad() {
    loading = false
    if (stateExitCode !== 0) {
      var detail = Model.errorMessage(stateStderr)
      errorText = detail.indexOf("unrecognized subcommand") >= 0
        ? "This installed ai-usagebar-omarchy binary predates native settings. Update the package, or use the terminal settings fallback."
        : detail
      snapshot = ({ primary_choices: [], keys: [] })
      selectedPrimary = ""
      return
    }
    var parsed = Model.parseSettingsSnapshot(stateStdout)
    if (!parsed.ok) {
      errorText = parsed.error
      snapshot = ({ primary_choices: [], keys: [], accounts: [] })
      selectedPrimary = ""
      return
    }
    snapshot = parsed
    selectedPrimary = parsed.primary
    hasLoaded = true
  }

  // Explicit leave: clear pending secrets and drafts. Contrast the hide
  // path below — hiding keeps everything for the copy-paste round-trip.
  function discard() {
    scrubSecrets()
  }

  // Plain key-card changes are gone with the API KEYS section (every key
  // vendor is an account card now); the patch's `keys` channel stays empty.
  function collectChanges() {
    return []
  }

  function save() {
    if (!canSave) return
    var built = Model.buildSettingsPatch(selectedPrimary, collectChanges(), collectAccountChanges(),
      pendingVendors)
    if (!built.ok) {
      errorText = built.error
      return
    }
    saving = true
    errorText = ""
    statusText = ""
    applyStdout = ""
    applyStderr = ""
    applyExitCode = -1
    pendingPayload = built.payload
    applyProcess.running = true
  }

  // Account mutations: updates/removals from the existing cards, adds from
  // the pending forms. Only rows that actually changed are sent.
  function collectAccountChanges() {
    var changes = []
    for (var i = 0; i < accountsRepeater.count; i++) {
      var row = accountsRepeater.itemAt(i)
      var shape = row && row.shape ? row.shape : null
      if (!shape) continue
      if (row.isDraft) {
        changes.push(shape.draftPayload())
        continue
      }
      if (!row.isCard) continue
      // The mutation rides the card's OWN vendor — a hardcoded one here
      // once wrote kimi keys into the [zai] section.
      if (shape.pendingRemove) {
        changes.push({ action: "remove", vendor: shape.card.vendor, label: shape.accountLabel })
        continue
      }
      var fields = shape.pendingFields()
      var apiKey = shape.pendingApiKey()
      if ((fields && Object.keys(fields).length > 0) || apiKey)
        changes.push({ action: "update", vendor: shape.card.vendor, label: shape.accountLabel,
          fields: fields && Object.keys(fields).length > 0 ? fields : undefined,
          apiKey: apiKey || undefined })
    }
    return changes
  }

  // Vendor section ids of pending "Add account" drafts, in click order.
  // The array changes ONLY on add/remove clicks — never on keystrokes — so
  // the delegate TextFields keep their focus across edits.
  property var drafts: []

  function addDraftAccount(vendor) {
    drafts = drafts.concat([vendor])
  }

  function removeDraftAt(index) {
    var next = drafts.slice()
    next.splice(index, 1)
    drafts = next
  }

  // The account section as ONE vendor-grouped model: each vendor's cards
  // (snapshot order), then its Add button, then that vendor's pending
  // drafts. Rust owns the grouping order and the vendor display names.
  function accountsModel() {
    var cards = snapshot.accounts
    var groups = []
    var byVendor = {}
    for (var i = 0; i < cards.length; i++) {
      var v = cards[i].vendor
      if (!byVendor[v]) {
        byVendor[v] = { vendor: v, display: String(cards[i].vendor_display || v), cards: [] }
        groups.push(byVendor[v])
      }
      byVendor[v].cards.push(cards[i])
    }
    var model = []
    for (var g = 0; g < groups.length; g++) {
      var group = groups[g]
      // The prefix bar alternates two colors across groups: Z.AI blue,
      // Kimi purple, then blue/purple again down the list — the eye tracks
      // group boundaries without reading any title.
      var accent = g % 2 === 0 ? "#61afef" : "#c678dd"
      for (var c = 0; c < group.cards.length; c++)
        model.push({ kind: "card", data: group.cards[c], accent: accent })
      model.push({ kind: "add", vendor: group.vendor, vendorDisplay: group.display, accent: accent })
      for (var d = 0; d < drafts.length; d++)
        if (drafts[d] === group.vendor)
          model.push({ kind: "draft", vendor: group.vendor, vendorDisplay: group.display, draftIndex: d, accent: accent })
    }
    return model
  }

  // Whether a vendor's accounts carry the monthly_limit field — read from
  // the vendor's own card shape, Rust decides which vendors have it.
  function vendorSupportsLimit(vendor) {
    var cards = snapshot.accounts
    for (var i = 0; i < cards.length; i++) {
      if (cards[i].vendor !== vendor) continue
      var fields = cards[i].fields || []
      for (var f = 0; f < fields.length; f++)
        if (fields[f].id === "monthly_limit") return true
    }
    return false
  }


  function scrubSecrets() {
    pendingPayload = ""
    for (var i = 0; i < accountsRepeater.count; i++) {
      var row = accountsRepeater.itemAt(i)
      if (row && row.isCard && row.shape) row.shape.scrub()
    }
    drafts = []
  }

  function finishApply() {
    saving = false
    if (applyExitCode !== 0 || !Model.parseSettingsApplyResult(applyStdout)) {
      errorText = Model.errorMessage(applyStderr || "The settings command did not confirm the save.")
      return
    }
    scrubSecrets()
    // The reload below re-reads the saved state; keeping the flips would make
    // every toggle look pending forever.
    pendingVendors = ({})
    saved()
    load()
    // load() clears stale status before refreshing the snapshot, so set the
    // confirmation afterwards and keep it visible while the refresh runs.
    statusText = "Settings saved. Usage is refreshing."
  }

  onVisibleChanged: {
    if (visible) {
      // Reopening after an outside-click dismissal keeps the form as it was
      // (pending input included); only the first open (and the post-save
      // reload) fetch a snapshot — replacing it would rebuild the cards.
      if (!hasLoaded) load()
      Qt.callLater(function() { root.forceActiveFocus() })
    }
  }

  Process {
    id: stateProcess
    running: false
    command: ["ai-usagebar-omarchy", "settings", "show"]

    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.stateStdout = text
    }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.stateStderr = text
    }
    onExited: function(exitCode, exitStatus) {
      root.stateExitCode = exitCode
      Qt.callLater(root.finishLoad)
    }
  }

  Process {
    id: applyProcess
    running: false
    command: ["ai-usagebar-omarchy", "settings", "apply"]
    stdinEnabled: true

    onStarted: {
      write(root.pendingPayload + "\n")
      root.pendingPayload = ""
    }
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyStdout = text
    }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.applyStderr = text
    }
    onExited: function(exitCode, exitStatus) {
      root.applyExitCode = exitCode
      Qt.callLater(root.finishApply)
    }
  }

  Column {
    visible: root.loading
    width: parent.width
    spacing: Style.space(8)

    PanelSectionHeader {
      text: "SETTINGS"
      foreground: root.foreground
      fontFamily: root.fontFamily
    }
    Text {
      width: parent.width
      text: "Loading configuration…"
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.body
      horizontalAlignment: Text.AlignHCenter
    }
  }

  Column {
    visible: !root.loading
    width: parent.width
    spacing: Style.space(8)

    PanelSectionHeader {
      text: "DISPLAY"
      foreground: root.foreground
      fontFamily: root.fontFamily
    }
    Toggle {
      width: parent.width
      label: "Tile every account in the top bar"
      description: "On, the bar shows every working account side by side, each led by its provider logo — no module icon in front. Off, the bar is just the robot icon (a minimal indicator; hover or click for the numbers). The wheel and the tabs still switch the selected account. Applies immediately."
      checked: root.barTiled
      foreground: root.foreground
      fontFamily: root.fontFamily
      enabled: !root.saving
      onClicked: root.barTiledRequested(!root.barTiled)
    }
    Toggle {
      width: parent.width
      label: "Show remaining instead of used"
      description: "Bar tiles show what is LEFT of each window by default (kmi 62% · 5.3h). Turn this off to show the used percentage instead (kmi 38% · 5.3h). Applies immediately."
      checked: root.showRemaining
      foreground: root.foreground
      fontFamily: root.fontFamily
      enabled: !root.saving
      onClicked: root.showRemainingRequested(!root.showRemaining)
    }
  }

  BorderSurface {
    visible: root.errorText !== ""
    width: parent.width
    implicitHeight: errorColumn.implicitHeight + Style.spacing.xl * 2
    color: Qt.rgba(root.urgent.r, root.urgent.g, root.urgent.b, 0.09)
    borderSpec: Border.flat(Qt.rgba(root.urgent.r, root.urgent.g, root.urgent.b, 0.35), 1)
    radius: Style.cornerRadius

    Column {
      id: errorColumn
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(12)
      anchors.rightMargin: Style.space(12)
      spacing: Style.space(8)

      Text {
        width: parent.width
        text: root.safe(root.errorText)
        textFormat: Text.PlainText
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
        wrapMode: Text.WordWrap
      }
      Row {
        spacing: Style.space(8)
        Button {
          text: "Retry"
          bordered: true
          focusable: true
          foreground: root.foreground
          fontFamily: root.fontFamily
          onClicked: root.load()
        }
        Button {
          text: "Open terminal settings"
          bordered: true
          focusable: true
          foreground: root.foreground
          fontFamily: root.fontFamily
          onClicked: root.fallbackRequested()
        }
      }
    }
  }

  Column {
    // Absent on a binary that predates provider toggles, which sends no
    // `vendors` at all — the rest of the form still works.
    visible: !root.loading && root.snapshot.vendors !== undefined
      && root.snapshot.vendors.length > 0
    width: parent.width
    spacing: Style.space(8)

    PanelSeparator {
      width: parent.width
      foreground: root.foreground
    }
    PanelSectionHeader {
      text: "PROVIDERS"
      foreground: root.foreground
      fontFamily: root.fontFamily
    }
    Text {
      width: parent.width
      text: "Which providers the bar fetches. Providers that sign in through their own CLI — Claude, Codex, SuperGrok, Cursor, Kiro, GitHub Copilot — have no key to paste here: log in with that tool, then switch it on. Takes effect on save."
      textFormat: Text.PlainText
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      wrapMode: Text.WordWrap
    }
    Repeater {
      model: root.snapshot.vendors
      delegate: Toggle {
        required property var modelData
        width: parent ? parent.width : 0
        label: String(modelData.label || modelData.id)
        description: modelData.credential === "login"
          ? "Signs in through its own CLI — no API key here."
          : (modelData.credential === "none"
            ? "No credentials: read from the local app while it runs."
            : "Needs an API key, added under Accounts below.")
        checked: root.vendorEnabled(modelData)
        foreground: root.foreground
        fontFamily: root.fontFamily
        enabled: !root.saving
        onClicked: root.toggleVendor(modelData)
      }
    }
  }

  Column {
    visible: !root.loading && root.snapshot.primary_choices.length > 0
    width: parent.width
    spacing: Style.space(8)

    PanelSectionHeader {
      text: "PRIMARY PROVIDER"
      foreground: root.foreground
      fontFamily: root.fontFamily
    }
    Text {
      width: parent.width
      text: "Used by the CLI, Waybar, TUI, and as this panel's preferred provider."
      textFormat: Text.PlainText
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      wrapMode: Text.WordWrap
    }
    Dropdown {
      id: primaryDropdown
      width: parent.width
      showLabel: false
      value: root.selectedPrimary
      options: root.snapshot.primary_choices
      foreground: root.foreground
      fontFamily: root.fontFamily
      enabled: !root.saving
      onChanged: function(value) { root.selectedPrimary = value }
    }
  }


  Column {
    visible: !root.loading
    width: parent.width
    spacing: Style.space(10)

    PanelSeparator {
      width: parent.width
      foreground: root.foreground
    }
    PanelSectionHeader {
      text: "ACCOUNTS"
      foreground: root.foreground
      fontFamily: root.fontFamily
    }
    Text {
      width: parent.width
      text: "One card per key, grouped by provider with its own Add button; the bar tags accounts with the provider logo. Accounts are auto-named 1, 2, … by their order in each group — no name to type. Only Z.AI team keys need the two organization ids; the two Admin-spend vendors take an optional monthly limit."
      textFormat: Text.PlainText
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      wrapMode: Text.WordWrap
    }

    // ONE vendor-grouped model: each vendor's cards (snapshot order), then
    // its Add button right under its last key, then that vendor's pending
    // drafts. A vendor with nothing configured shows the button alone —
    // that empty state IS its entry point. All three shapes render through
    // a Loader so only one exists per row, and every row stays in THIS
    // root-level repeater (the old per-vendor delegate sections hid their
    // repeaters from the root functions and silently killed every save).
    Repeater {
      id: accountsRepeater
      model: root.accountsModel()

      Item {
        id: row
        required property var modelData
        readonly property bool isCard: modelData.kind === "card"
        readonly property bool isDraft: modelData.kind === "draft"
        // Root helpers (collectAccountChanges/scrubSecrets) reach the
        // loaded shape through this.
        readonly property var shape: slot.item
        width: parent ? parent.width : 0
        implicitHeight: slot.implicitHeight

        Loader {
          id: slot
          width: parent.width
          sourceComponent: row.isCard ? cardShape : row.isDraft ? draftShape : addShape
        }

        Component {
          id: addShape
          Button {
            width: parent ? parent.width : 0
            text: "Add " + row.modelData.vendorDisplay + " Account"
            iconText: "󰐗"
            bordered: true
            focusable: true
            foreground: root.foreground
            fontFamily: root.fontFamily
            enabled: !root.saving
            onClicked: root.addDraftAccount(row.modelData.vendor)
          }
        }

        Component {
          id: draftShape
          BorderSurface {
            id: draftCard
            readonly property string draftVendor: row.modelData.vendor
            readonly property bool supportsLimit: root.vendorSupportsLimit(draftCard.draftVendor)
            property var draftFields: ({ account_type: "personal" })
            property string draftKeyValue: ""
            property string draftLimitValue: ""

            function setDraftField(key, value) {
              var next = {}
              for (var k in draftFields) next[k] = draftFields[k]
              next[key] = String(value || "")
              draftFields = next
            }

            // No name field: the account's name is its position (1, 2, …).
            // Z.AI drafts carry their billing shape; every other vendor is
            // a key (and, for the spend vendors, a monthly limit).
            function draftPayload() {
              var fields = {}
              if (draftVendor === "zai") {
                fields = draftFields
              } else if (supportsLimit && draftLimitValue.trim() !== "") {
                fields.monthly_limit = draftLimitValue.trim()
              }
              var payload = { action: "add", vendor: draftVendor, fields: fields }
              if (draftKeyValue !== "") payload.apiKey = { action: "set", value: draftKeyValue }
              return payload
            }

            width: parent ? parent.width : 0
            implicitHeight: draftColumn.implicitHeight + Style.spacing.xl * 2
            color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.035)
            borderSpec: Border.flat(Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.10), 1)
            radius: Style.cornerRadius

            // Same provider accent as the saved cards.
            Rectangle {
              width: 3
              radius: 1.5
              color: row.modelData.accent
              anchors.left: parent.left
              anchors.top: parent.top
              anchors.bottom: parent.bottom
              anchors.topMargin: 6
              anchors.bottomMargin: 6
            }

            Column {
              id: draftColumn
              anchors.left: parent.left
              anchors.right: parent.right
              anchors.verticalCenter: parent.verticalCenter
              anchors.leftMargin: Style.space(12)
              anchors.rightMargin: Style.space(12)
              spacing: Style.space(6)

              Item {
                width: parent.width
                implicitHeight: draftTitle.implicitHeight
                Text {
                  id: draftTitle
                  text: "New " + row.modelData.vendorDisplay + " account"
                  color: root.foreground
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.bodySmall
                  font.bold: true
                }
                PanelActionButton {
                  anchors.right: parent.right
                  anchors.verticalCenter: draftTitle.verticalCenter
                  iconText: "󰆴"
                  tooltipText: "Discard this draft"
                  foreground: root.foreground
                  hoverColor: root.urgent
                  fontFamily: root.fontFamily
                  enabled: !root.saving
                  onClicked: root.removeDraftAt(row.modelData.draftIndex)
                }
              }

              Text {
                visible: draftCard.draftVendor === "zai"
                width: parent.width
                text: root.safe("Account type")
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
              }
              Dropdown {
                visible: draftCard.draftVendor === "zai"
                width: parent.width
                showLabel: false
                value: draftCard.draftFields.account_type || "personal"
                options: [
                  { id: "personal", value: "personal", label: "personal" },
                  { id: "team", value: "team", label: "team" },
                  { id: "usage", value: "usage", label: "usage" }
                ]
                foreground: root.foreground
                fontFamily: root.fontFamily
                enabled: !root.saving
                onChanged: function(value) { draftCard.setDraftField("account_type", value) }
              }

              TextField {
                visible: draftCard.draftVendor === "zai" && draftCard.draftFields.account_type === "team"
                width: parent.width
                enabled: !root.saving
                placeholderText: "Organization ID"
                foreground: root.foreground
                onTextEdited: draftCard.setDraftField("organization_id", text)
              }
              TextField {
                visible: draftCard.draftVendor === "zai" && draftCard.draftFields.account_type === "team"
                width: parent.width
                enabled: !root.saving
                placeholderText: "Project ID"
                foreground: root.foreground
                onTextEdited: draftCard.setDraftField("project_id", text)
              }

              TextField {
                visible: draftCard.supportsLimit
                width: parent.width
                enabled: !root.saving
                placeholderText: "Monthly limit (USD, optional)"
                foreground: root.foreground
                onTextEdited: draftCard.draftLimitValue = text
                Keys.onEscapePressed: focus = false
              }

              TextField {
                width: parent.width
                password: true
                enabled: !root.saving
                placeholderText: "API key"
                foreground: root.foreground
                onTextEdited: draftCard.draftKeyValue = text
                Keys.onEscapePressed: focus = false
                onAccepted: root.save()
              }

              // The apply affordance lives WITH the form: the global save at
              // the bottom of a long settings page is below the fold exactly
              // when a draft was just filled in.
              Button {
                width: parent.width
                text: root.saving ? "Saving…" : "Apply"
                iconText: root.saving ? "󰑐" : "󰄬"
                iconSpinning: root.saving
                bordered: true
                focusable: true
                foreground: root.foreground
                fontFamily: root.fontFamily
                enabled: root.canSave
                onClicked: root.save()
              }
            }
          }
        }

        Component {
          id: cardShape
          BorderSurface {
          id: accountCard
          readonly property var card: row.modelData.data
          readonly property string accountLabel: String(card.label || "")
          property var fieldValues: ({})
          property string apiKeyAction: "unchanged"
          property alias apiKeyText: accountKeyField.text
          property bool pendingRemove: false

          function fieldValue(field) {
            return Object.prototype.hasOwnProperty.call(fieldValues, field.id)
              ? fieldValues[field.id] : String(field.value || "")
          }

          function setFieldValue(field, value) {
            var next = {}
            for (var key in fieldValues) next[key] = fieldValues[key]
            next[field.id] = String(value || "")
            fieldValues = next
          }

          function pendingFields() {
            var changes = {}
            var fields = card.fields || []
            var known = {}
            for (var i = 0; i < fields.length; i++) {
              known[fields[i].id] = true
              if (fields[i].kind === "secret") continue
              var current = fieldValue(fields[i])
              if (current !== String(fields[i].value || "")) changes[fields[i].id] = current
            }
            // Values typed into client-side synthesized inputs (the team ids
            // that appear when the type flips to team mid-edit) have no
            // snapshot field to diff against — send them as-is.
            for (var key in fieldValues) {
              if (!known[key] && fieldValues[key] !== "") changes[key] = fieldValues[key]
            }
            return changes
          }

          function pendingApiKey() {
            if (apiKeyAction === "set") return { action: "set", value: apiKeyText }
            if (apiKeyAction === "clear") return { action: "clear" }
            return null
          }

          function fieldOptions(field) {
            var options = []
            for (var i = 0; i < field.choices.length; i++) {
              var choice = field.choices[i]
              var label = (field.labels && field.labels[choice] !== undefined)
                ? field.labels[choice] : choice
              options.push({ id: choice, value: choice, label: label === "" ? "auto" : label })
            }
            return options
          }

          readonly property bool teamIncomplete: {
            var fields = card.fields || []
            var type = "", org = "", proj = ""
            for (var i = 0; i < fields.length; i++) {
              if (fields[i].id === "account_type") type = fieldValue(fields[i])
              if (fields[i].id === "organization_id") org = fieldValue(fields[i])
              if (fields[i].id === "project_id") proj = fieldValue(fields[i])
            }
            return type === "team" && (org === "" || proj === "")
          }

          function scrub() {
            fieldValues = ({})
            apiKeyAction = "unchanged"
            apiKeyText = ""
            pendingRemove = false
          }

          width: parent ? parent.width : 0
          implicitHeight: accountColumn.implicitHeight + Style.spacing.xl * 2
          color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.035)
          borderSpec: Border.flat(Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, pendingRemove ? 0.35 : 0.10), 1)
          radius: Style.cornerRadius

          // Provider accent bar — one glance tells a Z.AI card from a Kimi
          // card before any text is read.
          Rectangle {
            width: 3
            radius: 1.5
            color: row.modelData.accent
            anchors.left: parent.left
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            anchors.topMargin: 6
            anchors.bottomMargin: 6
          }

          Column {
            id: accountColumn
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            anchors.leftMargin: Style.space(12)
            anchors.rightMargin: Style.space(12)
            spacing: Style.space(6)

            Item {
              width: parent.width
              implicitHeight: Math.max(accountTitle.implicitHeight, accountStatus.implicitHeight, removeAccountButton.implicitHeight)

              Text {
                id: accountTitle
                anchors.left: parent.left
                anchors.right: accountStatus.left
                anchors.rightMargin: Style.spacing.md
                text: root.safe(accountCard.card.vendor_display
                  + (accountCard.accountLabel === "" ? "" : " · " + accountCard.card.display))
                textFormat: Text.PlainText
                color: root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.bodySmall
                font.bold: true
                elide: Text.ElideRight
              }
              Text {
                id: accountStatus
                anchors.right: removeAccountButton.left
                anchors.rightMargin: Style.spacing.sm
                text: accountCard.apiKeyAction === "clear" ? "key will clear"
                  : accountCard.apiKeyAction === "set" ? "new key"
                  : accountCard.pendingRemove ? "will remove"
                  : accountCard.card.environment_configured ? "environment override"
                  : accountCard.card.inline_configured ? "key stored"
                  : "no key"
                textFormat: Text.PlainText
                color: (accountCard.pendingRemove || accountCard.apiKeyAction === "clear") ? root.urgent : root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
              }
              PanelActionButton {
                id: removeAccountButton
                anchors.right: parent.right
                anchors.verticalCenter: accountTitle.verticalCenter
                iconText: accountCard.pendingRemove ? "󰕌" : "󰆴"
                tooltipText: accountCard.pendingRemove
                  ? "Keep this account" : "Remove this account"
                foreground: root.foreground
                hoverColor: accountCard.pendingRemove ? root.foreground : root.urgent
                fontFamily: root.fontFamily
                enabled: !root.saving
                onClicked: accountCard.pendingRemove = !accountCard.pendingRemove
              }
            }

            Repeater {
              model: accountCard.card.fields

              Column {
                id: accountFieldRow
                required property var modelData
                width: parent.width
                spacing: Style.space(4)

                Text {
                  width: parent.width
                  text: root.safe(accountFieldRow.modelData.label)
                  textFormat: Text.PlainText
                  color: root.dim
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.caption
                }

                Dropdown {
                  visible: accountFieldRow.modelData.kind === "choice"
                  width: parent.width
                  showLabel: false
                  value: accountCard.fieldValue(accountFieldRow.modelData)
                  options: accountCard.fieldOptions(accountFieldRow.modelData)
                  foreground: root.foreground
                  fontFamily: root.fontFamily
                  enabled: !root.saving && !accountCard.pendingRemove
                  onChanged: function(value) { accountCard.setFieldValue(accountFieldRow.modelData, value) }
                }

                TextField {
                  visible: accountFieldRow.modelData.kind === "text"
                  width: parent.width
                  enabled: !root.saving && !accountCard.pendingRemove
                  text: accountCard.fieldValue(accountFieldRow.modelData)
                  foreground: root.foreground
                  onTextEdited: accountCard.setFieldValue(accountFieldRow.modelData, text)
                  Keys.onEscapePressed: focus = false
                  onAccepted: root.save()
                }
              }
            }

            // Switching the type to team client-side must surface the id inputs
            // immediately — the snapshot only carries them for accounts that
            // were already team-typed when it was taken.
            Repeater {
              model: {
                var typeField = null
                var hasOrg = false
                var fields = accountCard.card.fields || []
                for (var i = 0; i < fields.length; i++) {
                  if (fields[i].id === "account_type") typeField = fields[i]
                  if (fields[i].id === "organization_id") hasOrg = true
                }
                return (typeField && accountCard.fieldValue(typeField) === "team" && !hasOrg)
                  ? ["organization_id", "project_id"] : []
              }

              Column {
                id: synthRow
                required property var modelData
                width: parent.width
                spacing: Style.space(4)

                Text {
                  width: parent.width
                  text: synthRow.modelData === "organization_id"
                    ? root.safe("Organization ID") : root.safe("Project ID")
                  textFormat: Text.PlainText
                  color: root.dim
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.caption
                }
                TextField {
                  width: parent.width
                  enabled: !root.saving && !accountCard.pendingRemove
                  placeholderText: synthRow.modelData === "organization_id" ? "org-…" : "proj-…"
                  foreground: root.foreground
                  onTextEdited: accountCard.setFieldValue(
                    { id: synthRow.modelData, value: "" }, text)
                  Keys.onEscapePressed: focus = false
                  onAccepted: root.save()
                }
              }
            }

            // The API key row — always last.
            Row {
              width: parent.width
              spacing: Style.space(8)

              TextField {
                id: accountKeyField
                width: parent.width - accountKeyClear.width - parent.spacing
                password: true
                enabled: !root.saving && accountCard.apiKeyAction !== "clear" && !accountCard.pendingRemove
                placeholderText: accountCard.card.configured
                  ? "Leave blank to keep current key" : "Paste API key"
                foreground: root.foreground
                onTextEdited: accountCard.apiKeyAction = text.length > 0 ? "set" : "unchanged"
                Keys.onEscapePressed: focus = false
                onAccepted: root.save()
              }

              PanelActionButton {
                id: accountKeyClear
                anchors.verticalCenter: accountKeyField.verticalCenter
                iconText: accountCard.apiKeyAction === "clear" ? "󰕌" : "󰆴"
                tooltipText: accountCard.apiKeyAction === "clear"
                  ? "Keep the stored key" : "Clear the stored inline key"
                foreground: root.foreground
                hoverColor: accountCard.apiKeyAction === "clear" ? root.foreground : root.urgent
                fontFamily: root.fontFamily
                enabled: !root.saving
                  && (accountCard.card.inline_configured || accountCard.apiKeyAction === "clear")
                onClicked: {
                  if (accountCard.apiKeyAction === "clear") {
                    accountCard.apiKeyAction = "unchanged"
                  } else {
                    accountKeyField.text = ""
                    accountCard.apiKeyAction = "clear"
                  }
                }
              }
            }

            Text {
              visible: accountCard.teamIncomplete && !accountCard.pendingRemove
              width: parent.width
              text: "A team key needs both ids. bigmodel.cn console → F12 → Application → Local Storage → Bigmodel-Organization / Bigmodel-Project."
              textFormat: Text.PlainText
              color: root.urgent
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }

            Button {
              width: parent.width
              text: root.saving ? "Saving…" : "Apply"
              iconText: root.saving ? "󰑐" : "󰄬"
              iconSpinning: root.saving
              bordered: true
              focusable: true
              foreground: root.foreground
              fontFamily: root.fontFamily
              enabled: root.canSave && !accountCard.pendingRemove
              onClicked: root.save()
            }
          }
          }
        }
      }
    }
  }

  BorderSurface {
    visible: root.statusText !== ""
    width: parent.width
    implicitHeight: savedText.implicitHeight + Style.spacing.lg * 2
    color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.06)
    borderSpec: Border.flat(Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.18), 1)
    radius: Style.cornerRadius

    Text {
      id: savedText
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(12)
      anchors.rightMargin: Style.space(12)
      text: root.safe(root.statusText)
      textFormat: Text.PlainText
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      horizontalAlignment: Text.AlignHCenter
    }
  }

  Button {
    visible: !root.loading
      && (root.snapshot.primary_choices.length > 0
        || root.snapshot.accounts.length > 0 || root.drafts.length > 0)
    width: parent.width
    text: root.saving ? "Saving…" : "Save settings"
    iconText: root.saving ? "󰑐" : "󰄬"
    iconSpinning: root.saving
    bordered: true
    focusable: true
    foreground: root.foreground
    fontFamily: root.fontFamily
    enabled: root.canSave
    onClicked: root.save()
  }
}
