import QtQuick
import QtQuick.Controls.Basic
import Ristle.Spike

// A playlist page: header, column header and the track table.
Item {
    id: page

    required property var tracks
    property alias list: list

    Rectangle {
        anchors.fill: parent
        color: Lumen.base
        radius: 10
        border.color: Lumen.hairline
    }

    Column {
        id: header
        x: 32
        y: 24
        spacing: 6

        Text {
            text: "SPOTIFY · PLAYLIST"
            color: Lumen.amber
            font.family: Lumen.body
            font.pixelSize: 11
            font.weight: Font.DemiBold
            font.letterSpacing: 1.8
        }
        Text {
            text: "Liked Songs"
            color: Lumen.text
            font.family: Lumen.headline
            font.weight: Font.ExtraBold
            font.pixelSize: 34
        }
        Text {
            text: page.tracks.rows.toLocaleString(Qt.locale("en_US"), "f", 0) + " songs"
            color: Lumen.muted
            font.family: Lumen.body
            font.pixelSize: 13
        }
    }

    Item {
        id: columns
        x: 16
        y: header.y + header.height + 18
        width: page.width - 32
        height: 28

        readonly property real albumX: width * 0.46
        readonly property real addedX: width * 0.72
        readonly property real durationX: width - 64

        Repeater {
            model: [
                { label: "#", x: 8 },
                { label: "TITLE", x: 52 },
                { label: "ALBUM", x: columns.albumX },
                { label: "DATE ADDED", x: columns.addedX },
                { label: "TIME", x: columns.durationX }
            ]
            Text {
                required property var modelData
                x: modelData.x
                anchors.verticalCenter: parent.verticalCenter
                text: modelData.label
                color: Lumen.faint
                font.family: Lumen.body
                font.pixelSize: 11
                font.weight: Font.DemiBold
                font.letterSpacing: 1.6
            }
        }

        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: Lumen.hairline
        }
    }

    ListView {
        id: list
        x: 16
        y: columns.y + columns.height + 4
        width: page.width - 32
        height: page.height - y - 8
        clip: true
        model: page.tracks
        reuseItems: true
        boundsBehavior: Flickable.StopAtBounds
        // Desktop: the mouse selects and drags rows; touch and wheel scroll.
        acceptedButtons: Qt.NoButton
        maximumFlickVelocity: 12000

        delegate: TrackRow {
            width: list.width
            albumX: columns.albumX
            addedX: columns.addedX
            durationX: columns.durationX
        }

        ScrollBar.vertical: ScrollBar {
            id: scrollBar
            policy: ScrollBar.AlwaysOn
            minimumSize: 0.04
            contentItem: Rectangle {
                implicitWidth: 6
                radius: 3
                color: scrollBar.pressed ? Lumen.amber : Lumen.track
            }
        }
    }

    // Model churn load axis: inserts, removes and moves near the visible rows.
    Timer {
        interval: Spike.churnPerSecond >= 60 ? 16 : 1000 / Math.max(1, Spike.churnPerSecond)
        repeat: true
        running: Spike.churnPerSecond > 0 && page.visible
        onTriggered: page.tracks.churn(Spike.churnPerSecond >= 60 ? Math.round(Spike.churnPerSecond * 0.016) : 1,
                                       list.indexAt(10, list.contentY + list.height / 2))
    }
}
