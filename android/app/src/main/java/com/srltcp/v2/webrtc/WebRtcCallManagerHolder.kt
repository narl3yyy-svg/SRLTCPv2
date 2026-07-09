package com.srltcp.v2.webrtc

import android.content.Context
import com.srltcp.v2.CallState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import com.srltcp.v2.SrltcpEngineHolder

object WebRtcCallManagerHolder {
    private var manager: WebRtcCallManager? = null

    suspend fun handleSignal(
        context: Context,
        peerId: String,
        callId: String,
        signal: String,
        payload: String,
        isVideo: Boolean,
        onState: (CallState) -> Unit,
    ) {
        val mgr = manager ?: WebRtcCallManager(context.applicationContext).also { manager = it }
        val engine = SrltcpEngineHolder.getOrCreate()
        when (signal) {
            "offer" -> mgr.handleOffer(callId, payload, isVideo, { ice ->
                engine.sendCallSignal(peerId, callId, "ice", ice, isVideo)
            }) { answer ->
                engine.sendCallSignal(peerId, callId, "answer", answer, isVideo)
                onState(CallState(callId, peerId, isVideo))
            }
            "answer" -> {
                mgr.handleAnswer(payload)
                onState(CallState(callId, peerId, isVideo))
            }
            "ice" -> mgr.handleIce(payload)
        }
    }

    suspend fun startOutgoing(
        context: Context,
        peerId: String,
        isVideo: Boolean,
        onState: (CallState) -> Unit,
    ): String = withContext(Dispatchers.IO) {
        val mgr = manager ?: WebRtcCallManager(context.applicationContext).also { manager = it }
        val engine = SrltcpEngineHolder.getOrCreate()
        var activeCallId = ""
        mgr.startOutgoing(isVideo, { ice ->
            engine.sendCallSignal(peerId, activeCallId, "ice", ice, isVideo)
        }) { callId, offer ->
            activeCallId = callId
            engine.sendCallSignal(peerId, callId, "offer", offer, isVideo)
            onState(CallState(callId, peerId, isVideo))
        }
    }

    fun end() {
        manager?.end()
        manager = null
    }
}