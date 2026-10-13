import SwiftUI

@main
struct BaselineApp: App {
    var body: some Scene {
        WindowGroup {
            VStack(spacing: 8) {
                Text("Spike D").font(.headline)
                Text("--:--.-").font(.system(.body, design: .monospaced))
            }
        }
    }
}
