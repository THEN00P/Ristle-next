pragma Singleton
import QtQuick

// Lumen tokens (docs/design/lumen-spec/guidelines-foundations.md).
QtObject {
    readonly property color deepest: "#0f0c09"
    readonly property color base: "#1b1712"
    readonly property color raised: "#262019"
    readonly property color baseRaise: "#241d15"
    readonly property color track: "#3a322a"
    readonly property color hairline: Qt.rgba(1, 0.94, 0.85, 0.07)
    readonly property color amber: "#eba24a"
    readonly property color amberHigh: "#f6b867"
    readonly property color ember: "#d4632e"
    readonly property color amberWash: Qt.rgba(0.92, 0.635, 0.29, 0.12)
    readonly property color amberLine: Qt.rgba(0.92, 0.635, 0.29, 0.35)
    readonly property color text: "#f5f0e8"
    readonly property color textSecondary: "#cfc6b7"
    readonly property color muted: "#a99e8d"
    readonly property color faint: "#8a8170"
    readonly property color dim: "#7d7466"
    readonly property color inkOnAmber: "#1b1712"
    readonly property var sources: ["#4ad17a", "#ff8a3d", "#9aa0ac", "#5ab0ff"]

    // The bundled static fonts register one family per weight (see
    // lumen-tokens.reference.ts), so weight is part of the family name.
    readonly property string headline: "SchibstedGrotesk-ExtraBold"
    readonly property string headlineBold: "SchibstedGrotesk-Bold"
    readonly property string mono: "DMMono-Regular"
    readonly property string body: "Segoe UI"

    readonly property int rowHeight: 48
    readonly property int queueRowHeight: 52
    readonly property int radius: 8
}
