import QtQuick
import Ristle.Spike

// Queue panel with animated insert, remove and reorder. Transitions stay
// enabled (Elisa turned them off because of QTBUG-49868).
Item {
    id: panel

    required property var queue
    required property Item overlay
    property alias list: list

    Rectangle {
        anchors.fill: parent
        color: Lumen.base
        radius: 10
        border.color: Lumen.hairline
    }

    Text {
        x: 18
        y: 18
        text: "Queue"
        color: Lumen.text
        font.family: Lumen.headlineBold
        font.weight: Font.Bold
        font.pixelSize: 20
    }

    Text {
        anchors.right: parent.right
        anchors.rightMargin: 18
        y: 22
        text: "Clear"
        color: Lumen.amber
        font.family: Lumen.body
        font.pixelSize: 13
        font.weight: Font.DemiBold
    }

    Text {
        x: 18
        y: 60
        text: "NOW PLAYING"
        color: Lumen.faint
        font.family: Lumen.body
        font.pixelSize: 11
        font.weight: Font.DemiBold
        font.letterSpacing: 1.8
    }

    Rectangle {
        id: nowPlaying
        x: 12
        y: 80
        width: parent.width - 24
        height: 56
        radius: 8
        color: Lumen.amberWash

        Artwork { x: 10; width: 38; height: 38; anchors.verticalCenter: parent.verticalCenter; artId: 7 }

        Column {
            x: 58
            anchors.verticalCenter: parent.verticalCenter
            spacing: 2
            Text { text: "Midnight Pilot"; color: Lumen.amber; font.family: Lumen.body; font.pixelSize: 14; font.weight: Font.DemiBold }
            Text { text: "Parallel · Nightshift EP"; color: Lumen.muted; font.family: Lumen.body; font.pixelSize: 12 }
        }

        // Equalizer glyph. Animators run on the render thread (ADR 0002).
        Row {
            anchors.right: parent.right
            anchors.rightMargin: 14
            anchors.verticalCenter: parent.verticalCenter
            spacing: 2
            Repeater {
                model: 3
                Rectangle {
                    required property int index
                    width: 3
                    height: 14
                    color: Lumen.amber
                    transformOrigin: Item.Bottom
                    // Render-thread Animators halve the GUI thread's frame
                    // rate on flip-model swap chains (spike A finding), so the
                    // default is a GUI-thread animation.
                    SequentialAnimation on scale {
                        loops: Animation.Infinite
                        running: Spike.heartbeat && Spike.animators
                        ScaleAnimator { from: 0.3; to: 1; duration: 300 + index * 90 }
                        ScaleAnimator { from: 1; to: 0.3; duration: 300 + index * 90 }
                    }
                    SequentialAnimation on scale {
                        loops: Animation.Infinite
                        running: Spike.heartbeat && !Spike.animators
                        NumberAnimation { from: 0.3; to: 1; duration: 300 + index * 90 }
                        NumberAnimation { from: 1; to: 0.3; duration: 300 + index * 90 }
                    }
                }
            }
        }
    }

    Text {
        x: 18
        y: 150
        text: "NEXT UP"
        color: Lumen.faint
        font.family: Lumen.body
        font.pixelSize: 11
        font.weight: Font.DemiBold
        font.letterSpacing: 1.8
    }

    ListView {
        id: list
        x: 12
        y: 170
        width: parent.width - 24
        height: parent.height - y - 8
        clip: true
        model: panel.queue
        boundsBehavior: Flickable.StopAtBounds
        acceptedButtons: Qt.NoButton

        delegate: QueueRow {
            queue: panel.queue
            overlay: panel.overlay
        }

        add: Transition {
            NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Spike.transitionMs; easing.type: Easing.OutCubic }
            NumberAnimation { property: "scale"; from: 0.96; to: 1; duration: Spike.transitionMs; easing.type: Easing.OutCubic }
        }
        remove: Transition {
            NumberAnimation { property: "opacity"; to: 0; duration: Spike.transitionMs; easing.type: Easing.OutCubic }
        }
        displaced: Spike.queueFix ? displacedSettling : displacedPlain
        move: Transition {
            NumberAnimation { properties: "x,y"; duration: Spike.transitionMs; easing.type: Easing.OutCubic }
        }
        moveDisplaced: Spike.queueFix ? displacedSettling : displacedPlain

        // Plain: what QTBUG-49868 reports break when transitions interrupt
        // each other (rows left dimmed, overlapping, or leaving gaps).
        Transition {
            id: displacedPlain
            NumberAnimation { properties: "x,y"; duration: Spike.transitionMs; easing.type: Easing.OutCubic }
        }
        // Settling: Qt's documented handling of interrupted transitions
        // (ViewTransition, "Handling Interrupted Animations"): a displaced item
        // may still be mid-add or mid-remove, so drive opacity and scale home.
        Transition {
            id: displacedSettling
            NumberAnimation { properties: "x,y"; duration: Spike.transitionMs; easing.type: Easing.OutCubic }
            NumberAnimation { property: "opacity"; to: 1; duration: Spike.transitionMs }
            NumberAnimation { property: "scale"; to: 1; duration: Spike.transitionMs }
        }
    }
}
