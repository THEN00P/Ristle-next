// Runs the generated Swift bindings against the real library
// (core/tools/check-bindings.sh). The bindings are their own Swift 5
// module, as ADR 0001 requires, so this file also checks that their types,
// including the error enums, are reachable from another module.

import Foundation
import ristle_core

final class NoSecrets: PlatformHost {
    func readSecret(key: String) throws -> Data? { nil }
    func writeSecret(key: String, value: Data) throws {}
    func deleteSecret(key: String) throws {}
}

final class Recorder: CoreObserver, @unchecked Sendable {
    let arrived = DispatchSemaphore(value: 0)
    private let lock = NSLock()
    private var onMainThread: [Bool] = []

    func onSnapshot(snapshot: CoreSnapshot) {
        lock.lock()
        onMainThread.append(Thread.isMainThread)
        lock.unlock()
        arrived.signal()
    }

    func onNotice(notice: PlaybackNotice) {}

    func onLibraryChanged(change: LibraryChange) {}

    var anyOnMainThread: Bool {
        lock.lock()
        defer { lock.unlock() }
        return onMainThread.contains(true)
    }
}

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("Swift bindings: \(message)\n".utf8))
    exit(1)
}

let core = try RistleCore(config: CoreConfig(dataDir: "data", cacheDir: "cache"), platform: NoSecrets())

let recorder = Recorder()
let subscription = core.subscribe(observer: recorder)
if recorder.arrived.wait(timeout: .now() + 5) == .timedOut {
    fail("no snapshot within 5 s")
}
if recorder.anyOnMainThread {
    fail("snapshot arrived on the main thread")
}
subscription.cancel()

do {
    try core.play()
    fail("play() should not be implemented yet")
} catch CoreError.NotReady(let feature) {
    if feature != "play" {
        fail("NotReady names \(feature)")
    }
}

let token = CancelToken()
token.cancel()
do {
    _ = try await core.search(query: "query", scope: .everywhere, cancel: token)
    fail("a cancelled search returned")
} catch CoreError.Cancelled {
    // Expected.
}

print("Swift bindings: ok")
