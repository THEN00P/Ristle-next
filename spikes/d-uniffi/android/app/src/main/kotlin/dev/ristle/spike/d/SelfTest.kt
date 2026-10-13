package dev.ristle.spike.d

import android.os.Looper
import android.util.Log
import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeout
import uniffi.ristle_core.Core
import uniffi.ristle_core.CoreException
import uniffi.ristle_core.InternalException
import uniffi.ristle_core.PlayerState
import uniffi.ristle_core.StateObserver
import uniffi.ristle_core.uniffiContinuationHandleMap
import uniffi.ristle_core.uniffiForeignFutureHandleMap

/**
 * Runs once per process on the main dispatcher and logs one SPIKE_SELFTEST line per
 * check: error mapping, panics, cancellation in both directions and threading.
 */
object SelfTest {
    private var failures = 0

    private fun report(name: String, pass: Boolean, detail: String) {
        if (!pass) failures++
        Log.i(TAG, "SPIKE_SELFTEST ${if (pass) "PASS" else "FAIL"} $name: $detail")
    }

    suspend fun run(core: Core) = coroutineScope {
        // Typed error: Rust CoreError::NotFound -> Kotlin CoreException.NotFound.
        try {
            core.loadTrack("missing-1")
            report("typed-error", false, "no exception")
        } catch (e: CoreException.NotFound) {
            report("typed-error", e.id == "missing-1", "NotFound(id=${e.id}) message='${e.message}'")
        }

        // Panic in a sync call -> InternalException; the core stays usable.
        try {
            MainCalls.timed("triggerPanic") { core.triggerPanic() }
            report("panic", false, "no exception")
        } catch (e: InternalException) {
            val usable = runCatching { core.state() }.isSuccess
            report("panic", usable, "InternalException('${e.message}'), core usable afterwards=$usable")
        }

        // Kotlin TokenProvider throwing a declared error -> Rust Err -> Kotlin exception.
        core.setTokenProvider(FakeTokenProvider(failWith = CoreException.Provider("kotlin said no")))
        try {
            core.loadTrack("t0")
            report("callback-typed-error", false, "no exception")
        } catch (e: CoreException.Provider) {
            report("callback-typed-error", e.reason == "kotlin said no", "Provider(reason=${e.reason})")
        }

        // Kotlin TokenProvider throwing an undeclared exception -> UnexpectedUniFFICallbackError.
        core.setTokenProvider(FakeTokenProvider(failWith = IllegalStateException("boom")))
        try {
            core.loadTrack("t0")
            report("callback-unexpected-error", false, "no exception")
        } catch (e: CoreException.Provider) {
            report("callback-unexpected-error", e.reason.contains("boom"), "Provider(reason=${e.reason.take(80)})")
        }

        // Kotlin cancels a Rust future: the Rust task must be aborted, nothing left in the maps.
        val before = core.stats()
        // The screen's own nextState long-poll may be in flight, so compare with a baseline.
        val contBefore = uniffiContinuationHandleMap.size
        repeat(200) {
            val job = launch { core.waitMs(10_000u) }
            delay(2)
            job.cancelAndJoin()
        }
        delay(300)
        val after = core.stats()
        val cancelled = after.futuresCancelled - before.futuresCancelled
        val contAfter = uniffiContinuationHandleMap.size
        report("kotlin-cancels-rust", cancelled == 200uL && contAfter <= contBefore,
            "rust futures cancelled=$cancelled/200 contMap before=$contBefore after=$contAfter")

        // Rust drops a future that awaits Kotlin: the Kotlin coroutine must be cancelled.
        val slow = FakeTokenProvider(delayMs = 10_000)
        core.setTokenProvider(slow)
        val load = async { runCatching { core.loadTrack("t-slow") } }
        delay(200)
        load.cancelAndJoin()
        delay(300)
        report("rust-cancels-kotlin", slow.cancelled.get() == 1 && uniffiForeignFutureHandleMap.size == 0,
            "kotlin token() cancelled=${slow.cancelled.get()} foreignFutMap=${uniffiForeignFutureHandleMap.size} " +
                "token() ran on '${slow.lastThread}'")

        // Threading: where callbacks arrive and where a suspend call resumes.
        var observerThread = ""
        val sub = core.subscribe(object : StateObserver {
            override fun onState(state: PlayerState) { observerThread = Thread.currentThread().name }
        })
        val normal = FakeTokenProvider()
        core.setTokenProvider(normal)
        val info = core.loadTrack("t1")
        val resumedOnMain = Looper.myLooper() == Looper.getMainLooper()
        withTimeout(1_000) { while (observerThread.isEmpty()) delay(20) }
        sub.close()
        // JNA names attached native threads Thread-N unless a CallbackThreadInitializer is set.
        report("threading", resumedOnMain && observerThread.isNotEmpty() && observerThread != "main",
            "loadTrack resumed on main=$resumedOnMain, onState on '$observerThread', token() on '${normal.lastThread}', " +
                "loaded '${info.title}'")

        // Forgetting close(): the subscription lives until the GC cleaner runs.
        val subsBefore = core.stats().liveSubscriptions
        core.subscribe(object : StateObserver { override fun onState(state: PlayerState) {} })
        val leaked = core.stats().liveSubscriptions - subsBefore
        var waitedMs = 0
        while (core.stats().liveSubscriptions > subsBefore && waitedMs < 5_000) {
            Runtime.getRuntime().gc()
            delay(100)
            waitedMs += 100
        }
        report("unclosed-subscription", leaked == 1uL,
            "unclosed subscription stayed registered until GC; released after ~${waitedMs} ms of forced GC " +
                "(live now=${core.stats().liveSubscriptions - subsBefore})")

        Log.i(TAG, "SPIKE_SELFTEST DONE failures=$failures")
    }
}
