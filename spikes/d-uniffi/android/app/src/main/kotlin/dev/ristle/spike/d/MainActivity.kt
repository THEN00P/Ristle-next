package dev.ristle.spike.d

import android.content.ComponentName
import android.os.Bundle
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleStartEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.launch
import uniffi.ristle_core.Core
import uniffi.ristle_core.CoreException
import uniffi.ristle_core.CoreStats
import uniffi.ristle_core.InternalException
import uniffi.ristle_core.PlaybackStatus
import uniffi.ristle_core.PlayerState
import uniffi.ristle_core.StateObserver

class MainActivity : ComponentActivity() {
    private var controller: ListenableFuture<MediaController>? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val app = application as SpikeApp
        if (!app.selfTestStarted) {
            app.selfTestStarted = true
            app.scope.launch { SelfTest.run(app.core) }
        }
        setContent {
            MaterialTheme {
                Surface(Modifier.fillMaxSize()) { SpikeScreen(app.core) }
            }
        }
    }

    override fun onStart() {
        super.onStart()
        // Connecting a controller starts the MediaSessionService, which subscribes
        // to the same core on its own.
        val token = SessionToken(this, ComponentName(this, PlaybackService::class.java))
        controller = MediaController.Builder(this, token).buildAsync()
    }

    override fun onStop() {
        controller?.let { MediaController.releaseFuture(it) }
        controller = null
        super.onStop()
    }
}

/** Pull-style stream: one async long-poll per state. Cancelled with the collector. */
fun Core.stateFlow(): Flow<PlayerState> = flow {
    var seq = 0uL
    while (true) {
        val s = nextState(seq)
        seq = s.seq
        emit(s)
    }
}

@Composable
fun SpikeScreen(core: Core) {
    // Push-style stream: a callback from a core thread into a StateFlow. Subscribed
    // only while the screen is started, so a backgrounded app leaves only the service.
    val pushed = remember { MutableStateFlow<PlayerState?>(null) }
    var pushCount by remember { mutableStateOf(0L) }
    LifecycleStartEffect(core) {
        val sub = MainCalls.timed("subscribe") {
            core.subscribe(object : StateObserver {
                override fun onState(state: PlayerState) { pushed.value = state }
            })
        }
        onStopOrDispose {
            MainCalls.timed("closeSubscription") { sub.close() }
        }
    }
    val push by pushed.collectAsStateWithLifecycle()
    LaunchedEffect(push) { if (push != null) pushCount++ }

    val pull by remember(core) { core.stateFlow() }.collectAsStateWithLifecycle(null)

    var stats by remember { mutableStateOf<CoreStats?>(null) }
    LaunchedEffect(core) {
        while (true) {
            stats = MainCalls.timed("stats") { core.stats() }
            delay(1_000)
        }
    }

    var message by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()

    Column(
        Modifier.safeDrawingPadding().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text("Spike D: UniFFI", style = MaterialTheme.typography.headlineSmall)
        Text("Status: ${push?.status ?: "-"}  ${push?.title ?: ""}")
        Mono("push  ${fmt(push?.positionMs)}  (#$pushCount)")
        Mono("pull  ${fmt(pull?.positionMs)}  (seq ${pull?.seq ?: 0u})")
        stats?.let {
            Mono("subs=${it.liveSubscriptions} callbacks=${it.callbacksDelivered} maxCbUs=${it.maxCallbackUs}")
            Mono("futures ${it.futuresStarted}/${it.futuresCompleted}/${it.futuresCancelled} (start/done/cancel)")
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = {
                scope.launch {
                    message = runCatching { core.loadTrack("t2").title }.fold({ "Loaded $it" }, { "Error: $it" })
                }
            }) { Text("Load") }
            Button(onClick = {
                scope.launch {
                    message = try {
                        core.loadTrack("missing-7"); "unexpected success"
                    } catch (e: CoreException.NotFound) {
                        "NotFound(id=${e.id})"
                    }
                }
            }) { Text("Load missing") }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = {
                MainCalls.timed("playPause") {
                    if (core.state().status == PlaybackStatus.PLAYING) core.pause() else core.play()
                }
            }) { Text("Play/Pause") }
            Button(onClick = {
                message = try {
                    core.triggerPanic(); "no panic?"
                } catch (e: InternalException) {
                    "Panic -> InternalException(${e.message})"
                }
            }) { Text("Panic") }
        }
        if (message.isNotEmpty()) Text(message)
    }
    LaunchedEffect(message) { if (message.isNotEmpty()) Log.i(TAG, "SPIKE_UI $message") }
}

@Composable
private fun Mono(text: String) = Text(text, fontFamily = FontFamily.Monospace)

private fun fmt(ms: ULong?): String {
    if (ms == null) return "--:--.-"
    val t = ms.toLong()
    return "%02d:%02d.%d".format(t / 60_000, (t / 1000) % 60, (t % 1000) / 100)
}
