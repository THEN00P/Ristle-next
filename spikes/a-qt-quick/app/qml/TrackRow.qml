import QtQuick
import QtQuick.Effects
import Ristle.Spike

// Desktop TrackRow: # · title and artist · album · date added · duration ·
// hover actions. `weight` is the delegate-weight load axis:
// 0 text only, 1 artwork, 2 artwork + MultiEffect glow + hover states,
// 3 artwork + RectangularShadow glow + hover states.
Item {
    id: row

    required property int index
    required property int number
    required property string title
    required property string artist
    required property string album
    required property string added
    required property string duration
    required property int art
    required property int source
    required property bool liked
    required property int seed

    property int weight: Spike.weight
    property real albumX
    property real addedX
    property real durationX

    height: Lumen.rowHeight

    HoverHandler {
        id: hover
        enabled: row.weight >= 2
    }

    Rectangle {
        anchors.fill: parent
        radius: 6
        color: row.index === 2 ? Lumen.amberWash : (hover.hovered ? Lumen.hairline : "transparent")
    }

    Text {
        x: 8
        width: 32
        anchors.verticalCenter: parent.verticalCenter
        horizontalAlignment: Text.AlignRight
        text: row.number
        color: row.index === 2 ? Lumen.amber : Lumen.faint
        font.family: Lumen.mono
        font.pixelSize: 12
    }

    Loader {
        id: artLoader
        x: 52
        width: 36
        height: 36
        anchors.verticalCenter: parent.verticalCenter
        active: row.weight >= 1
        sourceComponent: row.weight === 2 ? glowArt : (row.weight === 3 ? shadowArt : plainArt)
    }

    Component {
        id: plainArt
        Artwork { artId: row.art; unique: row.seed }
    }

    Component {
        id: glowArt
        Item {
            Artwork { id: glowSource; anchors.fill: parent; artId: row.art; unique: row.seed; visible: false }
            MultiEffect {
                anchors.fill: parent
                source: glowSource
                shadowEnabled: true
                shadowColor: Lumen.amber
                shadowOpacity: hover.hovered ? 0.7 : 0.35
                shadowBlur: 0.8
                blurMax: 24
                autoPaddingEnabled: true
            }
        }
    }

    Component {
        id: shadowArt
        Item {
            RectangularShadow {
                anchors.fill: parent
                radius: 5
                blur: 12
                spread: 1
                color: hover.hovered ? Lumen.amberLine : Qt.rgba(0.92, 0.635, 0.29, 0.18)
            }
            Artwork { anchors.fill: parent; artId: row.art; unique: row.seed }
        }
    }

    Column {
        x: row.weight >= 1 ? 100 : 52
        width: row.albumX - x - 16
        anchors.verticalCenter: parent.verticalCenter
        spacing: 2

        Text {
            width: parent.width
            text: row.title
            elide: Text.ElideRight
            color: row.index === 2 ? Lumen.amber : Lumen.text
            font.family: Lumen.body
            font.pixelSize: 14
            font.weight: Font.Medium
            font.underline: hover.hovered
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
        x: row.albumX
        width: row.addedX - row.albumX - 16
        anchors.verticalCenter: parent.verticalCenter
        text: row.album
        elide: Text.ElideRight
        color: Lumen.textSecondary
        font.family: Lumen.body
        font.pixelSize: 13
    }

    Text {
        x: row.addedX
        anchors.verticalCenter: parent.verticalCenter
        text: row.added
        color: Lumen.muted
        font.family: Lumen.body
        font.pixelSize: 13
    }

    Row {
        x: row.durationX - 60
        anchors.verticalCenter: parent.verticalCenter
        spacing: 12
        visible: hover.hovered || row.index === 2

        Text {
            text: row.liked ? "♥" : "♡"
            color: row.liked ? Lumen.amber : Lumen.muted
            font.pixelSize: 15
        }
        Text {
            text: "⋯"
            color: Lumen.muted
            font.pixelSize: 15
        }
    }

    Text {
        x: row.durationX
        width: 48
        anchors.verticalCenter: parent.verticalCenter
        horizontalAlignment: Text.AlignRight
        text: row.duration
        color: Lumen.muted
        font.family: Lumen.mono
        font.pixelSize: 12
    }
}
