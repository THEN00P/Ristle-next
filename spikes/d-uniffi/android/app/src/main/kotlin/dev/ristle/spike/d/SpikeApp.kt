package dev.ristle.spike.d

import android.app.Activity
import android.app.Application
import android.os.Bundle
import android.os.Debug
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.StrictMode
import android.os.SystemClock
import android.util.Log
import com.sun.jna.Callback
import com.sun.jna.CallbackThreadInitializer
import com.sun.jna.Native
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import uniffi.ristle_core.Core
import uniffi.ristle_core.TokenProvider
import uniffi.ristle_core.uniffiCallbackInterfaceStateObserver
import uniffi.ristle_core.uniffiCallbackInterfaceTokenProvider
import uniffi.ristle_core.uniffiContinuationHandleMap
import uniffi.ristle_core.uniffiForeignFutureDroppedCallbackImpl
import uniffi.ristle_core.uniffiRustFutureContinuationCallbackImpl
import uniffi.ristle_core.uniffiForeignFutureHandleMap
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong

const val TAG = "SPIKE"

class SpikeApp : Application() {
    lateinit var core: Core
        private set
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    val startedActivities = AtomicInteger(0)
    var selfTestStarted = false
    var jnaKeepAttached = false
        private set
    val lag = MainThreadLagMonitor()

    override fun onCreate() {
        StrictMode.setThreadPolicy(StrictMode.ThreadPolicy.Builder().detectAll().penaltyLog().build())
        StrictMode.setVmPolicy(StrictMode.VmPolicy.Builder().detectAll().penaltyLog().build())
        super.onCreate()

        // Deliberately on the main thread: measures JNA library load + core construction.
        val t0 = SystemClock.elapsedRealtimeNanos()
        core = Core()
        val ms = (SystemClock.elapsedRealtimeNanos() - t0) / 1e6
        Log.i(TAG, "SPIKE_INIT core constructed on main thread in %.1f ms".format(ms))
        core.setTokenProvider(FakeTokenProvider())

        // By default JNA attaches a native thread to the JVM for one callback and
        // detaches it afterwards, which creates a new java.lang.Thread per callback.
        // Keep core threads attached instead. `adb shell run-as dev.ristle.spike.d
        // touch files/jna-default` disables this to measure the difference.
        jnaKeepAttached = !java.io.File(filesDir, "jna-default").exists()
        if (jnaKeepAttached) {
            val init = CallbackThreadInitializer(true, false, "ristle-core-cb")
            listOf<Callback>(
                uniffiCallbackInterfaceStateObserver.onState,
                uniffiCallbackInterfaceStateObserver.uniffiFree,
                uniffiCallbackInterfaceStateObserver.uniffiClone,
                uniffiCallbackInterfaceTokenProvider.token,
                uniffiCallbackInterfaceTokenProvider.uniffiFree,
                uniffiCallbackInterfaceTokenProvider.uniffiClone,
                uniffiRustFutureContinuationCallbackImpl,
                uniffiForeignFutureDroppedCallbackImpl,
            ).forEach { Native.setCallbackThreadInitializer(it, init) }
        }
        Log.i(TAG, "SPIKE_INIT jnaKeepAttached=$jnaKeepAttached")

        registerActivityLifecycleCallbacks(object : ActivityLifecycleCallbacks {
            override fun onActivityStarted(a: Activity) { startedActivities.incrementAndGet() }
            override fun onActivityStopped(a: Activity) { startedActivities.decrementAndGet() }
            override fun onActivityCreated(a: Activity, b: Bundle?) {}
            override fun onActivityResumed(a: Activity) {}
            override fun onActivityPaused(a: Activity) {}
            override fun onActivitySaveInstanceState(a: Activity, b: Bundle) {}
            override fun onActivityDestroyed(a: Activity) {}
        })
        lag.start()
        startHealthLog()
    }

    val inForeground get() = startedActivities.get() > 0

    /** Every 30 s, off the main thread: leak and blocking indicators. */
    private fun startHealthLog() {
        val thread = HandlerThread("spike-health").apply { start() }
        val handler = Handler(thread.looper)
        val startedAt = SystemClock.elapsedRealtime()
        var prevCallbacks = 0uL
        var prevTotalUs = 0uL
        handler.post(object : Runnable {
            override fun run() {
                val s = core.stats()
                core.resetMaxCallback()
                val n = s.callbacksDelivered - prevCallbacks
                val avgUs = if (n > 0uL) (s.totalCallbackUs - prevTotalUs) / n else 0uL
                prevCallbacks = s.callbacksDelivered
                prevTotalUs = s.totalCallbackUs
                val rt = Runtime.getRuntime()
                Log.i(TAG, "SPIKE_HEALTH t=${(SystemClock.elapsedRealtime() - startedAt) / 1000}s " +
                    "fg=$inForeground subs=${s.liveSubscriptions} ticks=${s.ticks} callbacks=${s.callbacksDelivered} " +
                    "cbAvgUs=$avgUs cbMaxUs=${s.maxCallbackUs} threads=${Thread.activeCount()} futures=${s.futuresStarted}/${s.futuresCompleted}/${s.futuresCancelled} " +
                    "contMap=${uniffiContinuationHandleMap.size} foreignFutMap=${uniffiForeignFutureHandleMap.size} " +
                    "javaHeapKb=${(rt.totalMemory() - rt.freeMemory()) / 1024} nativeHeapKb=${Debug.getNativeHeapAllocatedSize() / 1024} " +
                    "mainLagMaxMs=${lag.takeMax()} maxMainFfiCallUs=${MainCalls.summary()}")
                handler.postDelayed(this, 30_000)
            }
        })
    }
}

/** Kotlin implementation of the core's async foreign trait. */
class FakeTokenProvider(
    private val delayMs: Long = 20,
    private val failWith: Exception? = null,
) : TokenProvider {
    val cancelled = AtomicInteger(0)
    @Volatile var lastThread = ""

    override suspend fun token(): String {
        lastThread = Thread.currentThread().name
        try {
            delay(delayMs)
        } catch (e: kotlinx.coroutines.CancellationException) {
            cancelled.incrementAndGet()
            throw e
        }
        failWith?.let { throw it }
        return "fake-token"
    }
}

/** Times synchronous FFI calls made on the main thread. */
object MainCalls {
    private val maxUs = ConcurrentHashMap<String, AtomicLong>()

    inline fun <T> timed(name: String, block: () -> T): T {
        val t0 = System.nanoTime()
        try {
            return block()
        } finally {
            record(name, (System.nanoTime() - t0) / 1000)
        }
    }

    fun record(name: String, us: Long) {
        check(Looper.myLooper() == Looper.getMainLooper()) { "$name not on main" }
        maxUs.getOrPut(name) { AtomicLong() }.accumulateAndGet(us, ::maxOf)
    }

    fun summary(): String = maxUs.entries.sortedBy { it.key }.joinToString(",") { "${it.key}:${it.value.get()}" }
}

/** Posts to the main looper every 50 ms from another thread and records the worst delay. */
class MainThreadLagMonitor {
    private val main = Handler(Looper.getMainLooper())
    private val max = AtomicLong(0)

    fun start() {
        val thread = HandlerThread("spike-lag").apply { start() }
        val bg = Handler(thread.looper)
        bg.post(object : Runnable {
            override fun run() {
                val posted = SystemClock.uptimeMillis()
                main.post { max.accumulateAndGet(SystemClock.uptimeMillis() - posted, ::maxOf) }
                bg.postDelayed(this, 50)
            }
        })
    }

    fun takeMax(): Long = max.getAndSet(0)
}
