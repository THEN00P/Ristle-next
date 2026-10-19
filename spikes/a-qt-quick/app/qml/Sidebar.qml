import QtQuick
import Ristle.Spike

// Left pane: navigation and playlists. Collapses to icons.
Item {
    id: sidebar

    property bool open: true
    property int page: 0
    signal navigate(int page)

    clip: true

    Column {
        x: 12
        y: 12
        width: sidebar.width - 24
        spacing: 2

        Repeater {
            model: [
                { glyph: "⌂", label: "Home", page: -1 },
                { glyph: "⌕", label: "Search", page: -1 },
                { glyph: "≋", label: "Feed", page: -1 },
                { glyph: "♪", label: "Liked Songs", page: 0 },
                { glyph: "≡", label: "Your Library", page: 1 }
            ]
            Rectangle {
                id: item
                required property var modelData
                width: parent.width
                height: 36
                radius: 8
                color: sidebar.page === modelData.page ? Lumen.amberWash : (hover.hovered ? Lumen.hairline : "transparent")

                HoverHandler { id: hover }
                TapHandler { onTapped: sidebar.navigate(item.modelData.page) }

                Rectangle {
                    x: 0
                    width: 3
                    height: 18
                    radius: 1.5
                    anchors.verticalCenter: parent.verticalCenter
                    color: Lumen.amber
                    visible: sidebar.page === item.modelData.page
                }
                Text {
                    x: 14
                    anchors.verticalCenter: parent.verticalCenter
                    text: item.modelData.glyph
                    color: Lumen.textSecondary
                    font.pixelSize: 16
                }
                Text {
                    x: 42
                    anchors.verticalCenter: parent.verticalCenter
                    text: item.modelData.label
                    color: sidebar.page === item.modelData.page ? Lumen.text : Lumen.textSecondary
                    font.family: Lumen.body
                    font.pixelSize: 14
                    opacity: sidebar.open ? 1 : 0
                }
            }
        }

        Item { width: 1; height: 18 }

        Text {
            x: 12
            text: "PLAYLISTS"
            color: Lumen.faint
            font.family: Lumen.body
            font.pixelSize: 11
            font.weight: Font.DemiBold
            font.letterSpacing: 1.8
            opacity: sidebar.open ? 1 : 0
        }

        Item { width: 1; height: 6 }

        Repeater {
            model: [
                { name: "Night Drive", source: 1, mix: true },
                { name: "Slow Burn", source: 1, mix: true },
                { name: "Daily Drift", source: 0, mix: false },
                { name: "Vault 2019", source: 1, mix: false },
                { name: "Crate Digs", source: 2, mix: false },
                { name: "Cloud Picks", source: 3, mix: false }
            ]
            Item {
                required property var modelData
                width: parent.width
                height: 32
                Rectangle {
                    x: 14
                    width: 8
                    height: 8
                    radius: 2
                    anchors.verticalCenter: parent.verticalCenter
                    color: Lumen.sources[parent.modelData.source]
                }
                Text {
                    x: 42
                    anchors.verticalCenter: parent.verticalCenter
                    text: parent.modelData.name
                    color: Lumen.textSecondary
                    font.family: Lumen.body
                    font.pixelSize: 14
                    opacity: sidebar.open ? 1 : 0
                }
                Text {
                    anchors.right: parent.right
                    anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    text: "MIX"
                    visible: parent.modelData.mix && sidebar.open
                    color: Lumen.faint
                    font.family: Lumen.mono
                    font.pixelSize: 10
                }
            }
        }
    }
}
