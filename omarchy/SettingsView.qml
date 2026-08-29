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
  readonly property color dim: Qt.darker(foreground, 1.45)

  property var snapshot: ({ primary_choices: [], keys: [], accounts: [] })
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
    && (selectedPrimary !== "" || snapshot.primary_choices.length === 0)

  signal saved()
  signal fallbackRequested()
  signal showRemainingRequested(bool enabled)
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
    draftCount = 0
  }

  function collectChanges() {
    var changes = []
    for (var i = 0; i < keyRepeater.count; i++) {
      var row = keyRepeater.itemAt(i)
      if (!row || row.pendingAction === "unchanged") continue
      changes.push({
        id: row.vendorId,
        action: row.pendingAction,
        value: row.pendingAction === "set" ? row.secretText : ""
      })
    }
    return changes
  }

  function save() {
    if (!canSave) return
    var built = Model.buildSettingsPatch(selectedPrimary, collectChanges(), collectAccountChanges())
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
    for (var i = 0; i < accountRepeater.count; i++) {
      var card = accountRepeater.itemAt(i)
      if (!card) continue
      // The mutation rides the card's OWN vendor — a hardcoded one here
      // wrote kimi keys into the [zai] section.
      if (card.pendingRemove) {
        changes.push({ action: "remove", vendor: card.modelData.vendor, label: card.accountLabel })
        continue
      }
      var fields = card.pendingFields()
      var apiKey = card.pendingApiKey()
      if ((fields && Object.keys(fields).length > 0) || apiKey)
        changes.push({ action: "update", vendor: card.modelData.vendor, label: card.accountLabel,
          fields: fields && Object.keys(fields).length > 0 ? fields : undefined,
          apiKey: apiKey || undefined })
    }
    for (var d = 0; d < draftRepeater.count; d++) {
      var draftCard = draftRepeater.itemAt(d)
      if (!draftCard) continue
      var payload = draftCard.draftPayload()
      if (payload === null) continue
      changes.push(payload)
    }
    return changes
  }

  property int draftCount: 0
  // Which vendor the next "Add account" button targets; drafts carry their
  // own vendor so one list serves every multi-account provider.
  property string addDraftVendor: "zai"

  // New drafts come pre-named (Z.AI, Z.AI 2, Kimi, Kimi 2 …) so adding a
  // key needs zero typing; the user renames only when they WANT a label.
  function nextDraftName(vendor) {
    var base = vendor === "kimi" ? "Kimi" : "Z.AI"
    var taken = {}
    for (var i = 0; i < snapshot.accounts.length; i++)
      if (snapshot.accounts[i].vendor === vendor)
        taken[snapshot.accounts[i].label] = true
    for (var d = 0; d < draftRepeater.count; d++) {
      var card = draftRepeater.itemAt(d)
      if (card && card.draftVendor === vendor && card.draftName !== "")
        taken[card.draftName] = true
    }
    if (!taken[base]) return base
    var n = 2
    while (taken[base + " " + n]) n++
    return base + " " + n
  }

  function addDraftAccount(vendor) {
    if (vendor) addDraftVendor = vendor
    draftCount++
  }

  function removeDraftAccount() { draftCount = Math.max(0, draftCount - 1) }

  function scrubSecrets() {
    pendingPayload = ""
    for (var i = 0; i < keyRepeater.count; i++) {
      var row = keyRepeater.itemAt(i)
      if (row) row.scrub()
    }
    for (var j = 0; j < accountRepeater.count; j++) {
      var card = accountRepeater.itemAt(j)
      if (card) card.scrub()
    }
    draftCount = 0
  }

  function finishApply() {
    saving = false
    if (applyExitCode !== 0 || !Model.parseSettingsApplyResult(applyStdout)) {
      errorText = Model.errorMessage(applyStderr || "The settings command did not confirm the save.")
      return
    }
    scrubSecrets()
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
      label: "Show remaining instead of used"
      description: "Bar tiles show the used percentage by default (kmi 38% · 2h). Turn this on to show what is left of each window instead (kmi 62% · 2h). Applies immediately."
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
      text: "One card per key; the name is the bar-tile tag. Z.AI: the site (z.ai/bigmodel.cn) is auto-detected from the account type; only team keys need the organization and project ids (bigmodel.cn console → F12 → Application → Local Storage). Kimi: region stays auto-detected — a name and a key are all it takes."
      textFormat: Text.PlainText
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      wrapMode: Text.WordWrap
    }

    Repeater {
      id: accountRepeater
      model: root.snapshot.accounts

      BorderSurface {
      id: accountCard
      required property var modelData
      readonly property string accountLabel: String(modelData.label || "")
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
        var fields = modelData.fields || []
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
        var fields = modelData.fields || []
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

      Component.onCompleted: root.registerAccountCard(accountCard)
      Component.onDestruction: root.unregisterAccountCard(accountCard)

      width: parent.width
      implicitHeight: accountColumn.implicitHeight + Style.spacing.xl * 2
      color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.035)
      borderSpec: Border.flat(Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, pendingRemove ? 0.35 : 0.10), 1)
      radius: Style.cornerRadius

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
            text: root.safe((accountCard.modelData.vendor === "kimi" ? "Kimi · " : "Z.AI · ")
              + accountCard.modelData.display)
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
              : accountCard.modelData.environment_configured ? "environment override"
              : accountCard.modelData.inline_configured ? "key stored"
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
          model: accountCard.modelData.fields

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
              placeholderText: accountFieldRow.modelData.id === "name"
                ? "bar tile name" : ""
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
            var fields = accountCard.modelData.fields || []
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
            placeholderText: accountCard.modelData.configured
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
              && (accountCard.modelData.inline_configured || accountCard.apiKeyAction === "clear")
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

    Repeater {
    id: draftRepeater
    model: root.draftCount

    BorderSurface {
      id: draftCard
      required property int index
      property string draftName: ""
      property var draftFields: ({ account_type: "personal" })
      property string draftKeyValue: ""
      // The vendor this draft belongs to; snapshotted at creation — a
      // binding would flip this draft's fields when the OTHER vendor's Add
      // button is used later.
      property string draftVendor: ""
      property bool nameTouched: false

      Component.onCompleted: {
        draftVendor = root.addDraftVendor
        draftName = root.nextDraftName(draftVendor)
      }

      function switchVendor(vendor) {
        if (vendor === draftVendor) return
        draftVendor = vendor
        root.addDraftVendor = vendor
        // Vendor-specific defaults restart; an untouched auto-name follows
        // the new vendor base, a user-typed name is kept.
        draftFields = ({ account_type: "personal" })
        if (!nameTouched) draftName = root.nextDraftName(vendor)
      }

      function setDraftField(key, value) {
        var next = {}
        for (var k in draftFields) next[k] = draftFields[k]
        next[key] = String(value || "")
        draftFields = next
      }

      function draftNameTaken() {
        var wanted = draftName.trim().toLowerCase()
        if (wanted === "") return false
        for (var i = 0; i < root.snapshot.accounts.length; i++) {
          var account = root.snapshot.accounts[i]
          if (account.vendor !== draftVendor) continue
          if (String(account.label).toLowerCase() === wanted) return true
        }
        for (var d = 0; d < draftRepeater.count; d++) {
          var card = draftRepeater.itemAt(d)
          if (card && card !== draftCard && card.draftVendor === draftVendor
            && String(card.draftName).trim().toLowerCase() === wanted)
            return true
        }
        return false
      }

      function draftPayload() {
        if (draftName.trim() === "" || draftNameTaken()) return null
        var fields = draftVendor === "kimi" ? {} : draftFields
        var payload = { action: "add", vendor: draftVendor, name: draftName.trim(),
          fields: fields }
        if (draftKeyValue !== "") payload.apiKey = { action: "set", value: draftKeyValue }
        return payload
      }

      width: draftRepeater.parent.width
      implicitHeight: draftColumn.implicitHeight + Style.spacing.xl * 2
      color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.035)
      borderSpec: Border.flat(Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.10), 1)
      radius: Style.cornerRadius

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
            text: "New account"
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
            onClicked: root.removeDraftAccount()
          }
        }

        Text {
          width: parent.width
          text: root.safe("Provider")
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
        }
        Dropdown {
          width: parent.width
          showLabel: false
          value: draftCard.draftVendor
          options: [
            { id: "zai", value: "zai", label: "Z.AI" },
            { id: "kimi", value: "kimi", label: "Kimi" }
          ]
          foreground: root.foreground
          fontFamily: root.fontFamily
          enabled: !root.saving
          onChanged: function(value) { draftCard.switchVendor(value) }
        }

        TextField {
          width: parent.width
          enabled: !root.saving
          text: draftCard.draftName
          placeholderText: "Account name (bar tile tag)"
          foreground: draftCard.draftNameTaken() ? root.urgent : root.foreground
          onTextEdited: {
            draftCard.nameTouched = true
            draftCard.draftName = text
          }
          Keys.onEscapePressed: focus = false
        }

        Text {
          visible: draftCard.draftNameTaken()
          width: parent.width
          text: "That name is already in use — pick another."
          textFormat: Text.PlainText
          color: root.urgent
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
        }

        Text {
          visible: draftCard.draftVendor !== "kimi"
          width: parent.width
          text: root.safe("Account type")
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
        }
        Dropdown {
          visible: draftCard.draftVendor !== "kimi"
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
          enabled: !root.saving && draftCard.draftVendor !== "kimi"
          onChanged: function(value) { draftCard.setDraftField("account_type", value) }
        }

        TextField {
          visible: draftCard.draftVendor !== "kimi" && draftCard.draftFields.account_type === "team"
          width: parent.width
          enabled: !root.saving
          placeholderText: "Organization ID"
          foreground: root.foreground
          onTextEdited: draftCard.setDraftField("organization_id", text)
        }
        TextField {
          visible: draftCard.draftVendor !== "kimi" && draftCard.draftFields.account_type === "team"
          width: parent.width
          enabled: !root.saving
          placeholderText: "Project ID"
          foreground: root.foreground
          onTextEdited: draftCard.setDraftField("project_id", text)
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

    Button {
      width: parent.width
      text: "Add account"
      iconText: "󰐗"
      bordered: true
      focusable: true
      foreground: root.foreground
      fontFamily: root.fontFamily
      enabled: !root.saving
      onClicked: root.addDraftAccount()
    }
  }

  Column {
    visible: !root.loading && root.snapshot.keys.length > 0
    width: parent.width
    spacing: Style.space(10)

    PanelSeparator {
      width: parent.width
      foreground: root.foreground
    }
    PanelSectionHeader {
      text: "API KEYS"
      foreground: root.foreground
      fontFamily: root.fontFamily
    }
    Text {
      width: parent.width
      text: "Stored values are never loaded into the shell. Leave a field blank to keep its current value, or use the clear button to remove an inline key. Environment variables take precedence."
      textFormat: Text.PlainText
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      wrapMode: Text.WordWrap
    }

    Repeater {
      id: keyRepeater
      model: root.snapshot.keys

      BorderSurface {
        id: keyCard
        required property var modelData
        readonly property string vendorId: String(modelData.id || "")
        property string pendingAction: "unchanged"
        property alias secretText: keyField.text
        function scrub() {
          keyField.text = ""
          pendingAction = "unchanged"
        }

        width: keyRepeater.parent.width
        implicitHeight: keyColumn.implicitHeight + Style.spacing.xl * 2
        color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.035)
        borderSpec: Border.flat(Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.10), 1)
        radius: Style.cornerRadius

        Column {
          id: keyColumn
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.verticalCenter: parent.verticalCenter
          anchors.leftMargin: Style.space(12)
          anchors.rightMargin: Style.space(12)
          spacing: Style.space(6)

          Item {
            width: parent.width
            implicitHeight: Math.max(keyLabel.implicitHeight, keyStatus.implicitHeight)

            Text {
              id: keyLabel
              anchors.left: parent.left
              anchors.right: keyStatus.left
              anchors.rightMargin: Style.spacing.md
              text: root.safe(keyCard.modelData.label)
              textFormat: Text.PlainText
              color: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.bodySmall
              font.bold: true
              elide: Text.ElideRight
            }
            Text {
              id: keyStatus
              anchors.right: parent.right
              text: keyCard.pendingAction === "clear" ? "will clear"
                : keyCard.pendingAction === "set" ? "new key"
                : keyCard.modelData.environment_configured ? "environment override"
                : keyCard.modelData.inline_configured ? "stored"
                : "not configured"
              textFormat: Text.PlainText
              color: keyCard.pendingAction === "clear" ? root.urgent : root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
            }
          }

          Text {
            visible: text !== ""
            width: parent.width
            text: {
              var parts = []
              if (keyCard.modelData.environment) parts.push(root.safe(keyCard.modelData.environment))
              if (keyCard.modelData.note) parts.push(root.safe(keyCard.modelData.note))
              return parts.join(" · ")
            }
            textFormat: Text.PlainText
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            elide: Text.ElideRight
          }

          Row {
            width: parent.width
            spacing: Style.space(8)

            TextField {
              id: keyField
              width: parent.width - clearButton.width - parent.spacing
              password: true
              enabled: !root.saving && keyCard.pendingAction !== "clear"
              placeholderText: keyCard.modelData.configured
                ? "Leave blank to keep current key" : "Paste API key"
              foreground: root.foreground
              onTextEdited: keyCard.pendingAction = text.length > 0 ? "set" : "unchanged"
              Keys.onEscapePressed: focus = false
              onAccepted: root.save()
            }

            PanelActionButton {
              id: clearButton
              anchors.verticalCenter: keyField.verticalCenter
              iconText: keyCard.pendingAction === "clear" ? "󰕌" : "󰆴"
              tooltipText: keyCard.pendingAction === "clear"
                ? "Keep the stored key" : "Clear the stored inline key"
              foreground: root.foreground
              hoverColor: keyCard.pendingAction === "clear" ? root.foreground : root.urgent
              fontFamily: root.fontFamily
              focusable: true
              enabled: !root.saving && (keyCard.modelData.inline_configured || keyCard.pendingAction === "clear")
              onClicked: {
                if (keyCard.pendingAction === "clear") {
                  keyCard.pendingAction = "unchanged"
                } else {
                  keyField.text = ""
                  keyCard.pendingAction = "clear"
                }
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
      && (root.snapshot.primary_choices.length > 0 || root.snapshot.keys.length > 0
        || root.snapshot.accounts.length > 0 || root.draftCount > 0)
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
