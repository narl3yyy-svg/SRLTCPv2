package com.srltcp.v2.webrtc

import android.content.Context
import com.srltcp.v2.CallState
import com.srltcp.v2.SrltcpEngineHolder
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import org.webrtc.SurfaceViewRenderer

object WebRtcCallManagerHolder {
    private var manager: WebRtcCallManager? = null

    private fun mgr(context: Context): WebRtcCallManager =
        manager ?: WebRtcCallManager(context.applicationContext).also { manager = it }

    suspend fun handleSignal(
        context: Context,
        peerId: String,
        callId: String,
        signal: String,
        payload: String,
        isVideo: Boolean,
        onState: (CallState) -> Unit,
    ) = withContext(Dispatchers.IO) {
        val m = mgr(context)
        val engine = SrltcpEngineHolder.awaitEngine()
        val notify: (CallState) -> Unit = { state ->
            CoroutineScope(Dispatchers.Main).launch { onState(state) }
        }
        when (signal) {
            "offer" -> m.handleOffer(callId, payload, isVideo, { ice ->
                engine.sendCallSignal(peerId, callId, "ice", ice, isVideo)
            }) { answer ->
                engine.sendCallSignal(peerId, callId, "answer", answer, isVideo)
                notify(CallState(callId, peerId, isVideo))
            }
            "answer" -> {
                m.handleAnswer(payload)
                notify(CallState(callId, peerId, isVideo))
            }
            "ice" -> m.handleIce(payload)
        }
    }

    suspend fun startOutgoing(
        context: Context,
        peerId: String,
        isVideo: Boolean,
        onState: (CallState) -> Unit,
    ): String = withContext(Dispatchers.IO) {
        val m = mgr(context)
        val engine = SrltcpEngineHolder.awaitEngine()
        var activeCallId = ""
        m.startOutgoing(isVideo, { ice ->
            engine.sendCallSignal(peerId, activeCallId, "ice", ice, isVideo)
        }) { callId, offer ->
            activeCallId = callId
            engine.sendCallSignal(peerId, callId, "offer", offer, isVideo)
            CoroutineScope(Dispatchers.Main).launch {
                onState(CallState(callId, peerId, isVideo))
            }
        }
    }

    fun bindLocal(renderer: SurfaceViewRenderer) {
        manager?.bindLocal(renderer)
    }

    fun bindRemote(renderer: SurfaceViewRenderer) {
        manager?.bindRemote(renderer)
    }

    fun setMute(muted: Boolean) {
        manager?.setMute(muted)
    }

    fun end() {
        manager?.end()
        manager = null
    }
}