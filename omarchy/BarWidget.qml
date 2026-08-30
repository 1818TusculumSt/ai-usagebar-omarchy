import QtQuick
import Quickshell
import qs.Commons
import qs.Ui
import "Model.js" as Model

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

  // Semi-transparent backdrop: on a transparent bar the tiles float over
  // the wallpaper — light or busy backgrounds can wash the text out. A
  // theme-colored translucent plate keeps every tile legible regardless.
  Rectangle {
    anchors.fill: labelRow
    anchors.margins: -3
    radius: Style.space(4)
    color: Util.alpha(Color.background, 0.62)
    border.width: 1
    border.color: Util.alpha(Color.foreground, 0.14)
    visible: labelRow.visible
  }

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

  // One part per tile — the vendor LOGO (plus a named account's suffix),
  // or a Text fallback when no asset ships / the report predates the field.
  // Each window figure keeps its own Text with its own remaining-band
  // color; no shared rich-text label whose spans can be dropped wholesale.
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
        readonly property string logo: modelData.logo !== undefined ? String(modelData.logo) : ""
        readonly property string logoLabel: modelData.logoLabel !== undefined
          ? String(modelData.logoLabel) : ""
        // Only the tag part can be a logo; figures and separators are text.
        readonly property bool showLogo: role === "tag"
          && Model.logoAssetName(logo) !== ""
        readonly property string role: String(modelData.role || "")
        readonly property string textWhenNoLogo: String(modelData.text || "")
        readonly property int leftPad: role === "sep" ? 2
          : role === "tag" ? 6
          : role === "figure" ? 4 : 0
        readonly property int rightPad: role === "sep" ? 2 : 0
        // The text beside a logo is the account suffix (named accounts
        // only); without a logo the full tag text stands in.
        readonly property string text: showLogo ? logoLabel : textWhenNoLogo
        implicitWidth: leftPad + rightPad
          + (showLogo ? logoImage.width + (logoLabel !== "" ? 3 : 0) : 0)
          + (text !== "" ? partText.implicitWidth : 0)
        implicitHeight: Math.max(logoImage.height, partText.implicitHeight)
        anchors.verticalCenter: parent.verticalCenter

        Image {
          id: logoImage
          visible: tilePart.showLogo
          source: tilePart.showLogo
            ? Qt.resolvedUrl("logos/" + Model.logoAssetName(tilePart.logo)) : ""
          // Sized against the bar's body text so the mark reads as part of
          // the label, not an icon bolted on.
          height: Style.font.bodySmall
          width: height
          sourceSize.width: height
          sourceSize.height: height
          fillMode: Image.PreserveAspectFit
          anchors.verticalCenter: parent.verticalCenter
          anchors.left: parent.left
          asynchronous: true
        }

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
          // Right of the logo when both render (named accounts); at the
          // item origin otherwise (no logo, or logo-only default accounts).
          x: tilePart.showLogo && tilePart.logoLabel !== ""
            ? logoImage.width + 3 : 0
        }
      }
    }
  }
}
