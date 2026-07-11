// SRLTCP WebRTC — desktop call manager (Tauri webview)

const ICE_SERVERS = [
  { urls: 'stun:stun.l.google.com:19302' },
  { urls: 'stun:stun1.l.google.com:19302' },
];

let peerConnection = null;
let localStream = null;
let remoteAudioStream = null;
let pendingIncoming = null;
let pendingIceCandidates = [];
let callEndedNotified = false;
let intentionalHangup = false;
let callSettings = { mic: true, camera: false, audioInputId: '', audioOutputId: '' };
let localCameraAvailable = false;
let recvOnlyVideo = false;
let recvOnlyAudio = false;

function mediaErrorHelp(err) {
  const name = err?.name || '';
  if (name === 'NotAllowedError' || name === 'PermissionDeniedError') {
    return 'Microphone/camera blocked by WebKit. Restart the app after updating (permission handler). Linux also needs: pipewire + wireplumber + xdg-desktop-portal running. Then Settings → Test mic & camera again.';
  }
  if (name === 'OverconstrainedError' || name === 'NotFoundError') {
    return 'Camera/mic not available — listen/watch-only mode will be used if possible.';
  }
  if (name === 'NotReadableError' || name === 'AbortError') {
    return 'Device busy or unavailable. Close other apps using the camera/mic.';
  }
  if (name === 'SecurityError') {
    return 'Media blocked by security policy. Restart SRLTCP from ./run.sh (sets portal/PipeWire env).';
  }
  return err?.message || String(err);
}

/**
 * Minimal constraints for WebKitGTK/GStreamer on Linux.
 * Never call enumerateDevices() — triggers GstIntRange errors.
 * Supports recv-only when no local mic/camera (e.g. headless Arch desktop).
 */
async function getMedia(isVideo) {
  if (!navigator.mediaDevices?.getUserMedia) {
    if (isVideo) {
      recvOnlyVideo = true;
      recvOnlyAudio = true;
      return new MediaStream();
    }
    throw new Error('WebRTC media not available in this webview');
  }

  recvOnlyVideo = false;
  recvOnlyAudio = false;

  const wantMic = callSettings.mic;
  const wantLocalVideo = isVideo && callSettings.camera && localCameraAvailable;

  const combined = new MediaStream();

  if (wantMic) {
    try {
      const audioConstraints = {
        echoCancellation: true,
        noiseSuppression: true,
        autoGainControl: true,
      };
      if (callSettings.audioInputId) {
        audioConstraints.deviceId = { exact: callSettings.audioInputId };
      }
      const a = await navigator.mediaDevices.getUserMedia({
        audio: audioConstraints,
        video: false,
      });
      a.getAudioTracks().forEach((t) => combined.addTrack(t));
    } catch (_) {
      // Fallback without deviceId if exact device failed
      try {
        const a = await navigator.mediaDevices.getUserMedia({
          audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
          video: false,
        });
        a.getAudioTracks().forEach((t) => combined.addTrack(t));
      } catch (__) {
        recvOnlyAudio = true;
      }
    }
  } else if (isVideo) {
    recvOnlyAudio = true;
  }

  if (wantLocalVideo) {
    try {
      const v = await navigator.mediaDevices.getUserMedia({ audio: false, video: true });
      v.getVideoTracks().forEach((t) => combined.addTrack(t));
    } catch (_) {
      recvOnlyVideo = true;
    }
  } else if (isVideo) {
    recvOnlyVideo = true;
  }

  if (isVideo && !combined.getVideoTracks().length) recvOnlyVideo = true;
  if (wantMic && !combined.getAudioTracks().length) recvOnlyAudio = true;

  return combined;
}

function addLocalTracks(pc, stream) {
  stream.getTracks().forEach((t) => pc.addTrack(t, stream));
  if (recvOnlyAudio) {
    try { pc.addTransceiver('audio', { direction: 'recvonly' }); } catch (_) {}
  }
  if (recvOnlyVideo) {
    try { pc.addTransceiver('video', { direction: 'recvonly' }); } catch (_) {}
  }
}

function setCallVideoLayout(isVideo) {
  const videos = document.querySelector('.call-videos');
  const local = document.getElementById('local-video');
  const remote = document.getElementById('remote-video');
  const voiceOnly = document.getElementById('call-voice-only');
  const showLocalVideo = isVideo && !!localStream?.getVideoTracks()?.length;
  if (videos) videos.classList.toggle('voice-only', !isVideo || !showLocalVideo);
  if (local) local.classList.toggle('hidden', !showLocalVideo);
  if (remote) remote.classList.toggle('hidden', !isVideo);
  if (voiceOnly) {
    const listenOnly = recvOnlyAudio && recvOnlyVideo;
    const recvOnly = recvOnlyVideo && !showLocalVideo;
    voiceOnly.textContent = listenOnly
      ? 'Listen & watch only (no local mic/camera)'
      : recvOnly
        ? 'Receiving video (no local camera)'
        : recvOnlyAudio
          ? 'Listen only (no local mic)'
          : 'Voice call';
    voiceOnly.classList.toggle('hidden', isVideo && showLocalVideo && !recvOnlyAudio);
  }
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

async function playMediaEl(el) {
  if (!el?.srcObject && !el?.src) return;
  try {
    if (el.tagName === 'VIDEO') el.muted = el.id === 'local-video';
    else el.muted = false;
    await el.play();
  } catch (_) {}
}

function resetRemoteAudio() {
  const audioEl = document.getElementById('remote-audio');
  if (audioEl) {
    audioEl.pause?.();
    audioEl.srcObject = null;
  }
  remoteAudioStream = null;
}

async function cleanupCall() {
  intentionalHangup = true;
  callEndedNotified = true;
  recvOnlyVideo = false;
  recvOnlyAudio = false;
  pendingIceCandidates = [];
  peerConnection?.close();
  peerConnection = null;
  localStream?.getTracks().forEach((t) => t.stop());
  localStream = null;
  resetRemoteAudio();
  const rv = document.getElementById('remote-video');
  const lv = document.getElementById('local-video');
  if (rv) { rv.srcObject = null; rv.pause?.(); }
  if (lv) { lv.srcObject = null; lv.pause?.(); }
  showCallOverlay(false);
  showIncomingModal(false);
}

async function applyOutputDevice(el) {
  if (!el || !callSettings.audioOutputId) return;
  if (typeof el.setSinkId === 'function') {
    try {
      await el.setSinkId(callSettings.audioOutputId);
    } catch (_) {}
  }
}

function bindStreams(isVideo) {
  const lv = document.getElementById('local-video');
  const rv = document.getElementById('remote-video');
  const audioEl = document.getElementById('remote-audio');
  if (lv && localStream?.getVideoTracks().length) {
    lv.srcObject = localStream;
    playMediaEl(lv);
  }
  if (audioEl) applyOutputDevice(audioEl);
  if (!peerConnection) return;
  peerConnection.ontrack = (e) => {
    const track = e.track;
    if (!track) return;
    if (track.kind === 'audio' && audioEl) {
      if (!remoteAudioStream) {
        remoteAudioStream = new MediaStream();
        audioEl.srcObject = remoteAudioStream;
      }
      if (!remoteAudioStream.getTracks().some((t) => t.id === track.id)) {
        remoteAudioStream.addTrack(track);
      }
      track.enabled = true;
      audioEl.muted = false;
      applyOutputDevice(audioEl);
      playMediaEl(audioEl);
    } else if (track.kind === 'video' && rv && e.streams?.[0]) {
      rv.srcObject = e.streams[0];
      rv.muted = false;
      playMediaEl(rv);
    }
  };
  setCallVideoLayout(isVideo);
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
  callEndedNotified = false;

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
    if (intentionalHangup || callEndedNotified) return;
    if (state === 'failed' || state === 'disconnected' || state === 'closed') {
      callEndedNotified = true;
      invoke('end_call', { peerId, callId }).catch(() => {});
      onEnded?.();
    }
  };

  return peerConnection;
}

function callModeLabel(isVideo, gotVideo) {
  if (!gotVideo) return recvOnlyAudio ? 'Listen only' : 'Voice call';
  if (recvOnlyVideo && recvOnlyAudio) return 'Listen & watch';
  if (recvOnlyVideo) return 'Video (receive only)';
  if (recvOnlyAudio) return 'Voice (listen only)';
  return 'Video call';
}

let hasActiveCall = () => false;

async function startOutgoingCall(peerId, isVideo, invoke, peerLabel, onEnded) {
  if (hasActiveCall()) {
    throw new Error('Already in a call');
  }
  await cleanupCall();
  intentionalHangup = false;
  const callId = crypto.randomUUID();
  localStream = await getMedia(isVideo);
  const gotVideo = isVideo && (!!localStream.getVideoTracks().length || recvOnlyVideo);
  showCallOverlay(true, gotVideo);
  document.getElementById('call-peer-label').textContent = peerLabel || peerId;
  document.getElementById('call-type-label').textContent = callModeLabel(isVideo, gotVideo);

  createPeerConnection(peerId, callId, isVideo, invoke, onEnded);
  addLocalTracks(peerConnection, localStream);
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
  intentionalHangup = false;
  localStream = await getMedia(isVideo);
  const gotVideo = isVideo && (!!localStream.getVideoTracks().length || recvOnlyVideo);
  showCallOverlay(true, gotVideo);
  document.getElementById('call-peer-label').textContent = peerLabel || peerId;
  document.getElementById('call-type-label').textContent = callModeLabel(isVideo, gotVideo);

  createPeerConnection(peerId, callId, isVideo, invoke, onEnded);
  addLocalTracks(peerConnection, localStream);
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

  if (signal === 'end' || signal === 'ended') {
    intentionalHangup = true;
    await cleanupCall();
    activeCallRef.current = null;
    onEnded?.(false);
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

function setCallSettings({ mic, camera, audioInputId, audioOutputId }) {
  if (mic !== undefined) callSettings.mic = mic;
  if (camera !== undefined) callSettings.camera = camera && localCameraAvailable;
  if (audioInputId !== undefined) callSettings.audioInputId = audioInputId || '';
  if (audioOutputId !== undefined) {
    callSettings.audioOutputId = audioOutputId || '';
    const audioEl = document.getElementById('remote-audio');
    applyOutputDevice(audioEl);
  }
}

/** List mics/speakers after a brief getUserMedia so labels are available. */
async function listAudioDevices() {
  try {
    if (!navigator.mediaDevices?.enumerateDevices) return { inputs: [], outputs: [] };
    // Probe once so browser fills in device labels
    try {
      const s = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
      s.getTracks().forEach((t) => t.stop());
    } catch (_) {}
    const all = await navigator.mediaDevices.enumerateDevices();
    return {
      inputs: all.filter((d) => d.kind === 'audioinput').map((d) => ({
        id: d.deviceId,
        label: d.label || `Microphone ${d.deviceId.slice(0, 6)}`,
      })),
      outputs: all.filter((d) => d.kind === 'audiooutput').map((d) => ({
        id: d.deviceId,
        label: d.label || `Speaker ${d.deviceId.slice(0, 6)}`,
      })),
    };
  } catch (_) {
    return { inputs: [], outputs: [] };
  }
}

function setLocalCameraAvailable(available) {
  localCameraAvailable = !!available;
  if (!localCameraAvailable) callSettings.camera = false;
}

async function testMediaPermissions() {
  const parts = [];
  if (callSettings.mic) {
    try {
      const s = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
      s.getTracks().forEach((t) => t.stop());
      parts.push('microphone OK');
    } catch (e) {
      parts.push(`microphone: ${mediaErrorHelp(e)} (listen-only calls still work)`);
    }
  }
  if (callSettings.camera && localCameraAvailable) {
    try {
      const s = await navigator.mediaDevices.getUserMedia({ audio: false, video: true });
      const hasVideo = s.getVideoTracks().length > 0;
      s.getTracks().forEach((t) => t.stop());
      parts.push(hasVideo ? 'camera OK' : 'camera unavailable');
    } catch (e) {
      parts.push(`camera: ${mediaErrorHelp(e)} (receive-only video still works)`);
    }
  } else {
    parts.push('camera disabled — receive-only video for calls');
  }
  return parts.join(' · ');
}

window.SrltcpWebRTC = {
  startOutgoingCall,
  answerIncomingCall,
  declineIncomingCall,
  handleIncomingCallSignal,
  cleanupCall,
  toggleMute,
  toggleCamera,
  listAudioDevices,
  setCallSettings,
  setLocalCameraAvailable,
  testMediaPermissions,
  setHasActiveCall(fn) { hasActiveCall = fn; },
  get pendingIncoming() { return pendingIncoming; },
};