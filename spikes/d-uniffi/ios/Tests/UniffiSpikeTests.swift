import Darwin
import XCTest
@testable import RistleSpikeD

// Each test prints RISTLE_SPIKE_D lines; the workflow collects them from the log.
// Observations (Swift cancellation, panics) are logged rather than asserted so a
// single expensive macOS run reports everything.

private func log(_ line: String) {
    print("RISTLE_SPIKE_D \(line)")
}

private func threadName() -> String {
    if pthread_main_np() != 0 { return "main" }
    var buf = [CChar](repeating: 0, count: 64)
    pthread_getname_np(pthread_self(), &buf, buf.count)
    let name = String(cString: buf)
    return name.isEmpty ? "unnamed" : name
}

private func seconds(since start: Date) -> String {
    String(format: "%.2f", Date().timeIntervalSince(start))
}

final class Collector: StateObserver, @unchecked Sendable {
    private let lock = NSLock()
    private var _states: [PlayerState] = []
    private var _threads = Set<String>()

    func onState(state: PlayerState) {
        let name = threadName()
        lock.lock()
        _states.append(state)
        _threads.insert(name)
        lock.unlock()
    }

    var states: [PlayerState] {
        lock.lock()
        defer { lock.unlock() }
        return _states
    }

    var threads: Set<String> {
        lock.lock()
        defer { lock.unlock() }
        return _threads
    }
}

final class SwiftTokenProvider: TokenProvider, @unchecked Sendable {
    enum Mode { case ok, typedError, otherError, slow }
    struct Boom: Error {}

    let mode: Mode
    private let lock = NSLock()
    private var _cancelled = 0
    private var _thread = ""

    init(_ mode: Mode) { self.mode = mode }

    var cancelled: Int {
        lock.lock()
        defer { lock.unlock() }
        return _cancelled
    }

    var thread: String {
        lock.lock()
        defer { lock.unlock() }
        return _thread
    }

    func token() async throws -> String {
        let name = threadName()
        lock.lock()
        _thread = name
        lock.unlock()
        switch mode {
        case .ok:
            return "swift-token"
        case .typedError:
            throw CoreError.Provider(reason: "swift said no")
        case .otherError:
            throw Boom()
        case .slow:
            do {
                try await Task.sleep(nanoseconds: 10_000_000_000)
            } catch {
                lock.lock()
                _cancelled += 1
                lock.unlock()
                throw error
            }
            return "late"
        }
    }
}

final class UniffiSpikeTests: XCTestCase {
    func testPushStream() async throws {
        let core = Core()
        let collector = Collector()
        let sub = core.subscribe(observer: collector)
        _ = try await core.loadTrack(id: "t1")
        core.resetMaxCallback()
        let before = collector.states.count
        try await Task.sleep(nanoseconds: 2_000_000_000)
        let n = collector.states.count - before
        let positions = collector.states.filter { $0.status == .playing }.map(\.positionMs)
        let increasing = zip(positions, positions.dropFirst()).allSatisfy { $1 > $0 }
        let stats = core.stats()
        let avgUs = stats.callbacksDelivered > 0 ? stats.totalCallbackUs / stats.callbacksDelivered : 0
        log("push updates_in_2s=\(n) increasing=\(increasing) threads=\(collector.threads.sorted()) " +
            "cbAvgUs=\(avgUs) cbMaxUs=\(stats.maxCallbackUs)")
        XCTAssertTrue((15...25).contains(n), "updates in 2 s: \(n)")
        XCTAssertTrue(increasing)
        XCTAssertFalse(collector.threads.contains("main"))
        sub.cancel()
        XCTAssertEqual(core.stats().liveSubscriptions, 0)
    }

    func testPullStreamAsAsyncStream() async throws {
        let core = Core()
        _ = try await core.loadTrack(id: "t1")
        let stream = AsyncStream<PlayerState> { continuation in
            let task = Task {
                var seq: UInt64 = 0
                while !Task.isCancelled {
                    let state = await core.nextState(afterSeq: seq)
                    seq = state.seq
                    continuation.yield(state)
                }
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
        var seqs: [UInt64] = []
        let start = Date()
        for await state in stream {
            seqs.append(state.seq)
            if seqs.count == 10 { break }
        }
        let elapsed = seconds(since: start)
        try await Task.sleep(nanoseconds: 300_000_000)
        let s = core.stats()
        let increasing = zip(seqs, seqs.dropFirst()).allSatisfy { $1 > $0 }
        log("pull states=\(seqs.count) in=\(elapsed)s increasing=\(increasing) " +
            "futures started=\(s.futuresStarted) completed=\(s.futuresCompleted) cancelled=\(s.futuresCancelled)")
        XCTAssertEqual(seqs.count, 10)
        XCTAssertTrue(increasing)
    }

    func testTypedError() async {
        let core = Core()
        do {
            _ = try await core.loadTrack(id: "missing-1")
            XCTFail("no error")
        } catch CoreError.NotFound(let id) {
            log("typed-error NotFound(id: \(id)) description='\(CoreError.NotFound(id: id).localizedDescription)'")
            XCTAssertEqual(id, "missing-1")
        } catch {
            XCTFail("unexpected error \(error)")
        }
    }

    func testCallbackErrors() async {
        let core = Core()
        let typed = SwiftTokenProvider(.typedError)
        core.setTokenProvider(provider: typed)
        do {
            _ = try await core.loadTrack(id: "t0")
            XCTFail("no error")
        } catch CoreError.Provider(let reason) {
            log("callback-typed-error Provider(reason: \(reason)) token() on '\(typed.thread)'")
            XCTAssertEqual(reason, "swift said no")
        } catch {
            XCTFail("unexpected error \(error)")
        }

        core.setTokenProvider(provider: SwiftTokenProvider(.otherError))
        do {
            _ = try await core.loadTrack(id: "t0")
            XCTFail("no error")
        } catch CoreError.Provider(let reason) {
            log("callback-unexpected-error Provider(reason: \(reason))")
            XCTAssertTrue(reason.contains("unexpected"))
        } catch {
            XCTFail("unexpected error \(error)")
        }
    }

    /// Observation: does cancelling a Swift Task cancel the Rust future?
    func testSwiftTaskCancellation() async {
        let core = Core()
        let before = core.stats().futuresCancelled
        let start = Date()
        let task = Task { await core.waitMs(ms: 1500) }
        try? await Task.sleep(nanoseconds: 100_000_000)
        task.cancel()
        let value = await task.value
        let elapsed = seconds(since: start)
        let cancelled = core.stats().futuresCancelled - before
        log("swift-task-cancel elapsed=\(elapsed)s (rust sleep 1.50s) value=\(value) rustFuturesCancelled=\(cancelled)")
    }

    /// Mitigation: forward Task cancellation to a core CancelToken.
    func testCancelTokenMitigation() async {
        let core = Core()
        let token = CancelToken()
        let start = Date()
        let task = Task {
            try await withTaskCancellationHandler {
                try await core.waitMsCancellable(ms: 10_000, token: token)
            } onCancel: {
                token.cancel()
            }
        }
        try? await Task.sleep(nanoseconds: 100_000_000)
        task.cancel()
        let result = await task.result
        let elapsed = Date().timeIntervalSince(start)
        var gotCancelled = false
        if case .failure(let error) = result, let coreError = error as? CoreError, coreError == .Cancelled {
            gotCancelled = true
        }
        log("cancel-token elapsed=\(String(format: "%.2f", elapsed))s gotCoreErrorCancelled=\(gotCancelled)")
        XCTAssertTrue(gotCancelled)
        XCTAssertLessThan(elapsed, 1.0)
    }

    /// The core drops a future that awaits Swift: the Swift task must be cancelled.
    func testRustCancelsSwift() async {
        let core = Core()
        let slow = SwiftTokenProvider(.slow)
        core.setTokenProvider(provider: slow)
        let start = Date()
        do {
            _ = try await core.probeToken(timeoutMs: 200)
            XCTFail("no error")
        } catch CoreError.Cancelled {
        } catch {
            XCTFail("unexpected error \(error)")
        }
        try? await Task.sleep(nanoseconds: 300_000_000)
        log("rust-cancels-swift elapsed=\(seconds(since: start))s swiftTaskCancelled=\(slow.cancelled) token() on '\(slow.thread)'")
        XCTAssertEqual(slow.cancelled, 1)
    }

    /// A panic in a throwing function. (A panic in a non-throwing one hits `try!`
    /// in the generated code and kills the process, so it is not run here.)
    func testPanicInFallibleFunction() {
        let core = Core()
        do {
            try core.tryPanic()
            XCTFail("no error")
        } catch {
            log("panic-fallible type=\(type(of: error)) isCoreError=\(error is CoreError) message='\(error.localizedDescription)'")
        }
        _ = core.state()
    }

    @MainActor
    func testResumesOnMainActor() async throws {
        let core = Core()
        _ = try await core.loadTrack(id: "t1")
        let onMain = pthread_main_np() != 0
        log("main-actor resumedOnMain=\(onMain)")
        XCTAssertTrue(onMain)
    }

    func testSubscriptionChurnAndRelease() {
        let core = Core()
        let cycles = 10_000
        let start = Date()
        for _ in 0..<cycles {
            core.subscribe(observer: Collector()).cancel()
        }
        let perCycleUs = Date().timeIntervalSince(start) / Double(cycles) * 1e6

        // Dropping a Subscription without cancel(): ARC frees it immediately.
        do { _ = core.subscribe(observer: Collector()) }
        let liveAfterDrop = core.stats().liveSubscriptions

        // The observer object must be released once the core lets go of it.
        weak var weakObserver: Collector?
        do {
            let observer = Collector()
            weakObserver = observer
            core.subscribe(observer: observer).cancel()
        }
        log("churn cycles=\(cycles) perCycleUs=\(String(format: "%.1f", perCycleUs)) " +
            "liveAfterDropWithoutCancel=\(liveAfterDrop) observerReleased=\(weakObserver == nil)")
        XCTAssertEqual(liveAfterDrop, 0)
        XCTAssertNil(weakObserver)
    }
}
