import QtQuick
import Ristle.Spike

// Rounded artwork. Corners are cut in the decode worker (probe.cpp), so this
// is one texture per row. Until the decode lands, the Lumen generated-cover
// gradient shows instead of an empty box.
Item {
    id: art

    property int artId
    property int radiusPct: 15
    property int unique: 0

    Rectangle {
        anchors.fill: parent
        radius: art.width * art.radiusPct / 100
        visible: image.status !== Image.Ready
        gradient: Gradient {
            GradientStop { position: 0; color: Lumen.track }
            GradientStop { position: 1; color: Lumen.baseRaise }
        }
    }

    Image {
        id: image
        anchors.fill: parent
        sourceSize: Qt.size(art.width, art.height)
        cache: Spike.artCache
        source: Spike.artCache ? "image://art/" + art.artId + "/r" + art.radiusPct
                               : "image://art/" + art.artId + "/r" + art.radiusPct + "/u" + art.unique
    }
}
