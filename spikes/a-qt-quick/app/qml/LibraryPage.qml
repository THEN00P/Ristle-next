import QtQuick
import QtQuick.Effects
import Ristle.Spike

// "Your Library": the second heavy page, a grid of MediaCards.
Item {
    id: page

    required property var tracks

    Rectangle {
        anchors.fill: parent
        color: Lumen.base
        radius: 10
        border.color: Lumen.hairline
    }

    Text {
        x: 32
        y: 24
        text: "Your Library"
        color: Lumen.text
        font.family: Lumen.headline
        font.weight: Font.ExtraBold
        font.pixelSize: 34
    }

    Row {
        anchors.right: parent.right
        anchors.rightMargin: 32
        y: 32
        spacing: 8
        Repeater {
            model: ["Playlists", "Albums", "Local"]
            Rectangle {
                required property string modelData
                required property int index
                width: label.implicitWidth + 28
                height: 28
                radius: 14
                color: index === 0 ? Lumen.amber : Lumen.raised
                Text {
                    id: label
                    anchors.centerIn: parent
                    text: parent.modelData
                    color: parent.index === 0 ? Lumen.inkOnAmber : Lumen.textSecondary
                    font.family: Lumen.body
                    font.pixelSize: 13
                    font.weight: Font.DemiBold
                }
            }
        }
    }

    GridView {
        id: grid
        x: 24
        y: 90
        width: page.width - 48
        height: page.height - y - 8
        clip: true
        model: page.tracks
        reuseItems: true
        cellWidth: 148
        cellHeight: 186
        boundsBehavior: Flickable.StopAtBounds
        acceptedButtons: Qt.NoButton

        delegate: Item {
            id: card
            required property int index
            required property string title
            required property string artist
            required property int art
            required property int source
            required property int seed
            width: grid.cellWidth
            height: grid.cellHeight

            HoverHandler { id: hover }

            Item {
                x: 8
                y: hover.hovered ? 6 : 8
                width: 128
                height: 170

                RectangularShadow {
                    width: 128
                    height: 128
                    radius: 10
                    blur: 18
                    offset.y: 6
                    color: Qt.rgba(0, 0, 0, 0.45)
                }
                Artwork {
                    width: 128
                    height: 128
                    radiusPct: 8
                    artId: card.art
                    unique: card.seed
                }
                Text {
                    y: 136
                    width: 128
                    text: card.title
                    elide: Text.ElideRight
                    color: hover.hovered ? Lumen.amber : Lumen.text
                    font.family: Lumen.body
                    font.pixelSize: 13
                    font.weight: Font.Medium
                }
                Row {
                    y: 155
                    spacing: 6
                    Rectangle {
                        width: 6
                        height: 6
                        radius: 3
                        anchors.verticalCenter: parent.verticalCenter
                        color: Lumen.sources[card.source]
                    }
                    Text {
                        text: card.artist
                        color: Lumen.muted
                        font.family: Lumen.body
                        font.pixelSize: 12
                    }
                }
            }
        }
    }
}
