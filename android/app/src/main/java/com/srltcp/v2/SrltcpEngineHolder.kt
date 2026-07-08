package com.srltcp.v2

import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import uniffi.srltcp_core.SrltcpEngine
import uniffi.srltcp_core.SrltcpEvent
import uniffi.srltcp_core.initCrypto

/**
 * Process-wide singleton holding the Rust P2P engine.
 * Survives Activity destruction; kept alive by Foreground Service.
 */
object SrltcpEngineHolder {
    private const val TAG = "SrltcpEngineHolder"
    private const val QUIC_PORT: UShort = 9473u

    @Volatile
    private var engine: SrltcpEngine? = null

    private var scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val eventListeners = mutableSetOf<(SrltcpEvent) -> Unit>()
    @Volatile
    private var polling = false

    private fun ensureNativeLibrary() {
        System.setProperty("uniffi.component.srltcp_core.libraryOverride", "srltcp_core")
    }

    @Synchronized
    fun getOrCreate(): SrltcpEngine {
        ensureNativeLibrary()
        engine?.let { return it }
        val job = scope.coroutineContext[Job]
        if (job == null || !job.isActive) {
            scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
        }
        Log.i(TAG, "Creating Rust SrltcpEngine via UniFFI")
        initCrypto()
        val eng = SrltcpEngine()
        eng.start(QUIC_PORT)
        engine = eng
        polling = false
        startEventPolling(eng)
        Log.i(TAG, "Engine started on QUIC port $QUIC_PORT")
        return eng
    }

    private fun startEventPolling(eng: SrltcpEngine) {
        if (polling) return
        polling = true
        scope.launch {
            while (isActive && engine != null) {
                val drained = eng.drainEvents()
                if (drained.isNotEmpty()) {
                    synchronized(eventListeners) {
                        drained.forEach { event ->
                            eventListeners.forEach { it(event) }
                        }
                    }
                }
                delay(100)
            }
        }
    }

    fun addEventListener(listener: (SrltcpEvent) -> Unit) {
        synchronized(eventListeners) { eventListeners.add(listener) }
    }

    fun removeEventListener(listener: (SrltcpEvent) -> Unit) {
        synchronized(eventListeners) { eventListeners.remove(listener) }
    }

    fun isEngineRunning(): Boolean = engine?.isRunning() ?: false

    /** Only on explicit ACTION_STOP — not on swipe-away. */
    @Synchronized
    fun shutdown() {
        Log.i(TAG, "Shutting down Rust engine")
        engine?.shutdown()
        engine = null
        polling = false
        scope.cancel()
    }
}