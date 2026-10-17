package dev.ristle.spike.b

import android.app.PendingIntent
import android.content.Intent
import android.os.Handler
import android.os.Looper
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
import uniffi.ristle_audio.PlayerState
import uniffi.ristle_audio.StateObserver
import uniffi.ristle_audio.Status

class PlaybackService : MediaSessionService() {
    private var session: MediaSession? = null

    override fun onCreate() {
        super.onCreate()
        val app = application as SpikeApp
        val core = app.awaitPlayer() ?: error("core failed: ${app.initError}")
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        session = MediaSession.Builder(this, CorePlayer(core, app)).setSessionActivity(open).build()
        Log.i(TAG, "SPIKE_B_SERVICE created")
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo) = session

    override fun onDestroy() {
        session?.run {
            player.release()
            release()
        }
        session = null
        Log.i(TAG, "SPIKE_B_SERVICE destroyed")
        super.onDestroy()
    }
}

/**
 * Media3 facade. The Rust core owns queue, position and transport; this only
 * mirrors `PlayerState` and forwards play/pause/seek/skip from the
 * notification, lock screen and headset buttons.
 */
@androidx.annotation.OptIn(UnstableApi::class)
class CorePlayer(
    private val core: uniffi.ristle_audio.Player,
    private val app: SpikeApp,
) : SimpleBasePlayer(Looper.getMainLooper()) {
    @Volatile private var latest: PlayerState = core.state()
    private val main = Handler(Looper.getMainLooper())

    private val observer = object : StateObserver {
        // Core thread. Media3 wants its application looper, so hop.
        override fun onState(state: PlayerState) {
            latest = state
            main.post { invalidateState() }
        }
    }

    init {
        core.setObserver(observer)
    }

    private val items = ALBUM.mapIndexed { i, name ->
        MediaItem.Builder()
            .setMediaId("$i")
            .setMediaMetadata(
                MediaMetadata.Builder().setTitle(name).setArtist("Ristle spike B").setAlbumTitle("Gapless test album")
                    .setTrackNumber(i + 1).build(),
            )
            .build()
    }

    override fun getState(): State {
        val s = latest
        val playlist = items.mapIndexed { i, item ->
            val d = MediaItemData.Builder(i).setMediaItem(item)
            if (i == s.trackIndex.toInt() && s.durationMs > 0uL) d.setDurationUs(s.durationMs.toLong() * 1000)
            d.build()
        }
        return State.Builder()
            .setAvailableCommands(
                Player.Commands.Builder().addAll(
                    Player.COMMAND_PLAY_PAUSE, Player.COMMAND_STOP,
                    Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM, Player.COMMAND_SEEK_TO_MEDIA_ITEM,
                    Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
                    Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM,
                    Player.COMMAND_GET_CURRENT_MEDIA_ITEM, Player.COMMAND_GET_TIMELINE,
                    Player.COMMAND_GET_METADATA, Player.COMMAND_RELEASE,
                ).build(),
            )
            .setPlaylist(playlist)
            .setCurrentMediaItemIndex(s.trackIndex.toInt())
            .setContentPositionMs(s.positionMs.toLong())
            .setPlayWhenReady(s.status == Status.PLAYING || s.status == Status.BUFFERING, Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
            .setPlaybackState(
                when (s.status) {
                    Status.BUFFERING -> Player.STATE_BUFFERING
                    Status.ERROR -> Player.STATE_IDLE
                    else -> Player.STATE_READY
                },
            )
            .setRepeatMode(Player.REPEAT_MODE_ALL)
            .build()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        Log.i(TAG, "SPIKE_B_COMMAND playWhenReady=$playWhenReady")
        if (playWhenReady) core.play() else core.pause()
        app.setPlaying(playWhenReady)
        return Futures.immediateVoidFuture()
    }

    override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> {
        Log.i(TAG, "SPIKE_B_COMMAND seek index=$mediaItemIndex pos=$positionMs command=$seekCommand")
        core.seek(mediaItemIndex.coerceAtLeast(0).toUInt(), positionMs.coerceAtLeast(0).toULong())
        return Futures.immediateVoidFuture()
    }

    override fun handleStop(): ListenableFuture<*> {
        core.pause()
        app.setPlaying(false)
        return Futures.immediateVoidFuture()
    }

    override fun handleRelease(): ListenableFuture<*> {
        main.removeCallbacksAndMessages(null)
        return Futures.immediateVoidFuture()
    }
}
