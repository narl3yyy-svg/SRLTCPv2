// SRLTCP WebRTC — desktop call manager (Tauri webview)

const ICE_SERVERS = [{ urls: 'stun:stun.l.google.com:19302' }];

let peerConnection = null;
let localStream = null;
let pendingIncoming = null;
let callSettings = { mic: true, camera: true };

function mediaErrorHelp(err) {
  const name = err?.name || '';
  if (name === 'NotAllowedError' || name === 'PermissionDeniedError') {
    return 'Microphone/camera blocked. On Linux: allow PipeWire/portal permissions, then retry. Settings → ensure mic is not muted.';
  }
  if (name === 'OverconstrainedError' || name === 'NotFoundError') {
    return 'Camera/mic not available. Try voice-only or check device settings.';
  }
  return err?.message || String(err);
}

async function getMedia(isVideo) {
  const audio = callSettings.mic ? { echoCancellation: true, noiseSuppression: true } : false;
  const video = isVideo && callSettings.camera
    ? { width: { ideal: 640 }, height: { ideal: 480 }, facingMode: 'user' }
    : false;
  try {
    return await navigator.mediaDevices.getUserMedia({ audio, video });
  } catch (e) {
    if (isVideo && (e.name === 'OverconstrainedError' || e.name === 'NotFoundError')) {
      return await navigator.mediaDevices.getUserMedia({ audio, video: false });
    }
    throw e;
  }
}

function showCallOverlay(show) {
  document.getElementById('call-overlay')?.classList.toggle('hidden', !show);
}

function showIncomingModal(show, info = null) {
  const modal = document.getElementById('incoming-call-modal');
  if (!modal) return;
  modal.classList.toggle('hidden', !show);
  if (info) {
    document.getElementById('incoming-call-peer').textContent = info.peerLabel || 'Peer';
    document.getElementById('incoming-call-type').textContent =
      info.isVideo ? 'Incoming video call' : 'Incoming voice call';
    pendingIncoming = info;
  } else {
    pendingIncoming = null;
  }
}

async function cleanupCall() {
  peerConnection?.close();
  peerConnection = null;
  localStream?.getTracks().forEach(t => t.stop());
  localStream = null;
  const rv = document.getElementById('remote-video');
  const lv = document.getElementById('local-video');
  if (rv) rv.srcObject = null;
  if (lv) lv.srcObject = null;
  showCallOverlay(false);
  showIncomingModal(false);
}

function bindStreams() {
  const lv = document.getElementById('local-video');
  const rv = document.getElementById('remote-video');
  if (lv && localStream) lv.srcObject = localStream;
  peerConnection.ontrack = (e) => {
    if (rv) rv.srcObject = e.streams[0];
  };
}

function createPeerConnection(peerId, callId, isVideo, invoke) {
  peerConnection = new RTCPeerConnection({ iceServers: ICE_SERVERS });
  peerConnection.onicecandidate = (e) => {
    if (e.candidate) {
      invoke('send_call_signal', {
        peerId, callId,
        signal: 'ice',
        payload: JSON.stringify(e.candidate.toJSON()),
        isVideo,
      }).catch(() => {});
    }
  };
  return peerConnection;
}

async function startOutgoingCall(peerId, isVideo, invoke, peerLabel) {
  await cleanupCall();
  const callId = crypto.randomUUID();
  try {
    localStream = await getMedia(isVideo);
  } catch (e) {
    throw new Error(mediaErrorHelp(e));
  }
  showCallOverlay(true);
  document.getElementById('call-peer-label').textContent = peerLabel || peerId;
  document.getElementById('call-type-label').textContent = isVideo ? 'Video call' : 'Voice call';

  createPeerConnection(peerId, callId, isVideo, invoke);
  localStream.getTracks().forEach(t => peerConnection.addTrack(t, localStream));
  bindStreams();

  const offer = await peerConnection.createOffer();
  await peerConnection.setLocalDescription(offer);
  await invoke('send_call_signal', {
    peerId, callId, signal: 'offer', payload: offer.sdp, isVideo,
  });
  return { callId, peer: peerId, video: isVideo };
}

async function answerIncomingCall(invoke) {
  if (!pendingIncoming) throw new Error('No incoming call');
  const { peerId, callId, payload, isVideo, peerLabel } = pendingIncoming;
  showIncomingModal(false);
  try {
    localStream = await getMedia(isVideo);
  } catch (e) {
    await invoke('send_call_signal', {
      peerId, callId, signal: 'end', payload: '', isVideo,
    });
    throw new Error(mediaErrorHelp(e));
  }
  showCallOverlay(true);
  document.getElementById('call-peer-label').textContent = peerLabel || peerId;
  document.getElementById('call-type-label').textContent = isVideo ? 'Video call' : 'Voice call';

  createPeerConnection(peerId, callId, isVideo, invoke);
  localStream.getTracks().forEach(t => peerConnection.addTrack(t, localStream));
  bindStreams();

  await peerConnection.setRemoteDescription({ type: 'offer', sdp: payload });
  const answer = await peerConnection.createAnswer();
  await peerConnection.setLocalDescription(answer);
  await invoke('send_call_signal', {
    peerId, callId, signal: 'answer', payload: answer.sdp, isVideo,
  });
  return { callId, peer: peerId, video: isVideo };
}

async function declineIncomingCall(invoke) {
  if (!pendingIncoming) return;
  const { peerId, callId, isVideo } = pendingIncoming;
  showIncomingModal(false);
  pendingIncoming = null;
  await invoke('send_call_signal', { peerId, callId, signal: 'end', payload: '', isVideo });
}

async function handleIncomingCallSignal(p, invoke, activeCallRef, peerLabelFn) {
  const signal = p.type?.replace('call_', '') || '';
  const callId = p.call_id ?? p.callId;
  const peerId = p.peer_id ?? p.peerId;
  const payload = p.payload || '';
  const isVideo = p.is_video ?? p.isVideo ?? false;

  if (signal === 'offer' && callId && peerId) {
    if (activeCallRef.current || pendingIncoming) {
      await invoke('send_call_signal', { peerId, callId, signal: 'end', payload: '', isVideo });
      return null;
    }
    const peerLabel = peerLabelFn?.(peerId) || peerId;
    showIncomingModal(true, { peerId, callId, payload, isVideo, peerLabel });
    return null;
  }

  if (signal === 'answer' && peerConnection) {
    await peerConnection.setRemoteDescription({ type: 'answer', sdp: payload });
    return null;
  }

  if (signal === 'ice' && peerConnection && payload) {
    try { await peerConnection.addIceCandidate(JSON.parse(payload)); } catch (_) {}
    return null;
  }

  return null;
}

function toggleMute() {
  if (!localStream) return false;
  const track = localStream.getAudioTracks()[0];
  if (!track) return false;
  track.enabled = !track.enabled;
  callSettings.mic = track.enabled;
  return track.enabled;
}

function toggleCamera() {
  if (!localStream) return false;
  const track = localStream.getVideoTracks()[0];
  if (!track) return false;
  track.enabled = !track.enabled;
  callSettings.camera = track.enabled;
  return track.enabled;
}

function setCallSettings({ mic, camera }) {
  if (mic !== undefined) callSettings.mic = mic;
  if (camera !== undefined) callSettings.camera = camera;
}

window.SrltcpWebRTC = {
  startOutgoingCall,
  answerIncomingCall,
  declineIncomingCall,
  handleIncomingCallSignal,
  cleanupCall,
  toggleMute,
  toggleCamera,
  setCallSettings,
  get pendingIncoming() { return pendingIncoming; },
};