// SRLTCP WebRTC — desktop (Tauri webview)

const ICE_SERVERS = [{ urls: 'stun:stun.l.google.com:19302' }];

let peerConnection = null;
let localStream = null;

function ensureCallElements() {
  let panel = document.getElementById('call-media');
  if (!panel) {
    panel = document.createElement('div');
    panel.id = 'call-media';
    panel.className = 'call-media hidden';
    panel.innerHTML = `
      <video id="local-video" autoplay muted playsinline class="call-video local"></video>
      <video id="remote-video" autoplay playsinline class="call-video remote"></video>`;
    document.querySelector('.chat-area')?.prepend(panel);
  }
  return panel;
}

async function cleanupCall() {
  peerConnection?.close();
  peerConnection = null;
  localStream?.getTracks().forEach(t => t.stop());
  localStream = null;
  document.getElementById('call-media')?.classList.add('hidden');
  const rv = document.getElementById('remote-video');
  const lv = document.getElementById('local-video');
  if (rv) rv.srcObject = null;
  if (lv) lv.srcObject = null;
}

async function startOutgoingCall(peerId, isVideo, invoke) {
  const callId = crypto.randomUUID();
  ensureCallElements().classList.remove('hidden');
  localStream = await navigator.mediaDevices.getUserMedia({
    audio: true,
    video: isVideo,
  });
  const lv = document.getElementById('local-video');
  if (lv) lv.srcObject = localStream;

  peerConnection = new RTCPeerConnection({ iceServers: ICE_SERVERS });
  localStream.getTracks().forEach(t => peerConnection.addTrack(t, localStream));
  peerConnection.ontrack = (e) => {
    const rv = document.getElementById('remote-video');
    if (rv) rv.srcObject = e.streams[0];
  };
  peerConnection.onicecandidate = (e) => {
    if (e.candidate) {
      invoke('send_call_signal', {
        peerId,
        callId,
        signal: 'ice',
        payload: JSON.stringify(e.candidate.toJSON()),
        isVideo,
      }).catch(() => {});
    }
  };

  const offer = await peerConnection.createOffer();
  await peerConnection.setLocalDescription(offer);
  await invoke('send_call_signal', {
    peerId,
    callId,
    signal: 'offer',
    payload: offer.sdp,
    isVideo,
  });
  return { callId, peer: peerId, video: isVideo };
}

async function handleIncomingCallSignal(p, invoke, activeCallRef) {
  const signal = p.type?.replace('call_', '') || '';
  const callId = p.call_id ?? p.callId;
  const peerId = p.peer_id ?? p.peerId;
  const payload = p.payload || '';
  const isVideo = p.is_video ?? p.isVideo ?? false;

  if (signal === 'offer') {
    ensureCallElements().classList.remove('hidden');
    localStream = await navigator.mediaDevices.getUserMedia({
      audio: true,
      video: isVideo,
    });
    const lv = document.getElementById('local-video');
    if (lv) lv.srcObject = localStream;

    peerConnection = new RTCPeerConnection({ iceServers: ICE_SERVERS });
    localStream.getTracks().forEach(t => peerConnection.addTrack(t, localStream));
    peerConnection.ontrack = (e) => {
      const rv = document.getElementById('remote-video');
      if (rv) rv.srcObject = e.streams[0];
    };
    peerConnection.onicecandidate = (e) => {
      if (e.candidate) {
        invoke('send_call_signal', {
          peerId,
          callId,
          signal: 'ice',
          payload: JSON.stringify(e.candidate.toJSON()),
          isVideo,
        }).catch(() => {});
      }
    };

    await peerConnection.setRemoteDescription({ type: 'offer', sdp: payload });
    const answer = await peerConnection.createAnswer();
    await peerConnection.setLocalDescription(answer);
    await invoke('send_call_signal', {
      peerId,
      callId,
      signal: 'answer',
      payload: answer.sdp,
      isVideo,
    });
    activeCallRef.current = { id: callId, peer: peerId, video: isVideo };
    return activeCallRef.current;
  }

  if (signal === 'answer' && peerConnection) {
    await peerConnection.setRemoteDescription({ type: 'answer', sdp: payload });
    return null;
  }

  if (signal === 'ice' && peerConnection && payload) {
    try {
      await peerConnection.addIceCandidate(JSON.parse(payload));
    } catch (_) {}
    return null;
  }

  return null;
}

window.SrltcpWebRTC = { startOutgoingCall, handleIncomingCallSignal, cleanupCall };