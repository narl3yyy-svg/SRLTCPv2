package com.srltcp.v2.webrtc

import android.content.Context
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
import org.webrtc.SdpObserver
import org.webrtc.SessionDescription
import org.webrtc.SurfaceViewRenderer
import org.webrtc.VideoCapturer
import org.webrtc.VideoSource
import org.webrtc.VideoTrack
import java.util.UUID

class WebRtcCallManager(private val context: Context) {
    private val egl = EglBase.create()
    private var factory: PeerConnectionFactory? = null
    private var peerConnection: PeerConnection? = null
    private var localAudio: AudioTrack? = null
    private var localVideo: VideoTrack? = null
    private var remoteVideo: VideoTrack? = null
    private var videoCapturer: VideoCapturer? = null
    private var localRenderer: SurfaceViewRenderer? = null
    private var remoteRenderer: SurfaceViewRenderer? = null
    private val pendingIce = mutableListOf<IceCandidate>()

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
        end()
        val callId = UUID.randomUUID().toString()
        val pc = createPeer(onIce)
        peerConnection = pc

        val audioSource = factory!!.createAudioSource(MediaConstraints())
        localAudio = factory!!.createAudioTrack("audio0", audioSource)
        pc.addTrack(localAudio)

        val gotVideo = if (isVideo) addCameraTrack(pc) else false

        val constraints = MediaConstraints().apply {
            mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveAudio", "true"))
            mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveVideo", if (gotVideo) "true" else "false"))
        }
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
        end()
        val pc = createPeer(onIce)
        peerConnection = pc

        val audioSource = factory!!.createAudioSource(MediaConstraints())
        localAudio = factory!!.createAudioTrack("audio0", audioSource)
        pc.addTrack(localAudio)

        val gotVideo = if (isVideo) addCameraTrack(pc) else false

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
            mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveVideo", if (gotVideo) "true" else "false"))
        }
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
        localVideo?.addSink(renderer)
    }

    fun bindRemote(renderer: SurfaceViewRenderer) {
        remoteRenderer?.release()
        remoteRenderer = renderer
        renderer.init(egl.eglBaseContext, null)
        renderer.setEnableHardwareScaler(true)
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
        pendingIce.clear()
        peerConnection?.close()
        peerConnection = null
        localRenderer?.release()
        remoteRenderer?.release()
        localRenderer = null
        remoteRenderer = null
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
            pc.addTrack(localVideo)
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
                val track = receiver?.track()
                if (track is VideoTrack) attachRemote(track)
            }
            override fun onSignalingChange(p0: PeerConnection.SignalingState?) {}
            override fun onIceConnectionChange(p0: PeerConnection.IceConnectionState?) {}
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