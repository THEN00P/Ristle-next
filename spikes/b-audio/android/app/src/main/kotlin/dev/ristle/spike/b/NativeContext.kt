package dev.ristle.spike.b

import android.content.Context

/** Hands the JavaVM and application context to the Rust side for cpal (see player/src/android.rs). */
object NativeContext {
    init {
        System.loadLibrary("ristle_audio")
    }

    external fun init(context: Context)
}
