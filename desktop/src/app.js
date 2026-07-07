// SRLTCP v0.2.1 Desktop Frontend

const invoke = window.__TAURI__?.core?.invoke
  ?? (async (cmd, args) => {
      console.log(`[mock] ${cmd}`, args);
      return null;
    });

const listen = window.__TAURI__?.event?.listen
  ?? (async () => () => {});

const openFileDialog = window.__TAURI__?.dialog?.open
  ?? (async () => null);

const convertFileSrc = window.__TAURI__?.core?.convertFileSrc
  ?? ((path) => path);

let activePeer = null;
let peers = [];
let activeCall = null;

const IMAGE_EXT = new Set(['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp']);
const VIDEO_EXT = new Set(['mp4', 'webm', 'mkv', 'mov', '3gp']);

function mediaKind(filename) {
  const ext = (filename || '').split('.').pop()?.toLowerCase() || '';
  if (IMAGE_EXT.has(ext)) return 'image';
  if (VIDEO_EXT.has(ext)) return 'video';
  return 'file';
}

async function init() {
  try {
    const qr = await invoke('get_qr_payload');
    document.getElementById('qr-payload').textContent = qr;

    const ports = await invoke('list_serial_ports');
    const select = document.getElementById('serial-port');
    select.innerHTML = '';
    if (ports.length === 0) {
      select.innerHTML = '<option value="">No ports found</option>';
    } else {
      ports.forEach(p => {
        const opt = document.createElement('option');
        opt.value = p;
        opt.textContent = p;
        select.appendChild(opt);
      });
    }

    const existingPeers = await invoke('get_peers');
    existingPeers.forEach(addPeer);

    setStatus('Online', true);
  } catch (e) {
    console.error('Init failed:', e);
    setStatus('Offline', false);
  }

  await listen('srltcp-event', (event) => {
    handleEvent(event.payload);
  });
}

function handleEvent(payload) {
  switch (payload.type) {
    case 'message':
      appendMessage(payload.content, 'received', payload.sender);
      break;
    case 'peer_connected':
      addPeer(payload.peer_id);
      showToast(`Peer connected: ${payload.peer_id}`);
      break;
    case 'peer_disconnected':
      removePeer(payload.peer_id);
      showToast(`Peer disconnected: ${payload.peer_id}`);
      break;
    case 'sas_ready':
      showSas(payload.sas);
      break;
    case 'transfer_progress':
      showTransferProgress(payload.filename, payload.progress);
      break;
    case 'transfer_complete':
      hideTransferProgress();
      appendMessage(`📁 ${payload.filename}`, 'received', 'Transfer');
      break;
    case 'call_started':
      activeCall = { id: payload.call_id, peer: payload.peer_id, video: payload.is_video };
      updateCallUI();
      break;
    case 'call_ended':
      activeCall = null;
      updateCallUI();
      break;
    case 'started':
      setStatus('Online', true);
      break;
    case 'stopped':
      setStatus('Offline', false);
      break;
    case 'error':
      console.error('Engine error:', payload.message);
      showToast(`Error: ${payload.message}`, true);
      break;
  }
}

function setStatus(text, online) {
  const el = document.getElementById('status');
  el.textContent = text;
  el.classList.toggle('offline', !online);
}

function showToast(msg, isError = false) {
  const el = document.createElement('div');
  el.className = `toast${isError ? ' error' : ''}`;
  el.textContent = msg;
  document.body.appendChild(el);
  setTimeout(() => el.remove(), 4000);
}

function addPeer(id) {
  if (peers.includes(id)) return;
  peers.push(id);
  renderPeers();
  if (!activePeer) selectPeer(id);
}

function removePeer(id) {
  peers = peers.filter(p => p !== id);
  renderPeers();
  if (activePeer === id) {
    activePeer = peers[0] || null;
    updateChatHeader();
    updateInputState();
  }
}

function renderPeers() {
  const list = document.getElementById('peer-list');
  const noPeers = document.getElementById('no-peers');
  const count = document.getElementById('peer-count');
  list.innerHTML = '';
  count.textContent = peers.length;
  noPeers.classList.toggle('hidden', peers.length > 0);
  peers.forEach(id => {
    const li = document.createElement('li');
    li.textContent = id;
    li.className = id === activePeer ? 'active' : '';
    li.onclick = () => selectPeer(id);
    list.appendChild(li);
  });
}

function selectPeer(id) {
  activePeer = id;
  renderPeers();
  updateChatHeader();
  updateInputState();
  document.getElementById('empty-state')?.classList.add('hidden');
}

function updateChatHeader() {
  document.getElementById('chat-title').textContent =
    activePeer ? `Chat with ${activePeer}` : 'Select or connect a peer';
}

function updateInputState() {
  const enabled = !!activePeer && !activeCall;
  document.getElementById('message-input').disabled = !enabled;
  document.getElementById('send-btn').disabled = !enabled;
  document.getElementById('send-file-btn').disabled = !enabled;
  document.getElementById('voice-call-btn').disabled = !activePeer || !!activeCall;
  document.getElementById('video-call-btn').disabled = !activePeer || !!activeCall;
}

function updateCallUI() {
  const bar = document.getElementById('call-status');
  const endBtn = document.getElementById('end-call-btn');
  if (activeCall) {
    const kind = activeCall.video ? 'Video' : 'Voice';
    bar.textContent = `${kind} call with ${activeCall.peer}`;
    bar.classList.remove('hidden');
    endBtn.classList.remove('hidden');
  } else {
    bar.classList.add('hidden');
    endBtn.classList.add('hidden');
  }
  updateInputState();
}

function appendMessage(content, direction, sender, opts = {}) {
  document.getElementById('empty-state')?.classList.add('hidden');
  const div = document.createElement('div');
  div.className = `message ${direction}`;

  if (opts.kind === 'image' && opts.path) {
    const img = document.createElement('img');
    img.src = convertFileSrc(opts.path);
    img.alt = content;
    img.className = 'msg-media';
    div.appendChild(img);
  } else if (opts.kind === 'video' && opts.path) {
    const vid = document.createElement('video');
    vid.src = convertFileSrc(opts.path);
    vid.controls = true;
    vid.className = 'msg-media';
    div.appendChild(vid);
  } else {
    const text = document.createElement('div');
    text.innerHTML = escapeHtml(content);
    div.appendChild(text);
  }

  const meta = document.createElement('div');
  meta.className = 'meta';
  meta.textContent = sender || '';
  div.appendChild(meta);

  document.getElementById('messages').appendChild(div);
  div.scrollIntoView({ behavior: 'smooth' });
}

function escapeHtml(text) {
  const d = document.createElement('div');
  d.textContent = text;
  return d.innerHTML;
}

function showSas(sas) {
  const el = document.getElementById('sas-display');
  el.textContent = sas;
  el.classList.remove('hidden');
}

function showTransferProgress(filename, progress) {
  const bar = document.getElementById('transfer-bar');
  const label = document.getElementById('transfer-label');
  const fill = document.getElementById('transfer-progress');
  bar.classList.remove('hidden');
  label.textContent = `${filename}: ${Math.round(progress * 100)}%`;
  fill.style.width = `${Math.round(progress * 100)}%`;
}

function hideTransferProgress() {
  document.getElementById('transfer-bar').classList.add('hidden');
}

document.getElementById('copy-qr').onclick = async () => {
  const qr = document.getElementById('qr-payload').textContent;
  await navigator.clipboard.writeText(qr);
  showToast('QR payload copied');
};

document.getElementById('connect-serial').onclick = async () => {
  const port = document.getElementById('serial-port').value;
  if (!port) return;
  try {
    await invoke('connect_serial', { portName: port, baudRate: 115200 });
    showToast(`Connecting serial: ${port}`);
  } catch (e) {
    showToast(`Serial error: ${e}`, true);
  }
};

document.getElementById('connect-quic').onclick = async () => {
  const addr = document.getElementById('quic-addr').value;
  if (!addr) return;
  try {
    await invoke('connect_quic', { addr });
    showToast(`Connecting QUIC: ${addr}`);
  } catch (e) {
    showToast(`QUIC error: ${e}`, true);
  }
};

document.getElementById('verify-peer').onclick = async () => {
  const qr = document.getElementById('remote-qr').value;
  if (!qr || !activePeer) return;
  try {
    const sas = await invoke('handshake', { peerId: activePeer, remoteQr: qr });
    showSas(sas);
  } catch (e) {
    showToast(`Handshake error: ${e}`, true);
  }
};

document.getElementById('voice-call-btn').onclick = async () => {
  if (!activePeer) return;
  try {
    const callId = await invoke('start_voice_call', { peerId: activePeer });
    activeCall = { id: callId, peer: activePeer, video: false };
    updateCallUI();
    showToast(`Voice call started (${callId})`);
  } catch (e) {
    showToast(`Call error: ${e}`, true);
  }
};

document.getElementById('video-call-btn').onclick = async () => {
  if (!activePeer) return;
  try {
    const callId = await invoke('start_video_call', { peerId: activePeer });
    activeCall = { id: callId, peer: activePeer, video: true };
    updateCallUI();
    showToast(`Video call started (${callId})`);
  } catch (e) {
    showToast(`Call error: ${e}`, true);
  }
};

document.getElementById('end-call-btn').onclick = async () => {
  if (!activeCall) return;
  try {
    await invoke('end_call', { callId: activeCall.id });
    activeCall = null;
    updateCallUI();
    showToast('Call ended');
  } catch (e) {
    showToast(`End call error: ${e}`, true);
  }
};

document.getElementById('send-btn').onclick = sendMessage;
document.getElementById('message-input').onkeydown = (e) => {
  if (e.key === 'Enter') sendMessage();
};

document.getElementById('send-file-btn').onclick = async () => {
  if (!activePeer) return;

  let filePath = null;
  try {
    filePath = await openFileDialog({ multiple: false });
  } catch (e) {
    console.warn('Dialog open failed:', e);
  }

  if (!filePath) return;

  try {
    const result = await invoke('send_file', { peerId: activePeer, filePath });
    const kind = mediaKind(result.filename);
    if (kind === 'image' || kind === 'video') {
      appendMessage(result.filename, 'sent', 'You', { kind, path: filePath });
    } else {
      appendMessage(`📤 Sending: ${result.filename}`, 'sent', 'You');
    }
    showTransferProgress(result.filename, result.progress || 0);
  } catch (e) {
    showToast(`File send error: ${e}`, true);
  }
};

async function sendMessage() {
  const input = document.getElementById('message-input');
  const content = input.value.trim();
  if (!content || !activePeer) return;
  try {
    await invoke('send_message', { peerId: activePeer, content });
    appendMessage(content, 'sent', 'You');
    input.value = '';
  } catch (e) {
    showToast(`Send error: ${e}`, true);
  }
}

init();