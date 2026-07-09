// SRLTCP WebRTC — desktop call manager (Tauri webview)

const ICE_SERVERS = [
  { urls: 'stun:stun.l.google.com:19302' },
  { urls: 'stun:stun1.l.google.com:19302' },
];

let peerConnection = null;
let localStream = null;
let pendingIncoming = null;
let pendingIceCandidates = [];
let callEndedNotified = false;
let callSettings = { mic: true, camera: true };

function mediaErrorHelp(err) {
  const name = err?.name || '';
  if (name === 'NotAllowedError' || name === 'PermissionDeniedError') {
    return 'Microphone/camera blocked. On Linux: allow PipeWire/portal permissions, then retry.';
  }
  if (name === 'OverconstrainedError' || name === 'NotFoundError') {
    return 'Camera/mic not available. Try voice-only or disable camera in Settings.';
  }
  if (name === 'NotReadableError' || name === 'AbortError') {
    return 'Device busy or unavailable. Close other apps using the camera/mic.';
  }
  return err?.message || String(err);
}

async function getMedia(isVideo) {
  if (!navigator.mediaDevices?.getUserMedia) {
    throw new Error('WebRTC media not available in this webview');
  }
  const audio = callSettings.mic
    ? { echoCancellation: true, noiseSuppression: true, autoGainControl: true }
    : false;
  let video = false;
  if (isVideo && callSettings.camera) {
    video = { width: { ideal: 640, max: 1280 }, height: { ideal: 480, max: 720 } };
  }
  try {
    return await navigator.mediaDevices.getUserMedia({ audio, video });
  } catch (e) {
    if (isVideo && video !== false) {
      try {
        return await navigator.mediaDevices.getUserMedia({ audio, video: false });
      } catch (e2) {
        throw new Error(mediaErrorHelp(e2));
      }
    }
    throw new Error(mediaErrorHelp(e));
  }
}

function setCallVideoLayout(isVideo) {
  const videos = document.querySelector('.call-videos');
  const local = document.getElementById('local-video');
  const remote = document.getElementById('remote-video');
  const voiceOnly = document.getElementById('call-voice-only');
  if (videos) videos.classList.toggle('voice-only', !isVideo);
  if (local) local.classList.toggle('hidden', !isVideo);
  if (remote) remote.classList.toggle('hidden', !isVideo);
  if (voiceOnly) voiceOnly.classList.toggle('hidden', isVideo);
}

function showCallOverlay(show, isVideo = true) {
  document.getElementById('call-overlay')?.classList.toggle('hidden', !show);
  if (show) setCallVideoLayout(isVideo);
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

async function playVideoEl(el) {
  if (!el?.srcObject && !el?.src) return;
  try {
    el.muted = el.id === 'local-video';
    await el.play();
  } catch (_) {}
}

async function cleanupCall() {
  callEndedNotified = false;
  pendingIceCandidates = [];
  peerConnection?.close();
  peerConnection = null;
  localStream?.getTracks().forEach(t => t.stop());
  localStream = null;
  const rv = document.getElementById('remote-video');
  const lv = document.getElementById('local-video');
  if (rv) { rv.srcObject = null; rv.pause?.(); }
  if (lv) { lv.srcObject = null; lv.pause?.(); }
  showCallOverlay(false);
  showIncomingModal(false);
}

function bindStreams(isVideo) {
  const lv = document.getElementById('local-video');
  const rv = document.getElementById('remote-video');
  if (lv && localStream) {
    lv.srcObject = localStream;
    playVideoEl(lv);
  }
  if (!peerConnection) return;
  peerConnection.ontrack = (e) => {
    if (!rv || !e.streams?.[0]) return;
    rv.srcObject = e.streams[0];
    playVideoEl(rv);
  };
  setCallVideoLayout(isVideo && !!localStream?.getVideoTracks()?.length);
}

async function flushPendingIce() {
  if (!peerConnection?.remoteDescription) return;
  const pending = pendingIceCandidates.splice(0);
  for (const c of pending) {
    try { await peerConnection.addIceCandidate(c); } catch (_) {}
  }
}

async function addIceCandidateRaw(payload) {
  if (!payload || !peerConnection) return;
  let data;
  try {
    data = typeof payload === 'string' ? JSON.parse(payload) : payload;
  } catch (_) {
    return;
  }
  const candidate = new RTCIceCandidate(data);
  if (!peerConnection.remoteDescription) {
    pendingIceCandidates.push(candidate);
    return;
  }
  try {
    await peerConnection.addIceCandidate(candidate);
  } catch (_) {}
}

function createPeerConnection(peerId, callId, isVideo, invoke, onEnded) {
  peerConnection = new RTCPeerConnection({ iceServers: ICE_SERVERS });
  pendingIceCandidates = [];

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

  peerConnection.onconnectionstatechange = () => {
    const state = peerConnection?.connectionState;
    if ((state === 'failed' || state === 'disconnected' || state === 'closed') && !callEndedNotified) {
      callEndedNotified = true;
      invoke('end_call', { peerId, callId }).catch(() => {});
      onEnded?.();
    }
  };

  return peerConnection;
}

async function startOutgoingCall(peerId, isVideo, invoke, peerLabel, onEnded) {
  await cleanupCall();
  const callId = crypto.randomUUID();
  let gotVideo = isVideo;
  try {
    localStream = await getMedia(isVideo);
    gotVideo = !!localStream.getVideoTracks().length;
  } catch (e) {
    throw new Error(mediaErrorHelp(e));
  }
  showCallOverlay(true, gotVideo);
  document.getElementById('call-peer-label').textContent = peerLabel || peerId;
  document.getElementById('call-type-label').textContent = gotVideo ? 'Video call' : 'Voice call';

  createPeerConnection(peerId, callId, isVideo, invoke, onEnded);
  localStream.getTracks().forEach(t => peerConnection.addTrack(t, localStream));
  bindStreams(gotVideo);

  const offer = await peerConnection.createOffer();
  await peerConnection.setLocalDescription(offer);
  await invoke('send_call_signal', {
    peerId, callId, signal: 'offer', payload: offer.sdp, isVideo,
  });
  return { callId, peer: peerId, video: gotVideo };
}

async function answerIncomingCall(invoke, onEnded) {
  if (!pendingIncoming) throw new Error('No incoming call');
  const { peerId, callId, payload, isVideo, peerLabel } = pendingIncoming;
  showIncomingModal(false);
  let gotVideo = isVideo;
  try {
    localStream = await getMedia(isVideo);
    gotVideo = !!localStream.getVideoTracks().length;
  } catch (e) {
    await invoke('send_call_signal', {
      peerId, callId, signal: 'end', payload: '', isVideo,
    });
    throw new Error(mediaErrorHelp(e));
  }
  showCallOverlay(true, gotVideo);
  document.getElementById('call-peer-label').textContent = peerLabel || peerId;
  document.getElementById('call-type-label').textContent = gotVideo ? 'Video call' : 'Voice call';

  createPeerConnection(peerId, callId, isVideo, invoke, onEnded);
  localStream.getTracks().forEach(t => peerConnection.addTrack(t, localStream));
  bindStreams(gotVideo);

  await peerConnection.setRemoteDescription({ type: 'offer', sdp: payload });
  await flushPendingIce();
  const answer = await peerConnection.createAnswer();
  await peerConnection.setLocalDescription(answer);
  await invoke('send_call_signal', {
    peerId, callId, signal: 'answer', payload: answer.sdp, isVideo,
  });
  return { callId, peer: peerId, video: gotVideo };
}

async function declineIncomingCall(invoke) {
  if (!pendingIncoming) return;
  const { peerId, callId, isVideo } = pendingIncoming;
  showIncomingModal(false);
  pendingIncoming = null;
  await invoke('send_call_signal', { peerId, callId, signal: 'end', payload: '', isVideo });
}

async function handleIncomingCallSignal(p, invoke, activeCallRef, peerLabelFn, onEnded) {
  const signal = p.type?.replace('call_', '') || '';
  const callId = p.call_id ?? p.callId;
  const peerId = p.peer_id ?? p.peerId;
  const payload = p.payload || p.message || '';
  const isVideo = p.is_video ?? p.isVideo ?? p.auto_trusted ?? false;

  if (signal === 'end') {
    await cleanupCall();
    onEnded?.();
    return null;
  }

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
    await flushPendingIce();
    return activeCallRef.current;
  }

  if (signal === 'ice' && payload) {
    await addIceCandidateRaw(payload);
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