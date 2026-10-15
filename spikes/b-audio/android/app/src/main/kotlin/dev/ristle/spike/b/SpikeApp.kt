package dev.ristle.spike.b

import android.app.Application
import android.os.Handler
import android.os.HandlerThread
import android.os.PowerManager
import android.os.Process
import android.os.SystemClock
import android.util.Log
import com.sun.jna.Callback
import com.sun.jna.CallbackThreadInitializer
import com.sun.jna.Native
import uniffi.ristle_audio.Player
import uniffi.ristle_audio.uniffiCallbackInterfaceStateObserver
import java.io.File
import java.util.concurrent.CountDownLatch

const val TAG = "SPIKE_B"

/** Album played on loop: the gapless test tracks, MP3 then AAC (host edit-list trim). */
val ALBUM = listOf("t0.mp3", "t1.mp3", "t2.mp3", "t0.m4a", "t1.m4a", "t2.m4a")

class SpikeApp : Application() {
    @Volatile var player: Player? = null
        private set
    private val ready = CountDownLatch(1)
    @Volatile var initError: String? = null
    private var wakeLock: PowerManager.WakeLock? = null

    override fun onCreate() {
        super.onCreate()
        // Keep core threads attached to the JVM between callbacks (spike D, sharp edge 1).
        val init = CallbackThreadInitializer(true, false, "ristle-core-cb")
        listOf<Callback>(
            uniffiCallbackInterfaceStateObserver.onState,
            uniffiCallbackInterfaceStateObserver.uniffiFree,
            uniffiCallbackInterfaceStateObserver.uniffiClone,
        ).forEach { Native.setCallbackThreadInitializer(it, init) }
        // JNA's first use and the core's construction stay off the main thread (sharp edge 2).
        Thread({
            try {
                // Pushed by build-app.sh. A subdirectory created by adb would be owned by shell and unreadable.
                val dir = getExternalFilesDir(null)!!
                val paths = ALBUM.map { File(dir, it).absolutePath }
                val missing = paths.filterNot { File(it).exists() }
                check(missing.isEmpty()) { "push the album first: missing $missing" }
                NativeContext.init(applicationContext)
                player = Player(paths, ALBUM, true)
                Log.i(TAG, "SPIKE_B_INIT player ready, album=${ALBUM.size} tracks")
            } catch (e: Throwable) {
                initError = e.toString()
                Log.e(TAG, "SPIKE_B_INIT failed", e)
            } finally {
                ready.countDown()
            }
        }, "core-init").start()
        startHealthLog()
    }

    fun awaitPlayer(): Player? {
        ready.await()
        return player
    }

    /** `adb shell run-as dev.ristle.spike.b touch files/wakelock` holds a partial wake lock while playing. */
    fun setPlaying(playing: Boolean) {
        if (!File(filesDir, "wakelock").exists()) return
        val pm = getSystemService(PowerManager::class.java)
        if (playing && wakeLock == null) {
            wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "ristle:playback").apply { acquire() }
        } else if (!playing) {
            wakeLock?.release()
            wakeLock = null
        }
    }

    /** Every 30 s, off the main thread: position, underruns, CPU, screen and Doze state. */
    private fun startHealthLog() {
        val thread = HandlerThread("spike-health").apply { start() }
        val handler = Handler(thread.looper)
        val pm = getSystemService(PowerManager::class.java)
        val startedAt = SystemClock.elapsedRealtime()
        val startCpu = Process.getElapsedCpuTime()
        handler.post(object : Runnable {
            override fun run() {
                val p = player
                if (p != null) {
                    val s = p.state()
                    val st = p.stats()
                    val wallMs = SystemClock.elapsedRealtime() - startedAt
                    val procCpuMs = Process.getElapsedCpuTime() - startCpu
                    Log.i(
                        TAG,
                        "SPIKE_B_HEALTH t=${wallMs / 1000}s status=${s.status} track=${s.trackIndex}:${s.title} " +
                            "pos=${s.positionMs}/${s.durationMs} played=${st.framesPlayed / st.sampleRate.toULong()}s " +
                            "starved=${st.starvedCallbacks} xruns=${st.xruns} cbs=${st.callbacks} maxCbUs=${st.maxCallbackUs} " +
                            "transitions=${st.transitions} decodeCpuMs=${st.decodeCpuMs} cbCpuMs=${st.callbackCpuMs} procCpuMs=$procCpuMs " +
                            "screenOn=${pm.isInteractive} idle=${pm.isDeviceIdleMode} err=${s.error}",
                    )
                }
                handler.postDelayed(this, 30_000)
            }
        })
    }
}
