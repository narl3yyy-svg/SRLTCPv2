package com.srltcp.v2

import android.app.Application
import android.util.Log

/**
 * Ensures UniFFI loads `libsrltcp_core.so` (cargo-ndk output name)
 * instead of the default `libuniffi_srltcp_core.so`.
 * Initializes Android JNI context for iroh before the engine starts.
 */
class SrltcpApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        System.setProperty("uniffi.component.srltcp_core.libraryOverride", "srltcp_core")
        // Required for iroh DNS on Android when panic=abort (release builds).
        AndroidInit.installSafely(this)
        SrltcpEngineHolder.init(this)
        SrltcpEngineHolder.startInBackground()
        Log.i("SrltcpApplication", "onCreate complete")
    }
}
