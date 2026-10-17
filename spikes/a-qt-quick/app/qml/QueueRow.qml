import QtQuick
import Ristle.Spike

// Queue row. Desktop drag reorders by click-dragging the row itself; the
// lifted row scales to 1.015. The 6 px strip on the left is instrumentation:
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
    required property var queue
    required property Item overlay

    width: ListView.view.width
    height: Lumen.queueRowHeight

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
            width: 6
            height: parent.height
            color: row.marker
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
