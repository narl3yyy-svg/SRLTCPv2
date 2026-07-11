package com.srltcp.v2.webrtc

import android.content.Context
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import org.json.JSONObject
import org.webrtc.AudioSource
import org.webrtc.AudioTrack
import org.webrtc.Camera2Enumerator
import org.webrtc.DataChannel
import org.webrtc.DefaultVideoDecoderFactory
import org.webrtc.DefaultVideoEncoderFactory
import org.webrtc.EglBase
import org.webrtc.IceCandidate
import org.webrtc.MediaConstraints
import org.webrtc.MediaStream
import org.webrtc.PeerConnection
import org.webrtc.PeerConnectionFactory
import org.webrtc.RtpReceiver
import org.webrtc.RtpTransceiver
import org.webrtc.SdpObserver
import org.webrtc.SessionDescription
import org.webrtc.RendererCommon
import org.webrtc.SurfaceViewRenderer
import org.webrtc.VideoCapturer
import org.webrtc.VideoSource
import org.webrtc.VideoTrack
import java.util.UUID

class WebRtcCallManager(private val context: Context) {
    private val egl = EglBase.create()
    private val mainHandler = Handler(Looper.getMainLooper())
    private var factory: PeerConnectionFactory? = null
    private var peerConnection: PeerConnection? = null
    private var localAudio: AudioTrack? = null
    private var localVideo: VideoTrack? = null
    private var remoteVideo: VideoTrack? = null
    private var videoCapturer: VideoCapturer? = null
    private var localRenderer: SurfaceViewRenderer? = null
    private var remoteRenderer: SurfaceViewRenderer? = null
    private val pendingIce = mutableListOf<IceCandidate>()
    var onConnectionLost: (() -> Unit)? = null
    var recvOnlyVideo = false
        private set
    var recvOnlyAudio = false
        private set

    private fun enableCallAudio() {
        val am = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return
        am.mode = AudioManager.MODE_IN_COMMUNICATION
        @Suppress("DEPRECATION")
        am.isSpeakerphoneOn = true
        // Ensure call volume path is active
        try {
            am.requestAudioFocus(
                null,
                AudioManager.STREAM_VOICE_CALL,
                AudioManager.AUDIOFOCUS_GAIN_TRANSIENT,
            )
        } catch (_: Exception) {
        }
    }

    private fun disableCallAudio() {
        val am = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return
        am.mode = AudioManager.MODE_NORMAL
        @Suppress("DEPRECATION")
        am.isSpeakerphoneOn = false
    }

    fun setSpeakerphone(on: Boolean) {
        val am = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return
        am.mode = AudioManager.MODE_IN_COMMUNICATION
        @Suppress("DEPRECATION")
        am.isSpeakerphoneOn = on
    }

    private fun ensureFactory() {
        if (factory != null) return
        PeerConnectionFactory.initialize(
            PeerConnectionFactory.InitializationOptions.builder(context)
                .createInitializationOptions(),
        )
        factory = PeerConnectionFactory.builder()
            .setVideoEncoderFactory(DefaultVideoEncoderFactory(egl.eglBaseContext, true, true))
            .setVideoDecoderFactory(DefaultVideoDecoderFactory(egl.eglBaseContext))
            .createPeerConnectionFactory()
    }

    fun startOutgoing(
        isVideo: Boolean,
        onIce: (String) -> Unit,
        onOffer: (String, String) -> Unit,
    ): String {
        ensureFactory()
        enableCallAudio()
        end()
        recvOnlyVideo = false
        recvOnlyAudio = false
        val callId = UUID.randomUUID().toString()
        val pc = createPeer(onIce)
        peerConnection = pc

        addAudioTrack(pc)
        val gotLocalVideo = if (isVideo) addCameraTrack(pc) else false
        if (isVideo && !gotLocalVideo) {
            recvOnlyVideo = true
            pc.addTransceiver(
                org.webrtc.MediaStreamTrack.MediaType.MEDIA_TYPE_VIDEO,
                RtpTransceiver.RtpTransceiverInit(RtpTransceiver.RtpTransceiverDirection.RECV_ONLY),
            )
        }

        val constraints = MediaConstraints().apply {
            mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveAudio", "true"))
            mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveVideo", if (isVideo) "true" else "false"))
        }
        val createOffer = {
            pc.createOffer(object : SdpObserverAdapter() {
                override fun onCreateSuccess(desc: SessionDescription?) {
                    desc ?: return
                    pc.setLocalDescription(SdpObserverAdapter(), desc)
                    onOffer(callId, desc.description)
                }
                override fun onCreateFailure(err: String?) {
                    throw RuntimeException("createOffer failed: $err")
                }
            }, constraints)
        }
        if (isVideo && gotLocalVideo) {
            mainHandler.postDelayed({ createOffer() }, 300)
        } else {
            createOffer()
        }
        return callId
    }

    fun handleOffer(
        callId: String,
        sdp: String,
        isVideo: Boolean,
        onIce: (String) -> Unit,
        onAnswer: (String) -> Unit,
    ) {
        ensureFactory()
        enableCallAudio()
        end()
        recvOnlyVideo = false
        recvOnlyAudio = false
        val pc = createPeer(onIce)
        peerConnection = pc

        addAudioTrack(pc)
        val gotLocalVideo = if (isVideo) addCameraTrack(pc) else false
        if (isVideo && !gotLocalVideo) {
            recvOnlyVideo = true
            pc.addTransceiver(
                org.webrtc.MediaStreamTrack.MediaType.MEDIA_TYPE_VIDEO,
                RtpTransceiver.RtpTransceiverInit(RtpTransceiver.RtpTransceiverDirection.RECV_ONLY),
            )
        }

        pc.setRemoteDescription(
            object : SdpObserverAdapter() {
                override fun onSetSuccess() {
                    flushPendingIce()
                }
            },
            SessionDescription(SessionDescription.Type.OFFER, sdp),
        )
        val constraints = MediaConstraints().apply {
            mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveAudio", "true"))
            mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveVideo", if (isVideo) "true" else "false"))
        }
        val createAnswer = {
            pc.createAnswer(object : SdpObserverAdapter() {
                override fun onCreateSuccess(desc: SessionDescription?) {
                    desc ?: return
                    pc.setLocalDescription(SdpObserverAdapter(), desc)
                    onAnswer(desc.description)
                }
                override fun onCreateFailure(err: String?) {
                    throw RuntimeException("createAnswer failed: $err")
                }
            }, constraints)
        }
        if (isVideo && gotLocalVideo) {
            mainHandler.postDelayed({ createAnswer() }, 300)
        } else {
            createAnswer()
        }
    }

    fun handleAnswer(sdp: String) {
        peerConnection?.setRemoteDescription(
            object : SdpObserverAdapter() {
                override fun onSetSuccess() {
                    flushPendingIce()
                }
            },
            SessionDescription(SessionDescription.Type.ANSWER, sdp),
        )
    }

    fun handleIce(json: String) {
        try {
            val o = JSONObject(json)
            val candidate = o.optString("candidate")
            if (candidate.isBlank()) return
            val sdpMid = o.optString("sdpMid")
            val sdpMLineIndex = o.optInt("sdpMLineIndex", 0)
            val ice = IceCandidate(sdpMid, sdpMLineIndex, candidate)
            val pc = peerConnection
            if (pc == null || pc.remoteDescription == null) {
                pendingIce.add(ice)
                return
            }
            pc.addIceCandidate(ice)
        } catch (_: Exception) {
        }
    }

    private fun flushPendingIce() {
        val pc = peerConnection ?: return
        if (pc.remoteDescription == null) return
        val batch = pendingIce.toList()
        pendingIce.clear()
        for (ice in batch) {
            try { pc.addIceCandidate(ice) } catch (_: Exception) {}
        }
    }

    fun bindLocal(renderer: SurfaceViewRenderer) {
        localRenderer?.release()
        localRenderer = renderer
        renderer.init(egl.eglBaseContext, null)
        renderer.setMirror(true)
        renderer.setEnableHardwareScaler(true)
        renderer.setScalingType(RendererCommon.ScalingType.SCALE_ASPECT_FILL)
        localVideo?.addSink(renderer)
    }

    fun bindRemote(renderer: SurfaceViewRenderer) {
        remoteRenderer?.release()
        remoteRenderer = renderer
        renderer.init(egl.eglBaseContext, null)
        renderer.setEnableHardwareScaler(true)
        renderer.setScalingType(RendererCommon.ScalingType.SCALE_ASPECT_FIT)
        remoteVideo?.addSink(renderer)
    }

    fun setMute(muted: Boolean) {
        localAudio?.setEnabled(!muted)
    }

    fun end() {
        try { videoCapturer?.stopCapture() } catch (_: Exception) {}
        videoCapturer?.dispose()
        videoCapturer = null
        localAudio?.dispose()
        localVideo?.dispose()
        remoteVideo?.dispose()
        localAudio = null
        localVideo = null
        remoteVideo = null
        recvOnlyVideo = false
        recvOnlyAudio = false
        pendingIce.clear()
        peerConnection?.close()
        peerConnection = null
        localRenderer?.release()
        remoteRenderer?.release()
        localRenderer = null
        remoteRenderer = null
        disableCallAudio()
    }

    private fun addAudioTrack(pc: PeerConnection) {
        try {
            val audioConstraints = MediaConstraints().apply {
                mandatory.add(MediaConstraints.KeyValuePair("googEchoCancellation", "true"))
                mandatory.add(MediaConstraints.KeyValuePair("googNoiseSuppression", "true"))
                mandatory.add(MediaConstraints.KeyValuePair("googAutoGainControl", "true"))
            }
            val audioSource = factory!!.createAudioSource(audioConstraints)
            localAudio = factory!!.createAudioTrack("audio0", audioSource)
            localAudio!!.setEnabled(true)
            pc.addTrack(localAudio, listOf("stream0"))
        } catch (_: Exception) {
            recvOnlyAudio = true
            pc.addTransceiver(
                org.webrtc.MediaStreamTrack.MediaType.MEDIA_TYPE_AUDIO,
                RtpTransceiver.RtpTransceiverInit(RtpTransceiver.RtpTransceiverDirection.RECV_ONLY),
            )
        }
    }

    private fun addCameraTrack(pc: PeerConnection): Boolean {
        return try {
            videoCapturer = createCameraCapturer()
            val videoSource = factory!!.createVideoSource(videoCapturer!!.isScreencast)
            videoCapturer!!.initialize(
                org.webrtc.SurfaceTextureHelper.create("capture", egl.eglBaseContext),
                context,
                videoSource.capturerObserver,
            )
            videoCapturer!!.startCapture(640, 480, 24)
            localVideo = factory!!.createVideoTrack("video0", videoSource)
            localVideo!!.setEnabled(true)
            pc.addTrack(localVideo, listOf("stream0"))
            localRenderer?.let { localVideo?.addSink(it) }
            true
        } catch (_: Exception) {
            false
        }
    }

    private fun createPeer(onIce: (String) -> Unit): PeerConnection {
        val ice = listOf(
            PeerConnection.IceServer.builder("stun:stun.l.google.com:19302").createIceServer(),
            PeerConnection.IceServer.builder("stun:stun1.l.google.com:19302").createIceServer(),
        )
        return factory!!.createPeerConnection(ice, object : PeerConnection.Observer {
            override fun onIceCandidate(candidate: IceCandidate?) {
                candidate ?: return
                val json = JSONObject()
                    .put("candidate", candidate.sdp)
                    .put("sdpMid", candidate.sdpMid)
                    .put("sdpMLineIndex", candidate.sdpMLineIndex)
                onIce(json.toString())
            }
            override fun onAddStream(stream: MediaStream?) {
                stream?.videoTracks?.firstOrNull()?.let { attachRemote(it) }
            }
            override fun onAddTrack(receiver: RtpReceiver?, streams: Array<out MediaStream>?) {
                val track = receiver?.track() ?: return
                if (track is VideoTrack) {
                    attachRemote(track)
                } else if (track.kind() == "audio") {
                    track.setEnabled(true)
                }
            }
            override fun onSignalingChange(p0: PeerConnection.SignalingState?) {}
            override fun onIceConnectionChange(state: PeerConnection.IceConnectionState?) {
                if (state == PeerConnection.IceConnectionState.FAILED
                    || state == PeerConnection.IceConnectionState.DISCONNECTED
                    || state == PeerConnection.IceConnectionState.CLOSED
                ) {
                    mainHandler.post { onConnectionLost?.invoke() }
                }
            }
            override fun onIceConnectionReceivingChange(p0: Boolean) {}
            override fun onIceGatheringChange(p0: PeerConnection.IceGatheringState?) {}
            override fun onIceCandidatesRemoved(p0: Array<out IceCandidate>?) {}
            override fun onRemoveStream(p0: MediaStream?) {}
            override fun onDataChannel(p0: DataChannel?) {}
            override fun onRenegotiationNeeded() {}
        })!!
    }

    private fun attachRemote(track: VideoTrack) {
        remoteVideo?.removeSink(remoteRenderer)
        remoteVideo?.dispose()
        remoteVideo = track
        track.setEnabled(true)
        remoteRenderer?.let { track.addSink(it) }
    }

    private fun createCameraCapturer(): VideoCapturer {
        val enumerator = Camera2Enumerator(context)
        val device = enumerator.deviceNames.firstOrNull { enumerator.isFrontFacing(it) }
            ?: enumerator.deviceNames.firstOrNull()
            ?: throw IllegalStateException("No camera found")
        return enumerator.createCapturer(device, null)
            ?: throw IllegalStateException("Failed to open camera")
    }

    private open class SdpObserverAdapter : SdpObserver {
        override fun onCreateSuccess(p0: SessionDescription?) {}
        override fun onSetSuccess() {}
        override fun onCreateFailure(p0: String?) {}
        override fun onSetFailure(p0: String?) {}
    }
}