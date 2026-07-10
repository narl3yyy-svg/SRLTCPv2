package com.srltcp.v2

import android.util.Log
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
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

    private val ready = CompletableDeferred<SrltcpEngine>()
    @Volatile
    private var starting = false

    private var scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val eventListeners = mutableSetOf<(SrltcpEvent) -> Unit>()
    @Volatile
    private var polling = false

    private fun ensureNativeLibrary() {
        System.setProperty("uniffi.component.srltcp_core.libraryOverride", "srltcp_core")
    }

    /** Non-blocking — returns null until [awaitEngine] completes. */
    fun engineOrNull(): SrltcpEngine? = engine

    fun isEngineReady(): Boolean = engine != null && (engine?.isRunning() == true)

    /** Starts engine on IO if needed; safe from any thread. */
    fun startInBackground() {
        if (engine != null || starting) return
        synchronized(this) {
            if (engine != null || starting) return
            starting = true
            val job = scope.coroutineContext[Job]
            if (job == null || !job.isActive) {
                scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
            }
            scope.launch {
                try {
                    createEngineLocked()
                } catch (e: Exception) {
                    Log.e(TAG, "Engine start failed", e)
                    starting = false
                    if (!ready.isCompleted) ready.completeExceptionally(e)
                }
            }
        }
    }

    suspend fun awaitEngine(): SrltcpEngine {
        engine?.let { return it }
        startInBackground()
        return withContext(Dispatchers.IO) {
            if (engine != null) return@withContext engine!!
            ready.await()
        }
    }

    @Synchronized
    private fun createEngineLocked(): SrltcpEngine {
        engine?.let { return it }
        ensureNativeLibrary()
        Log.i(TAG, "Creating Rust SrltcpEngine via UniFFI")
        initCrypto()
        val eng = SrltcpEngine()
        engine = eng
        starting = false
        polling = false
        startEventPolling(eng)
        if (!ready.isCompleted) ready.complete(eng)
        // iroh bind + online() can take 30–60s on mobile (or hang on SELinux).
        // Never block UI on this — wait in background.
        scope.launch {
            try {
                eng.waitUntilReady(60u)
                Log.i(TAG, "iroh endpoint ready")
            } catch (e: Exception) {
                Log.e(TAG, "iroh ready wait failed", e)
            }
        }
        Log.i(TAG, "Engine instance ready (iroh connecting in background)")
        return eng
    }

    /** @deprecated Use [awaitEngine] from coroutines — never call from main thread before ready. */
    @Synchronized
    fun getOrCreate(): SrltcpEngine {
        engine?.let { return it }
        return createEngineLocked()
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

    @Synchronized
    fun shutdown() {
        Log.i(TAG, "Shutting down Rust engine")
        engine?.shutdown()
        engine = null
        starting = false
        polling = false
        scope.cancel()
    }
}