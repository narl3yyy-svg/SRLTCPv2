package com.srltcp.v2

import android.app.Application

/**
 * Ensures UniFFI loads `libsrltcp_core.so` (cargo-ndk output name)
 * instead of the default `libuniffi_srltcp_core.so`.
 * Initializes the engine holder with application context for identity storage.
 */
class SrltcpApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        System.setProperty("uniffi.component.srltcp_core.libraryOverride", "srltcp_core")
        SrltcpEngineHolder.init(this)
        SrltcpEngineHolder.startInBackground()
    }
}
