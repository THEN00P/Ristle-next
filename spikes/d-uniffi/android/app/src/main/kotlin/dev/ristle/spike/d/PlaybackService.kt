package dev.ristle.spike.d

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import uniffi.ristle_core.Core
import uniffi.ristle_core.PlaybackStatus
import uniffi.ristle_core.PlayerState
import uniffi.ristle_core.StateObserver
import java.util.concurrent.atomic.AtomicLong

class PlaybackService : MediaSessionService() {
    private var session: MediaSession? = null

    override fun onCreate() {
        super.onCreate()
        val app = application as SpikeApp
        session = MediaSession.Builder(this, CorePlayer(app.core, app)).build()
        Log.i(TAG, "SPIKE_SERVICE created")
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo) = session

    override fun onDestroy() {
        session?.run {
            player.release()
            release()
        }
        session = null
        Log.i(TAG, "SPIKE_SERVICE destroyed")
        super.onDestroy()
    }
}

/**
 * A Media3 Player that only mirrors core state. The core owns playback; play and
 * pause from the notification or a Bluetooth headset go back to the core.
 */
@androidx.annotation.OptIn(UnstableApi::class)
class CorePlayer(private val core: Core, private val app: SpikeApp) : SimpleBasePlayer(Looper.getMainLooper()) {
    @Volatile private var latest: PlayerState = core.state()
    private val main = Handler(Looper.getMainLooper())
    private val updates = AtomicLong(0)
    private var lastLogAt = SystemClock.elapsedRealtime()

    // Called on a core thread. Media3 requires its application looper, so hop.
    private val subscription = core.subscribe(object : StateObserver {
        override fun onState(state: PlayerState) {
            latest = state
            val n = updates.incrementAndGet()
            if (n % 100 == 0L) {
                val now = SystemClock.elapsedRealtime()
                Log.i(TAG, "SPIKE_SERVICE updates=$n pos=${state.positionMs} status=${state.status} " +
                    "fg=${app.inForeground} last100In=${now - lastLogAt}ms thread=${Thread.currentThread().name}")
                lastLogAt = now
            }
            main.post { invalidateState() }
        }
    })

    override fun getState(): State {
        val s = latest
        val builder = State.Builder()
            .setAvailableCommands(
                Player.Commands.Builder()
                    .addAll(Player.COMMAND_PLAY_PAUSE, Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
                        Player.COMMAND_GET_METADATA, Player.COMMAND_GET_TIMELINE, Player.COMMAND_RELEASE)
                    .build(),
            )
            .setPlayWhenReady(s.status == PlaybackStatus.PLAYING || s.status == PlaybackStatus.LOADING,
                Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
            .setPlaybackState(
                when (s.status) {
                    PlaybackStatus.IDLE -> Player.STATE_IDLE
                    PlaybackStatus.LOADING -> Player.STATE_BUFFERING
                    PlaybackStatus.PLAYING, PlaybackStatus.PAUSED -> Player.STATE_READY
                },
            )
            .setContentPositionMs(s.positionMs.toLong())
        s.trackId?.let { id ->
            val item = MediaItem.Builder()
                .setMediaId(id)
                .setMediaMetadata(MediaMetadata.Builder().setTitle(s.title).setArtist("Ristle spike D").build())
                .build()
            builder.setPlaylist(
                listOf(MediaItemData.Builder(id).setMediaItem(item).setDurationUs(s.durationMs.toLong() * 1000).build()),
            )
        }
        return builder.build()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        if (playWhenReady) core.play() else core.pause()
        return Futures.immediateVoidFuture()
    }

    override fun handleRelease(): ListenableFuture<*> {
        subscription.close()
        main.removeCallbacksAndMessages(null)
        return Futures.immediateVoidFuture()
    }
}
