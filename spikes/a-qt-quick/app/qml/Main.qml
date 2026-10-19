import QtQuick
import Ristle.Spike

Window {
    id: root

    property int page: Spike.startPage
    property bool sidebarOpen: true
    property bool queueChurn: false

    x: 80
    y: 60
    width: 1280
    height: 800
    visible: true
    color: Lumen.deepest
    title: "Ristle spike A"

    onPageChanged: Spike.mark(16, page)

    TrackModel {
        id: trackModel
        rows: Spike.rows
    }

    QueueModel {
        id: queueModel
        length: Spike.queueLength
    }

    Sidebar {
        id: sidebar
        y: 0
        width: root.sidebarOpen ? 232 : 72
        height: root.height - 84
        open: root.sidebarOpen
        page: root.page
        onNavigate: target => root.page = target

        Behavior on width {
            NumberAnimation { duration: 200; easing.type: Easing.OutCubic }
        }
    }

    Item {
        id: pages
        x: sidebar.width
        y: 8
        width: root.width - sidebar.width - 336
        height: root.height - 84 - 16

        // Cached pages: both stay alive and only visibility switches.
        TrackTable {
            anchors.fill: parent
            visible: !Spike.freshPages && root.page === 0
            enabled: visible
            tracks: trackModel
        }
        LibraryPage {
            anchors.fill: parent
            visible: !Spike.freshPages && root.page === 1
            tracks: trackModel
        }

        // Fresh pages: every switch builds the page from scratch. With
        // SPIKE_ASYNC_PAGES the page incubates over several frames and a
        // skeleton shows until it is ready (Lumen: loading uses skeletons).
        Loader {
            id: freshLoader
            anchors.fill: parent
            active: Spike.freshPages
            asynchronous: Spike.asyncPages
            sourceComponent: root.page === 0 ? tablePage : libraryPage
            onLoaded: Spike.mark(16, 100 + root.page)
        }
        Rectangle {
            anchors.fill: parent
            visible: Spike.freshPages && freshLoader.status !== Loader.Ready
            color: Lumen.base
            radius: 10
            border.color: Lumen.hairline
            Column {
                x: 32
                y: 32
                spacing: 14
                Repeater {
                    model: 9
                    Rectangle { width: 420 - (index % 3) * 60; height: 18; radius: 4; color: Lumen.raised }
                }
            }
        }
        Component {
            id: tablePage
            TrackTable { tracks: trackModel }
        }
        Component {
            id: libraryPage
            LibraryPage { tracks: trackModel }
        }
    }

    QueuePanel {
        x: root.width - 328
        y: 8
        width: 320
        height: root.height - 84 - 16
        queue: queueModel
        overlay: overlay
    }

    NowPlayingBar {
        y: root.height - 84
        width: root.width
        height: 84
    }

    Timer {
        interval: 8
        repeat: true
        running: Spike.queueOpsPerSecond > 0 && root.queueChurn
        onTriggered: queueModel.stepAt(Spike.queueOpsPerSecond, 8)
    }

    Timer {
        interval: 4
        repeat: true
        running: Spike.burstCount > 0 && root.queueChurn
        onTriggered: queueModel.burstAt(Spike.burstCount, Spike.burstGapMs, Spike.burstPeriodMs, 8)
    }

    // Instrumentation, bottom-right corner: two 8 px clocks the duplication
    // pass decodes per frame. Left: red = GUI-thread animation phase. Right:
    // grey = render-thread Animator phase. Each changes on every frame.
    Rectangle {
        id: guiClock
        property real phase: 0
        x: root.width - 20
        y: root.height - 10
        width: 8
        height: 8
        color: Qt.rgba(phase, 0, 0, 1)
        visible: Spike.heartbeat
        NumberAnimation on phase { from: 0; to: 1; duration: 1000; loops: Animation.Infinite; running: Spike.heartbeat }
    }
    Rectangle {
        x: root.width - 10
        y: root.height - 10
        width: 8
        height: 8
        color: "black"
        visible: Spike.heartbeat
        Rectangle {
            anchors.fill: parent
            color: "white"
            OpacityAnimator on opacity { from: 0; to: 1; duration: 1000; loops: Animation.Infinite; running: Spike.heartbeat && Spike.animators }
        }
    }

    Timer {
        interval: 10000
        repeat: true
        triggeredOnStart: true
        running: Spike.memoryLog
        onTriggered: Spike.logMemory()
    }

    Timer {
        interval: Spike.quitAfterMs
        running: Spike.quitAfterMs > 0
        onTriggered: Qt.quit()
    }

    Item {
        id: overlay
        anchors.fill: parent
    }

    Loader {
        anchors.fill: parent
        active: Spike.specimen
        sourceComponent: TextSpecimen {}
    }

    Shortcut { sequence: "Ctrl+B"; onActivated: root.sidebarOpen = !root.sidebarOpen }
    Shortcut { sequence: "Ctrl+1"; onActivated: root.page = 0 }
    Shortcut { sequence: "Ctrl+2"; onActivated: root.page = 1 }
    Shortcut { sequence: "Ins"; onActivated: queueModel.insertAt(1) }
    Shortcut { sequence: "Del"; onActivated: queueModel.removeAt(1) }
    Shortcut { sequence: "Ctrl+M"; onActivated: queueModel.moveItem(1, 5) }
    Shortcut { sequence: "F2"; onActivated: root.queueChurn = !root.queueChurn }

    // Positive control: frame-aligned GUI-thread stalls on F5–F9, GUI stalls at
    // a random phase on Ctrl+F5–F9, render-thread stalls on Shift+F5–F9.
    Shortcut { sequence: "F5"; onActivated: Spike.stallGui(4) }
    Shortcut { sequence: "F6"; onActivated: Spike.stallGui(8) }
    Shortcut { sequence: "F7"; onActivated: Spike.stallGui(16) }
    Shortcut { sequence: "F8"; onActivated: Spike.stallGui(33) }
    Shortcut { sequence: "F9"; onActivated: Spike.stallGui(100) }
    Shortcut { sequence: "Ctrl+F5"; onActivated: Spike.stallGuiNow(4) }
    Shortcut { sequence: "Ctrl+F6"; onActivated: Spike.stallGuiNow(8) }
    Shortcut { sequence: "Ctrl+F7"; onActivated: Spike.stallGuiNow(16) }
    Shortcut { sequence: "Ctrl+F8"; onActivated: Spike.stallGuiNow(33) }
    Shortcut { sequence: "Ctrl+F9"; onActivated: Spike.stallGuiNow(100) }
    Shortcut { sequence: "Shift+F5"; onActivated: Spike.stallRender(4) }
    Shortcut { sequence: "Shift+F6"; onActivated: Spike.stallRender(8) }
    Shortcut { sequence: "Shift+F7"; onActivated: Spike.stallRender(16) }
    Shortcut { sequence: "Shift+F8"; onActivated: Spike.stallRender(33) }
    Shortcut { sequence: "Shift+F9"; onActivated: Spike.stallRender(100) }
}
