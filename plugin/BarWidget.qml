import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

// Bar indicator for omarchywispr. Follows the daemon over its unix socket.
// Idle: small static waveform. Recording: expands into a live red waveform.
// Transcribing: red waveform pulsing. Click toggles dictation.
BarWidget {
  id: root
  moduleName: "darkwarro.wispr"

  property string phase: "idle" // idle | recording | transcribing
  property real level: 0        // 0..1 mic level
  property int tick: 0          // bumps per level event, drives per-bar jitter
  readonly property bool opened: phase !== "idle"
  readonly property bool recording: phase === "recording"

  // Bar shape contract (shell.summon/hide/toggle).
  function open() { if (root.bar) root.bar.run("omarchywispr start") }
  function close() { if (root.bar) root.bar.run("omarchywispr stop") }

  readonly property color fg: bar ? bar.foreground : Color.foreground
  readonly property color red: Color.bar.active
  readonly property int barW: root.opened ? Style.space(3) : Style.space(2)
  readonly property int barGap: Style.space(2)
  readonly property int waveH: Math.round(root.barSize * 0.58)
  readonly property int minH: Math.max(2, Math.round(root.waveH * 0.2))
  // Resting silhouette, also the per-bar weight while recording.
  readonly property var shape: [0.3, 0.45, 0.35, 0.6, 1.0, 0.55, 0.4, 0.5, 0.3]

  implicitWidth: wave.implicitWidth + Style.space(10)
  implicitHeight: root.barSize
  Behavior on implicitWidth { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }

  function applyEvent(line) {
    try {
      var e = JSON.parse(String(line))
      if (e.phase !== undefined) root.phase = String(e.phase)
      if (e.level !== undefined) {
        root.level = Math.max(0, Math.min(1, Number(e.level)))
        root.tick = (root.tick + 1) % 1000
      }
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
      else { root.phase = "idle"; root.level = 0 }
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
    id: wave
    anchors.centerIn: parent
    spacing: root.barGap
    opacity: sock.connected ? 1 : 0.35

    SequentialAnimation on opacity {
      running: root.phase === "transcribing"
      loops: Animation.Infinite
      NumberAnimation { to: 0.3; duration: 450 }
      NumberAnimation { to: 1.0; duration: 450 }
      onRunningChanged: if (!running) wave.opacity = 1
    }

    Repeater {
      model: 9
      Rectangle {
        required property int index
        // Idle shows only the five centre bars; recording reveals all nine.
        readonly property bool core: index >= 2 && index <= 6
        readonly property real weight: root.shape[index]
        // Cheap deterministic wobble so bars don't move in lockstep.
        readonly property real jitter: 0.65 + 0.35 * (((index * 7 + root.tick * 3) % 5) / 4)
        visible: root.opened || core
        width: root.barW
        // Quiet mic still shows a low silhouette instead of a row of dots.
        readonly property real floorH: Math.max(root.minH, root.waveH * weight * 0.3)
        height: root.recording
          ? floorH + root.level * (root.waveH - floorH) * weight * jitter
          : Math.max(root.minH, Math.round(root.waveH * weight))
        radius: width / 2
        anchors.verticalCenter: parent.verticalCenter
        color: root.opened ? root.red : Util.alpha(root.fg, 0.8)
        Behavior on height { NumberAnimation { duration: 60 } }
        Behavior on color { ColorAnimation { duration: 160 } }
      }
    }
  }

  MouseArea {
    anchors.fill: parent
    cursorShape: Qt.PointingHandCursor
    onClicked: if (root.bar) root.bar.run("omarchywispr toggle")
  }
}
