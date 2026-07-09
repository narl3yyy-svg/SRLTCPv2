package com.srltcp.v2.webrtc

import android.content.Context
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
    private var videoCapturer: VideoCapturer? = null

    var onRemoteStream: ((MediaStream) -> Unit)? = null

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
        val callId = UUID.randomUUID().toString()
        val pc = createPeer(onIce)
        peerConnection = pc

        val audioSource = factory!!.createAudioSource(MediaConstraints())
        localAudio = factory!!.createAudioTrack("audio0", audioSource)
        pc.addTrack(localAudio)

        if (isVideo) {
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
        }

        val constraints = MediaConstraints().apply {
            mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveAudio", "true"))
            mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveVideo", if (isVideo) "true" else "false"))
        }
        pc.createOffer(object : SdpObserverAdapter() {
            override fun onCreateSuccess(desc: SessionDescription?) {
                desc ?: return
                pc.setLocalDescription(SdpObserverAdapter(), desc)
                onOffer(callId, desc.description)
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
        val pc = createPeer(onIce)
        peerConnection = pc

        val audioSource = factory!!.createAudioSource(MediaConstraints())
        localAudio = factory!!.createAudioTrack("audio0", audioSource)
        pc.addTrack(localAudio)

        if (isVideo) {
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
        }

        pc.setRemoteDescription(SdpObserverAdapter(), SessionDescription(SessionDescription.Type.OFFER, sdp))
        pc.createAnswer(object : SdpObserverAdapter() {
            override fun onCreateSuccess(desc: SessionDescription?) {
                desc ?: return
                pc.setLocalDescription(SdpObserverAdapter(), desc)
                onAnswer(desc.description)
            }
        }, MediaConstraints())
    }

    fun handleAnswer(sdp: String) {
        peerConnection?.setRemoteDescription(
            SdpObserverAdapter(),
            SessionDescription(SessionDescription.Type.ANSWER, sdp),
        )
    }

    fun handleIce(json: String) {
        try {
            val mid = Regex(""""sdpMid"\s*:\s*"([^"]*)"""").find(json)?.groupValues?.get(1)
            val idx = Regex(""""sdpMLineIndex"\s*:\s*(\d+)""").find(json)?.groupValues?.get(1)?.toIntOrNull() ?: 0
            val cand = Regex(""""candidate"\s*:\s*"([^"]*)"""").find(json)?.groupValues?.get(1) ?: return
            peerConnection?.addIceCandidate(IceCandidate(mid, idx, cand))
        } catch (_: Exception) {
        }
    }

    fun bindLocal(renderer: SurfaceViewRenderer) {
        renderer.init(egl.eglBaseContext, null)
        localVideo?.addSink(renderer)
    }

    fun bindRemote(renderer: SurfaceViewRenderer) {
        renderer.init(egl.eglBaseContext, null)
    }

    fun end() {
        videoCapturer?.stopCapture()
        videoCapturer?.dispose()
        localAudio?.dispose()
        localVideo?.dispose()
        peerConnection?.close()
        peerConnection = null
    }

    private fun createPeer(onIce: (String) -> Unit): PeerConnection {
        val ice = listOf(PeerConnection.IceServer.builder("stun:stun.l.google.com:19302").createIceServer())
        return factory!!.createPeerConnection(ice, object : PeerConnection.Observer {
            override fun onIceCandidate(candidate: IceCandidate?) {
                candidate ?: return
                onIce("""{"candidate":"${candidate.sdp}","sdpMid":"${candidate.sdpMid}","sdpMLineIndex":${candidate.sdpMLineIndex}}""")
            }
            override fun onAddStream(stream: MediaStream?) {
                stream ?: return
                onRemoteStream?.invoke(stream)
            }
            override fun onSignalingChange(p0: PeerConnection.SignalingState?) {}
            override fun onIceConnectionChange(p0: PeerConnection.IceConnectionState?) {}
            override fun onIceConnectionReceivingChange(p0: Boolean) {}
            override fun onIceGatheringChange(p0: PeerConnection.IceGatheringState?) {}
            override fun onIceCandidatesRemoved(p0: Array<out IceCandidate>?) {}
            override fun onRemoveStream(p0: MediaStream?) {}
            override fun onDataChannel(p0: DataChannel?) {}
            override fun onRenegotiationNeeded() {}
            override fun onAddTrack(receiver: RtpReceiver?, streams: Array<out MediaStream>?) {
                streams?.firstOrNull()?.let { onRemoteStream?.invoke(it) }
            }
        })!!
    }

    private fun createCameraCapturer(): VideoCapturer {
        val enumerator = Camera2Enumerator(context)
        val device = enumerator.deviceNames.firstOrNull { enumerator.isFrontFacing(it) }
            ?: enumerator.deviceNames.first()
        return enumerator.createCapturer(device, null)
    }

    private open class SdpObserverAdapter : SdpObserver {
        override fun onCreateSuccess(p0: SessionDescription?) {}
        override fun onSetSuccess() {}
        override fun onCreateFailure(p0: String?) {}
        override fun onSetFailure(p0: String?) {}
    }
}