// Runs the generated Kotlin bindings against the real library on the JVM
// (core/tools/check-bindings.sh). Checks the ADR 0001 conventions from the
// platform's side: callbacks arrive off the calling thread, errors arrive
// typed, and a CancelToken ends an async call.

import java.nio.file.Files
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.runBlocking
import uniffi.ristle_core.*

class NoSecrets : PlatformHost {
    override fun readSecret(key: String): ByteArray? = null
    override fun writeSecret(key: String, value: ByteArray) {}
    override fun deleteSecret(key: String) {}
}

class Recorder : CoreObserver {
    val threads = ArrayBlockingQueue<Thread>(16)

    override fun onSnapshot(snapshot: CoreSnapshot) {
        threads.add(Thread.currentThread())
    }

    override fun onNotice(notice: PlaybackNotice) {}

    override fun onLibraryChanged(change: LibraryChange) {}
}

fun ensure(condition: Boolean, message: () -> String) {
    if (!condition) throw AssertionError(message())
}

fun main() {
    val dir = Files.createTempDirectory("ristle-smoke")
    val config = CoreConfig(
        dataDir = dir.resolve("data").toString(),
        cacheDir = dir.resolve("cache").toString(),
    )
    RistleCore(config, NoSecrets()).use { core ->
        val recorder = Recorder()
        core.subscribe(recorder).use { subscription ->
            val thread = recorder.threads.poll(5, TimeUnit.SECONDS)
            ensure(thread != null) { "no snapshot within 5 s" }
            ensure(thread !== Thread.currentThread()) { "snapshot arrived on the calling thread" }
            subscription.cancel()
        }

        try {
            core.play()
            throw AssertionError("play() should not be implemented yet")
        } catch (e: CoreException.NotReady) {
            ensure(e.feature == "play") { "NotReady names ${e.feature}" }
        }

        val token = CancelToken()
        token.cancel()
        try {
            runBlocking { core.search("query", SearchScope.EVERYWHERE, token) }
            throw AssertionError("a cancelled search returned")
        } catch (e: CoreException.Cancelled) {
            // Expected.
        }
    }
    println("Kotlin bindings: ok")
}
