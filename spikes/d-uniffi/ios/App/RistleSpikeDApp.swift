import SwiftUI

@main
struct RistleSpikeDApp: App {
    @StateObject private var model = PlayheadModel()

    var body: some Scene {
        WindowGroup {
            VStack(spacing: 8) {
                Text("Spike D").font(.headline)
                Text(format(model.state?.positionMs)).font(.system(.body, design: .monospaced))
            }
        }
    }

    private func format(_ ms: UInt64?) -> String {
        guard let ms else { return "--:--.-" }
        return String(format: "%02llu:%02llu.%llu", ms / 60_000, (ms / 1000) % 60, (ms % 1000) / 100)
    }
}

/// Bridges the core's push stream onto the main actor. The core owns the state.
@MainActor
final class PlayheadModel: ObservableObject {
    @Published private(set) var state: PlayerState?
    private let core = Core()
    private var subscription: Subscription?

    init() {
        subscription = core.subscribe(observer: ClosureObserver { [weak self] state in
            Task { @MainActor in self?.state = state }
        })
        let core = self.core
        Task { _ = try? await core.loadTrack(id: "t1") }
    }
}

/// UniFFI's observer protocol is Sendable, and on_state arrives on a core thread.
final class ClosureObserver: StateObserver {
    private let onChange: @Sendable (PlayerState) -> Void

    init(_ onChange: @escaping @Sendable (PlayerState) -> Void) {
        self.onChange = onChange
    }

    func onState(state: PlayerState) {
        onChange(state)
    }
}
