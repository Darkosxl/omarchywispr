import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

// Bar indicator for omarchywispr. Follows the daemon over its unix socket;
// takes no space while idle, shows a small level meter while recording and a
// pulsing meter while transcribing.
BarWidget {
  id: root
  moduleName: "darkwarro.wispr"

  property string phase: "idle" // idle | recording | transcribing
  property real level: 0        // 0..1 mic level
  readonly property bool opened: phase !== "idle"

  // Bar shape contract (shell.summon/hide/toggle); nothing to open by hand.
  function open() {}
  function close() { root.phase = "idle"; root.level = 0 }

  readonly property int barW: Style.space(3)
  readonly property int barGap: Style.space(2)
  readonly property int minH: Style.space(3)
  readonly property int maxH: Math.max(root.minH + 2, Math.round(root.barSize * 0.55))

  visible: root.opened
  implicitWidth: root.opened ? meter.implicitWidth + Style.space(10) : 0
  implicitHeight: root.vertical ? meter.implicitHeight + Style.space(10) : root.barSize

  function applyEvent(line) {
    try {
      var e = JSON.parse(String(line))
      if (e.phase !== undefined) root.phase = String(e.phase)
      if (e.level !== undefined) root.level = Math.max(0, Math.min(1, Number(e.level)))
    } catch (err) {}
  }

  Socket {
    id: sock
    path: Quickshell.env("XDG_RUNTIME_DIR") + "/omarchywispr.sock"
    connected: true
    parser: SplitParser {
      onRead: function(line) { root.applyEvent(line) }
    }
    onConnectedChanged: {
      if (connected) write("follow\n")
      else root.close()
    }
  }

  // Daemon down (or not yet started): retry once a second.
  Timer {
    interval: 1000
    repeat: true
    running: !sock.connected
    onTriggered: sock.connected = true
  }

  Row {
    id: meter
    anchors.centerIn: parent
    spacing: root.barGap
    opacity: 1

    SequentialAnimation on opacity {
      running: root.phase === "transcribing"
      loops: Animation.Infinite
      NumberAnimation { to: 0.3; duration: 450 }
      NumberAnimation { to: 1.0; duration: 450 }
      onRunningChanged: if (!running) meter.opacity = 1
    }

    Repeater {
      model: 5
      Rectangle {
        required property int index
        // Centre bars swing most so it reads as a waveform.
        readonly property real weight: 0.45 + 0.55 * (1 - Math.abs(index - 2) / 3)
        width: root.barW
        height: root.phase === "recording"
          ? root.minH + root.level * (root.maxH - root.minH) * weight
          : root.minH
        radius: width / 2
        anchors.verticalCenter: parent.verticalCenter
        color: root.phase === "recording"
          ? Color.bar.active
          : (root.bar ? root.bar.foreground : Color.foreground)
        Behavior on height { NumberAnimation { duration: 60 } }
      }
    }
  }
}
