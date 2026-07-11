package com.srltcp.v2

import android.content.Context
import android.util.Log

/**
 * Installs Android Application Context into the Rust/iroh layer (ndk_context)
 * so system DNS lookups work. Must run before the engine starts networking.
 */
object AndroidInit {
    private const val TAG = "AndroidInit"

    init {
        try {
            System.loadLibrary("srltcp_core")
        } catch (e: UnsatisfiedLinkError) {
            Log.e(TAG, "Failed to load libsrltcp_core.so", e)
            throw e
        }
    }

    /** Native: `Java_com_srltcp_v2_AndroidInit_install` in core/src/android_init.rs */
    @JvmStatic
    external fun install(context: Context)

    fun installSafely(context: Context) {
        try {
            install(context.applicationContext)
            Log.i(TAG, "iroh ndk_context installed")
        } catch (e: Throwable) {
            Log.e(TAG, "install failed — iroh DNS may abort", e)
        }
    }
}
