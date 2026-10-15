package dev.ristle.spike.b

import android.app.Activity
import android.content.ComponentName
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import com.google.common.util.concurrent.MoreExecutors

/** Transport through a MediaController, so every action takes the same path as the notification. */
class MainActivity : Activity() {
    private var controllerFuture: ListenableFuture<MediaController>? = null
    private val main = Handler(Looper.getMainLooper())
    private lateinit var status: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(32, 96, 32, 32)
        }
        status = TextView(this).apply {
            textSize = 15f
            contentDescription = "status"
        }
        fun button(label: String, action: (MediaController) -> Unit) = Button(this).apply {
            text = label
            contentDescription = label
            setOnClickListener { controllerFuture?.takeIf { it.isDone }?.get()?.let(action) }
        }
        root.addView(button("Play") { it.play() })
        root.addView(button("Pause") { it.pause() })
        root.addView(button("Next") { it.seekToNextMediaItem() })
        root.addView(button("Seek +30 s") { it.seekTo(it.currentPosition + 30_000) })
        root.addView(status)
        setContentView(root)
    }

    override fun onStart() {
        super.onStart()
        val token = SessionToken(this, ComponentName(this, PlaybackService::class.java))
        controllerFuture = MediaController.Builder(this, token).buildAsync().also {
            it.addListener({
                // `am start ... --ez autoplay true` (soak script): press Play through the controller.
                if (intent.getBooleanExtra("autoplay", false)) {
                    intent.removeExtra("autoplay")
                    it.get().play()
                }
                refresh()
            }, MoreExecutors.directExecutor())
        }
    }

    private fun refresh() {
        val app = application as SpikeApp
        val p = app.player
        status.text = if (p == null) {
            "core: ${app.initError ?: "starting"}"
        } else {
            val s = p.state()
            val st = p.stats()
            "${s.status}  ${s.trackIndex + 1u}/${s.trackCount} ${s.title}\n${s.positionMs / 1000u} s / ${s.durationMs / 1000u} s\n" +
                "starved ${st.starvedCallbacks}, xruns ${st.xruns}, transitions ${st.transitions}\n" +
                "decode CPU ${st.decodeCpuMs} ms, callback CPU ${st.callbackCpuMs} ms" + (s.error?.let { "\nerror: $it" } ?: "")
        }
        main.postDelayed(::refresh, 500)
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        if (intent.getBooleanExtra("autoplay", false)) {
            intent.removeExtra("autoplay")
            controllerFuture?.takeIf { it.isDone }?.get()?.play()
        }
    }

    override fun onStop() {
        main.removeCallbacksAndMessages(null)
        controllerFuture?.let { MediaController.releaseFuture(it) }
        controllerFuture = null
        super.onStop()
    }
}
