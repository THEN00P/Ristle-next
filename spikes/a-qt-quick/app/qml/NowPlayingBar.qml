import QtQuick
import QtQuick.Effects
import Ristle.Spike

// Transport bar. The progress fill is driven by a GUI-thread animation on
// purpose: it is the heartbeat that makes every vsync produce a frame, so a
// stalled GUI thread shows up as a missing present.
Item {
    id: bar

    property real progress: 0

    NumberAnimation on progress {
        from: 0
        to: 1
        duration: 248000
        loops: Animation.Infinite
        running: Spike.heartbeat
    }

    // Glow level 2: a blurred backdrop of the current artwork behind the bar.
    Item {
        anchors.fill: parent
        visible: Spike.glow >= 2
        clip: true
        Artwork { id: backdropArt; width: parent.width; height: parent.width; y: -parent.width / 2; artId: 7; radiusPct: 0; visible: false }
        MultiEffect {
            anchors.fill: backdropArt
            source: backdropArt
            blurEnabled: true
            blur: 1.0
            blurMax: 64
            saturation: -0.2
            brightness: -0.55
        }
    }

    Rectangle {
        anchors.fill: parent
        color: Lumen.deepest
        opacity: Spike.glow >= 2 ? 0.55 : 1
    }

    Rectangle {
        width: parent.width
        height: 1
        color: Lumen.hairline
    }

    Artwork { x: 20; width: 50; height: 50; anchors.verticalCenter: parent.verticalCenter; artId: 7 }

    Column {
        x: 82
        anchors.verticalCenter: parent.verticalCenter
        spacing: 3
        Text { text: "Midnight Pilot"; color: Lumen.text; font.family: Lumen.body; font.pixelSize: 15; font.weight: Font.DemiBold }
        Text { text: "Parallel"; color: Lumen.muted; font.family: Lumen.body; font.pixelSize: 12 }
    }

    Item {
        id: transport
        width: 420
        height: parent.height
        anchors.horizontalCenter: parent.horizontalCenter

        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            y: 8
            spacing: 26
            Text { text: "⇄"; color: Lumen.textSecondary; font.pixelSize: 18; anchors.verticalCenter: parent.verticalCenter }
            Text { text: "⏮"; color: Lumen.textSecondary; font.pixelSize: 16; anchors.verticalCenter: parent.verticalCenter }
            Item {
                width: 42
                height: 42
                Rectangle {
                    id: play
                    anchors.fill: parent
                    radius: 21
                    gradient: Gradient {
                        GradientStop { position: 0; color: Lumen.amber }
                        GradientStop { position: 1; color: Lumen.ember }
                    }
                    Text { anchors.centerIn: parent; text: "❚❚"; color: Lumen.inkOnAmber; font.pixelSize: 13 }
                    visible: Spike.glow === 0
                }
                // The Lumen glow: amber is light, not paint.
                MultiEffect {
                    anchors.fill: play
                    source: play
                    visible: Spike.glow >= 1
                    shadowEnabled: true
                    shadowColor: Lumen.amber
                    shadowOpacity: 0.85
                    shadowBlur: 1.0
                    shadowScale: 1.12
                    blurMax: 32
                    autoPaddingEnabled: true
                }
            }
            Text { text: "⏭"; color: Lumen.textSecondary; font.pixelSize: 16; anchors.verticalCenter: parent.verticalCenter }
            Text { text: "↻"; color: Lumen.textSecondary; font.pixelSize: 18; anchors.verticalCenter: parent.verticalCenter }
        }

        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            y: 60
            spacing: 12
            Text { text: "1:42"; color: Lumen.muted; font.family: Lumen.mono; font.pixelSize: 11; anchors.verticalCenter: parent.verticalCenter }
            Item {
                width: 320
                height: 4
                anchors.verticalCenter: parent.verticalCenter
                Rectangle { anchors.fill: parent; radius: 2; color: Lumen.track }
                Rectangle {
                    width: parent.width * bar.progress
                    height: parent.height
                    radius: 2
                    gradient: Gradient {
                        orientation: Gradient.Horizontal
                        GradientStop { position: 0; color: Lumen.amber }
                        GradientStop { position: 1; color: Lumen.ember }
                    }
                }
                Rectangle {
                    x: parent.width * bar.progress - 6
                    width: 12
                    height: 12
                    radius: 6
                    anchors.verticalCenter: parent.verticalCenter
                    color: Lumen.text
                }
            }
            Text { text: "4:08"; color: Lumen.muted; font.family: Lumen.mono; font.pixelSize: 11; anchors.verticalCenter: parent.verticalCenter }
        }
    }

    Row {
        anchors.right: parent.right
        anchors.rightMargin: 24
        anchors.verticalCenter: parent.verticalCenter
        spacing: 16
        Row {
            spacing: 6
            anchors.verticalCenter: parent.verticalCenter
            Rectangle { width: 7; height: 7; radius: 3.5; color: Lumen.sources[0]; anchors.verticalCenter: parent.verticalCenter }
            Text { text: "Spotify"; color: Lumen.textSecondary; font.family: Lumen.body; font.pixelSize: 13 }
        }
        Rectangle { width: 90; height: 4; radius: 2; color: Lumen.track; anchors.verticalCenter: parent.verticalCenter
            Rectangle { width: 60; height: 4; radius: 2; color: Lumen.textSecondary }
        }
    }
}
