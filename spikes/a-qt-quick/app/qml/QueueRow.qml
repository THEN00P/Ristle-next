import QtQuick
import Ristle.Spike

// Queue row. Desktop drag reorders by click-dragging the row itself; the
// lifted row scales to 1.015. The 8 px strip on the left is instrumentation:
// its colour encodes the item id for the per-frame corruption check.
Item {
    id: row

    required property int index
    required property int itemId
    required property string title
    required property string artist
    required property string duration
    required property int art
    required property int source
    required property string marker
    required property string markerCheck
    required property var queue
    required property Item overlay

    width: ListView.view.width
    height: Lumen.queueRowHeight

    // Queue transition variants (SPIKE_QUEUE_FIX, see QueuePanel.qml):
    // 2  the row runs its own fade and scale on creation and its own fade on
    //    removal (ListView's delayRemove pattern). Left rows dimmed in the
    //    2026-10-18 diagnosis for a reason not yet found; Monday's batch logs
    //    the marks below to find it.
    // 3  opacity is a pure function of time since the row was created or
    //    removed, so no interrupted animation can leave it half-faded.
    // 4  no fade or scale at all; only positions animate.
    required property real clock
    readonly property int mode: Spike.queueFix
    readonly property bool ownsTransitions: mode === 2
    property real born: Date.now()
    property real died: 0
    readonly property real timedOpacity: Math.min(1, Math.max(0, (clock - born) / Spike.transitionMs))
                                         * (died > 0 ? Math.max(0, 1 - (clock - died) / Spike.transitionMs) : 1)
    opacity: mode === 2 ? 0 : (mode === 3 ? timedOpacity : 1)
    scale: mode === 2 ? 0.96 : 1
    Component.onCompleted: if (ownsTransitions) appear.start()
    // Instrumentation for the mode 2 diagnosis (ring marks 19 to 21).
    ListView.onAdd: Spike.mark(20, row.itemId)
    ListView.onRemove: {
        Spike.mark(19, row.itemId)
        if (mode === 2)
            disappear.start()
        else if (mode === 3)
            timedRemoval.start()
    }

    SequentialAnimation {
        id: timedRemoval
        PropertyAction { target: row; property: "ListView.delayRemove"; value: true }
        ScriptAction { script: row.died = Date.now() }
        PauseAnimation { duration: Spike.transitionMs }
        PropertyAction { target: row; property: "ListView.delayRemove"; value: false }
    }

    ParallelAnimation {
        id: appear
        onStarted: Spike.mark(22, row.itemId)
        onFinished: Spike.mark(21, row.itemId * 1000 + Math.round(row.opacity * 999))
        onStopped: Spike.mark(21, row.itemId * 1000 + Math.round(row.opacity * 999))
        NumberAnimation { target: row; property: "opacity"; to: 1; duration: Spike.transitionMs; easing.type: Easing.OutCubic }
        NumberAnimation { target: row; property: "scale"; to: 1; duration: Spike.transitionMs; easing.type: Easing.OutCubic }
    }
    SequentialAnimation {
        id: disappear
        PropertyAction { target: row; property: "ListView.delayRemove"; value: true }
        NumberAnimation { target: row; property: "opacity"; to: 0; duration: Spike.transitionMs; easing.type: Easing.OutCubic }
        PropertyAction { target: row; property: "ListView.delayRemove"; value: false }
    }

    MouseArea {
        id: dragArea

        // View state only: while held, the row detaches from the list.
        property bool held: false

        anchors.fill: parent
        drag.target: held ? content : undefined
        drag.axis: Drag.YAxis
        hoverEnabled: true
        onPressed: held = true
        onReleased: held = false
        onCanceled: held = false
    }

    DropArea {
        anchors.fill: parent
        anchors.margins: 8
        onEntered: drag => row.queue.moveItem(drag.source.rowIndex, row.index)
    }

    Rectangle {
        id: content

        property int rowIndex: row.index

        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: parent.verticalCenter
        width: row.width
        height: row.height
        radius: 8
        color: dragArea.drag.active ? Lumen.raised : (dragArea.containsMouse ? Lumen.hairline : "transparent")
        scale: dragArea.drag.active ? 1.015 : 1

        Drag.active: dragArea.drag.active
        Drag.source: content
        Drag.hotSpot.x: width / 2
        Drag.hotSpot.y: height / 2

        states: State {
            when: dragArea.held
            ParentChange { target: content; parent: row.overlay }
            AnchorChanges {
                target: content
                anchors.horizontalCenter: undefined
                anchors.verticalCenter: undefined
            }
        }

        Rectangle {
            x: 0
            width: 4
            height: parent.height
            color: row.marker
        }
        Rectangle {
            x: 4
            width: 4
            height: parent.height
            color: row.markerCheck
        }

        Artwork {
            x: 14
            width: 38
            height: 38
            anchors.verticalCenter: parent.verticalCenter
            artId: row.art
            unique: row.itemId
        }

        Column {
            x: 62
            width: parent.width - x - 60
            anchors.verticalCenter: parent.verticalCenter
            spacing: 2
            Text {
                width: parent.width
                text: row.title
                elide: Text.ElideRight
                color: Lumen.text
                font.family: Lumen.body
                font.pixelSize: 14
                font.weight: Font.Medium
            }
            Row {
                spacing: 6
                Rectangle {
                    width: 6
                    height: 6
                    radius: 3
                    anchors.verticalCenter: parent.verticalCenter
                    color: Lumen.sources[row.source]
                }
                Text {
                    text: row.artist
                    color: Lumen.muted
                    font.family: Lumen.body
                    font.pixelSize: 12
                }
            }
        }

        Text {
            anchors.right: parent.right
            anchors.rightMargin: 12
            anchors.verticalCenter: parent.verticalCenter
            text: row.duration
            color: Lumen.muted
            font.family: Lumen.mono
            font.pixelSize: 12
        }
    }
}
