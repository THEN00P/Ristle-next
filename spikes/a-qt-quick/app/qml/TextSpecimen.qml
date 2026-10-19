import QtQuick
import Ristle.Spike

// Text sharpness specimen (scenario 5): the same strings in Qt's three text
// render types, on Lumen's dark base and on white, at fixed positions so the
// harness can crop them from duplicated frames.
Rectangle {
    id: specimen

    color: Lumen.base

    readonly property var samples: [
        { family: Lumen.body, size: 12, weight: Font.Normal, text: "Floating Points · Crush · 3:51" },
        { family: Lumen.body, size: 13, weight: Font.Normal, text: "Ólafur Arnalds · re:member" },
        { family: Lumen.body, size: 14, weight: Font.Medium, text: "Midnight Pilot (Edit) — Saturn" },
        { family: Lumen.mono, size: 12, weight: Font.Normal, text: "1:42 / 4:08 · 320 kbps" },
        { family: Lumen.headline, size: 28, weight: Font.ExtraBold, text: "Liked Songs" }
    ]

    Repeater {
        model: 2
        Rectangle {
            id: band
            required property int index
            y: index * 360
            width: specimen.width
            height: 360
            color: index === 0 ? Lumen.base : "#ffffff"

            Repeater {
                model: [Text.QtRendering, Text.NativeRendering, Text.CurveRendering]
                Column {
                    id: column
                    required property int modelData
                    required property int index
                    x: 24 + index * 410
                    y: 24
                    spacing: 18
                    Text {
                        text: ["QtRendering", "NativeRendering", "CurveRendering"][column.index]
                        color: band.index === 0 ? Lumen.amber : "#a0522d"
                        font.family: Lumen.mono
                        font.pixelSize: 11
                    }
                    Repeater {
                        model: specimen.samples
                        Text {
                            required property var modelData
                            renderType: column.modelData
                            text: modelData.text
                            color: band.index === 0 ? Lumen.text : "#000000"
                            font.family: modelData.family
                            font.pixelSize: modelData.size
                            font.weight: modelData.weight
                        }
                    }
                }
            }
        }
    }
}
