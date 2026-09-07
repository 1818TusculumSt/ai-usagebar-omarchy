import QtQuick
import Quickshell
import qs.Commons
import qs.Ui

// Quattro bar entry point. The popup is loaded separately so the object in
// the bar slot owns shell routing while Panel.qml remains focused on report
// collection and presentation.
BarWidget {
  id: root
  moduleName: "ai-usagebar-omarchy"

  readonly property var panelItem: panelLoader.item
  readonly property bool opened: panelItem ? panelItem.opened === true : false
  readonly property bool popoutSwitchClosing: panelItem
    ? panelItem.popoutSwitchClosing === true
    : false

  function open() {
    if (panelItem) panelItem.open()
  }

  function close() {
    // Outside-click dismissal routes through here (KeyboardPanel → owner).
    // The panel CLOSES on focus loss so other windows become clickable
    // again; the settings form's input and scroll position survive the
    // close and are restored on the next open.
    if (panelItem) panelItem.close()
  }

  function toggle() {
    if (panelItem) panelItem.toggle()
  }

  function closeForPopoutSwitch() {
    if (panelItem) panelItem.closeForPopoutSwitch()
  }

  function refresh() {
    if (panelItem) panelItem.refresh()
  }

  function nextEntry() {
    if (panelItem) panelItem.selectEntry(panelItem.entryIndex + 1)
  }

  function launchDashboard() {
    if (root.bar) root.bar.run("omarchy-launch-floating-terminal-with-presentation ai-usagebar-omarchy-tui")
    root.close()
  }

  function injectPanel() {
    var target = panelItem
    if (!target) return
    if ("bar" in target) target.bar = root.bar
    if ("settings" in target) target.settings = root.settings
    if ("anchorItem" in target) target.anchorItem = button
    if ("hostWidget" in target) target.hostWidget = root
  }

  // Width follows OUR label row: the button's own text is hidden (each tile
  // part renders as its own Text item — one object, one color property, no
  // rich-text CSS), so its width contribution is just the margins.
  implicitWidth: vertical ? button.implicitWidth
    : Math.max(button.implicitWidth, labelRow.implicitWidth + 17)
  implicitHeight: button.implicitHeight

  onBarChanged: injectPanel()
  onSettingsChanged: injectPanel()

  Loader {
    id: panelLoader
    active: true
    source: Qt.resolvedUrl("Panel.qml")
    visible: false
    onLoaded: {
      root.injectPanel()
      Qt.callLater(root.injectPanel)
    }
  }

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    // The visible label is our per-agent Text row below; the button keeps
    // its chrome (hover, press, wheel) with its own text hidden.
    text: ""
    labelVisible: false
    keepSpace: true
    fontSize: Style.font.bodySmall
    active: root.panelItem ? root.panelItem.alarming : false
    tooltipText: root.panelItem ? root.panelItem.tooltipText() : "AI usage"
    horizontalMargin: 8.5

    onPressed: function(buttonCode) {
      if (buttonCode === Qt.RightButton) root.launchDashboard()
      else if (buttonCode === Qt.MiddleButton) root.nextEntry()
      else root.toggle()
    }

    onWheelMoved: function(delta) {
      if (delta !== 0 && root.panelItem)
        root.panelItem.selectEntry(root.panelItem.entryIndex + (delta < 0 ? 1 : -1))
    }
  }

  // No backdrop plate: the bar text color is wallpaper-aware
  // (bar.barForeground), so tiles stay legible on any wallpaper with no
  // pill behind them — just a plain glyph like every other widget.

  // Vertical bars have no width for figures: a small band-colored dot under
  // the icon carries the worst usage state (errors already turn the icon
  // urgent, so the dot only reflects READY accounts).
  Rectangle {
    visible: root.vertical && root.panelItem
      && !root.panelItem.alarming && root.panelItem.worstBand !== ""
    width: 5
    height: 5
    radius: 2.5
    color: root.panelItem ? root.panelItem.bandColor(root.panelItem.worstBand) : "transparent"
    anchors.horizontalCenter: parent.horizontalCenter
    anchors.bottom: parent.bottom
    anchors.bottomMargin: 3
  }

  // One part per tile — plain provider text in the bar foreground, like
  // every other widget. Brand logo assets stay out of the bar (they carry
  // their own vendor colors); the full tag text stands in instead.
  // Each window figure keeps its own Text with the monochrome bar color;
  // no shared rich-text label whose spans can be dropped wholesale.
  // Tile boundaries (the │ separators) carry almost no padding — the
  // divider glyph has whitespace of its own — while intra-tile gaps
  // (sep > tag > figure) keep tiles reading as groups.
  // Items don't take the pointer, so hover, click and wheel keep reaching
  // the WidgetButton underneath.
  Row {
    id: labelRow
    anchors.centerIn: button
    spacing: 0


    Repeater {
      model: root.panelItem ? root.panelItem.barLabelModels() : []

      Item {
        id: tilePart
        required property var modelData
        readonly property string role: String(modelData.role || "")
        // Plain provider/account text — no brand logos in the bar, so every
        // tile reads in the bar foreground like the rest of the widgets.
        readonly property string text: String(modelData.text || "")
        readonly property int leftPad: role === "sep" ? 2
          : role === "tag" ? 6
          : role === "figure" ? 4 : 0
        readonly property int rightPad: role === "sep" ? 2 : 0
        implicitWidth: leftPad + rightPad
          + (text !== "" ? partText.implicitWidth : 0)
        implicitHeight: partText.implicitHeight
        // Row positions children by their actual width/height, not
        // implicitWidth/implicitHeight — without this binding every tile
        // part measures as 0-wide, so labelRow (and BarWidget.implicitWidth,
        // and the bar's ModuleSlot that sizes off it) undercounts the real
        // content and the rightmost tiles run past the reserved slot.
        width: implicitWidth
        height: implicitHeight
        anchors.verticalCenter: parent.verticalCenter

        Text {
          id: partText
          visible: tilePart.text !== ""
          text: tilePart.text
          color: modelData.color
          font.family: root.bar ? root.bar.fontFamily : Style.font.family
          font.pixelSize: Style.font.bodySmall
          // Critical figures go bold — the alert survives color blindness.
          font.bold: modelData.bold === true
          renderType: Text.NativeRendering
          anchors.verticalCenter: parent.verticalCenter
        }
      }
    }
  }
}
