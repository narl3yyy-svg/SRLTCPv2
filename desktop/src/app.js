// SRLTCP v0.2.1 Desktop Frontend

const invoke = window.__TAURI__?.core?.invoke
  ?? (async (cmd, args) => { console.log(`[mock] ${cmd}`, args); return null; });
const listen = window.__TAURI__?.event?.listen ?? (async () => () => {});
const openFileDialog = window.__TAURI__?.dialog?.open ?? (async () => null);
const convertFileSrc = window.__TAURI__?.core?.convertFileSrc ?? ((p) => p);

let activePeer = null;
let peers = [];
let activeCall = null;
const transfers = new Map();

const IMAGE_EXT = new Set(['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp']);
const VIDEO_EXT = new Set(['mp4', 'webm', 'mkv', 'mov', '3gp']);

function mediaKind(filename) {
  const ext = (filename || '').split('.').pop()?.toLowerCase() || '';
  if (IMAGE_EXT.has(ext)) return 'image';
  if (VIDEO_EXT.has(ext)) return 'video';
  return 'file';
}

function nowTime() {
  return new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

// ── Sidebar navigation ─────────────────────────────────────────────
document.querySelectorAll('.nav-btn').forEach(btn => {
  btn.onclick = () => {
    document.querySelectorAll('.nav-btn').forEach(b => b.classList.remove('active'));
    document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
    btn.classList.add('active');
    document.getElementById(`panel-${btn.dataset.panel}`).classList.add('active');
  };
});

async function init() {
  try {
    const qr = await invoke('get_qr_payload');
    document.getElementById('qr-payload').textContent = qr;

    const ports = await invoke('list_serial_ports');
    const select = document.getElementById('serial-port');
    select.innerHTML = ports.length === 0
      ? '<option value="">No ports found</option>'
      : ports.map(p => `<option value="${p}">${p}</option>`).join('');

    const existingPeers = await invoke('get_peers');
    existingPeers.forEach(addPeer);
    setStatus('Online', true);
  } catch (e) {
    console.error('Init failed:', e);
    setStatus('Offline', false);
    toast('Failed to initialize engine', true);
  }

  await listen('srltcp-event', (e) => handleEvent(e.payload));
}

function handleEvent(p) {
  switch (p.type) {
    case 'message':
      appendMessage(p.content, 'received', p.sender);
      break;
    case 'peer_connected':
      addPeer(p.peer_id);
      toast(`Connected: ${p.peer_id}`);
      break;
    case 'peer_disconnected':
      removePeer(p.peer_id);
      toast(`Disconnected: ${p.peer_id}`);
      break;
    case 'sas_ready':
      showSas(p.sas);
      break;
    case 'transfer_progress':
      updateTransfer(p.id, p.filename, p.progress, false);
      break;
    case 'transfer_complete':
      updateTransfer(p.id, p.filename, 1, false);
      setTimeout(() => removeTransfer(p.id), 2000);
      appendMessage(`📁 ${p.filename}`, 'system', 'Transfer complete');
      break;
    case 'call_started':
      activeCall = { id: p.call_id, peer: p.peer_id, video: p.is_video };
      updateCallUI();
      break;
    case 'call_ended':
      activeCall = null;
      updateCallUI();
      break;
    case 'started': setStatus('Online', true); break;
    case 'stopped': setStatus('Offline', false); break;
    case 'error':
      console.error(p.message);
      toast(p.message, true);
      break;
  }
}

function setStatus(text, online) {
  const el = document.getElementById('status');
  el.textContent = text;
  el.classList.toggle('offline', !online);
}

function toast(msg, isError = false) {
  const c = document.getElementById('toast-container');
  const el = document.createElement('div');
  el.className = `toast${isError ? ' error' : ''}`;
  el.textContent = msg;
  c.appendChild(el);
  setTimeout(() => el.remove(), 4000);
}

function addPeer(id) {
  if (peers.includes(id)) return;
  peers.push(id);
  renderPeers();
  renderPeerChips();
  if (!activePeer) selectPeer(id);
}

function removePeer(id) {
  peers = peers.filter(p => p !== id);
  renderPeers();
  renderPeerChips();
  if (activePeer === id) {
    activePeer = peers[0] || null;
    updateChatHeader();
    updateInputState();
  }
}

function renderPeers() {
  const list = document.getElementById('peer-list');
  const noPeers = document.getElementById('no-peers');
  document.getElementById('peer-count').textContent = peers.length;
  noPeers.classList.toggle('hidden', peers.length > 0);
  list.innerHTML = peers.map(id => `
    <li class="${id === activePeer ? 'active' : ''}" data-peer="${id}">
      <span class="peer-dot"></span>${escapeHtml(id)}
    </li>`).join('');
  list.querySelectorAll('li').forEach(li => {
    li.onclick = () => selectPeer(li.dataset.peer);
  });
}

function renderPeerChips() {
  const bar = document.getElementById('peer-chips');
  if (peers.length === 0) { bar.classList.add('hidden'); return; }
  bar.classList.remove('hidden');
  bar.innerHTML = peers.map(id =>
    `<button class="chip${id === activePeer ? ' active' : ''}" data-peer="${id}">${escapeHtml(id.slice(0, 12))}</button>`
  ).join('');
  bar.querySelectorAll('.chip').forEach(c => {
    c.onclick = () => selectPeer(c.dataset.peer);
  });
}

function selectPeer(id) {
  activePeer = id;
  renderPeers();
  renderPeerChips();
  updateChatHeader();
  updateInputState();
  document.getElementById('empty-state')?.classList.add('hidden');
}

function updateChatHeader() {
  const title = document.getElementById('chat-title');
  const sub = document.getElementById('chat-subtitle');
  if (activePeer) {
    title.textContent = activePeer;
    sub.textContent = 'End-to-end encrypted';
  } else {
    title.textContent = 'Select a peer';
    sub.textContent = 'Connect or select a peer to start';
  }
}

function updateInputState() {
  const hasPeer = !!activePeer;
  const inCall = !!activeCall;
  const enabled = hasPeer && !inCall;
  ['message-input', 'send-btn', 'send-file-btn'].forEach(id => {
    document.getElementById(id).disabled = !enabled;
  });
  document.getElementById('voice-call-btn').disabled = !hasPeer || inCall;
  document.getElementById('video-call-btn').disabled = !hasPeer || inCall;
  document.getElementById('disconnect-btn').disabled = !hasPeer;
}

function updateCallUI() {
  const bar = document.getElementById('call-status');
  const endBtn = document.getElementById('end-call-btn');
  if (activeCall) {
    const kind = activeCall.video ? 'Video' : 'Voice';
    bar.innerHTML = `<span class="call-pulse"></span> ${kind} call active — ${activeCall.peer}`;
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
    img.className = 'msg-media';
    img.alt = content;
    div.appendChild(img);
  } else if (opts.kind === 'video' && opts.path) {
    const vid = document.createElement('video');
    vid.src = convertFileSrc(opts.path);
    vid.controls = true;
    vid.className = 'msg-media';
    div.appendChild(vid);
  } else {
    const text = document.createElement('div');
    text.className = 'msg-text';
    text.textContent = content;
    div.appendChild(text);
  }

  const meta = document.createElement('div');
  meta.className = 'meta';
  meta.textContent = `${sender || ''} · ${nowTime()}`;
  div.appendChild(meta);

  const container = document.getElementById('messages');
  container.appendChild(div);
  div.scrollIntoView({ behavior: 'smooth' });
}

function escapeHtml(t) {
  const d = document.createElement('div');
  d.textContent = t;
  return d.innerHTML;
}

function showSas(sas) {
  const el = document.getElementById('sas-display');
  el.textContent = sas;
  el.classList.remove('hidden');
  document.querySelector('[data-panel="identity"]').click();
}

function updateTransfer(id, filename, progress, outgoing) {
  transfers.set(id, { filename, progress, outgoing });
  renderTransfers();
}

function removeTransfer(id) {
  transfers.delete(id);
  renderTransfers();
}

function renderTransfers() {
  const panel = document.getElementById('transfers-panel');
  if (transfers.size === 0) { panel.classList.add('hidden'); return; }
  panel.classList.remove('hidden');
  panel.innerHTML = [...transfers.entries()].map(([id, t]) => `
    <div class="transfer-item">
      <span>${t.outgoing ? '↑' : '↓'} ${escapeHtml(t.filename)} — ${Math.round(t.progress * 100)}%</span>
      <div class="progress-track"><div class="progress-fill" style="width:${Math.round(t.progress * 100)}%"></div></div>
    </div>`).join('');
}

// ── Event bindings ─────────────────────────────────────────────────
document.getElementById('copy-qr').onclick = async () => {
  await navigator.clipboard.writeText(document.getElementById('qr-payload').textContent);
  toast('QR payload copied');
};

document.getElementById('connect-serial').onclick = async () => {
  const port = document.getElementById('serial-port').value;
  if (!port) return;
  try {
    await invoke('connect_serial', { portName: port, baudRate: 115200 });
    toast(`Serial: ${port}`);
  } catch (e) { toast(`Serial error: ${e}`, true); }
};

document.getElementById('connect-quic').onclick = async () => {
  const addr = document.getElementById('quic-addr').value;
  if (!addr) return;
  try {
    await invoke('connect_quic', { addr });
    toast(`QUIC: ${addr}`);
  } catch (e) { toast(`QUIC error: ${e}`, true); }
};

document.getElementById('verify-peer').onclick = async () => {
  const qr = document.getElementById('remote-qr').value;
  if (!qr || !activePeer) return;
  try {
    const sas = await invoke('handshake', { peerId: activePeer, remoteQr: qr });
    showSas(sas);
  } catch (e) { toast(`Handshake error: ${e}`, true); }
};

document.getElementById('refresh-peers').onclick = async () => {
  try {
    const list = await invoke('get_peers');
    peers = [];
    list.forEach(addPeer);
    toast(`Peers refreshed (${list.length})`);
  } catch (e) { toast(`Refresh error: ${e}`, true); }
};

document.getElementById('disconnect-btn').onclick = async () => {
  if (!activePeer) return;
  try {
    await invoke('disconnect_peer', { peerId: activePeer });
    removePeer(activePeer);
    toast(`Disconnected ${activePeer}`);
  } catch (e) { toast(`Disconnect error: ${e}`, true); }
};

document.getElementById('voice-call-btn').onclick = async () => {
  if (!activePeer) return;
  try {
    const callId = await invoke('start_voice_call', { peerId: activePeer });
    if (callId.startsWith('error:')) throw new Error(callId);
    activeCall = { id: callId, peer: activePeer, video: false };
    updateCallUI();
    toast(`Voice call started`);
  } catch (e) { toast(`Call error: ${e}`, true); }
};

document.getElementById('video-call-btn').onclick = async () => {
  if (!activePeer) return;
  try {
    const callId = await invoke('start_video_call', { peerId: activePeer });
    if (callId.startsWith('error:')) throw new Error(callId);
    activeCall = { id: callId, peer: activePeer, video: true };
    updateCallUI();
    toast(`Video call started`);
  } catch (e) { toast(`Call error: ${e}`, true); }
};

document.getElementById('end-call-btn').onclick = async () => {
  if (!activeCall) return;
  try {
    await invoke('end_call', { callId: activeCall.id });
    activeCall = null;
    updateCallUI();
    toast('Call ended');
  } catch (e) { toast(`End call error: ${e}`, true); }
};

document.getElementById('send-file-btn').onclick = async () => {
  if (!activePeer) return;
  let filePath;
  try { filePath = await openFileDialog({ multiple: false }); } catch (_) {}
  if (!filePath) return;
  try {
    const result = await invoke('send_file', { peerId: activePeer, filePath });
    const kind = mediaKind(result.filename);
    updateTransfer(result.transfer_id, result.filename, result.progress || 0, true);
    if (kind === 'image' || kind === 'video') {
      appendMessage(result.filename, 'sent', 'You', { kind, path: filePath });
    } else {
      appendMessage(`Sending: ${result.filename}`, 'sent', 'You');
    }
  } catch (e) { toast(`File error: ${e}`, true); }
};

document.getElementById('send-btn').onclick = sendMessage;
document.getElementById('message-input').onkeydown = (e) => {
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage(); }
};

async function sendMessage() {
  const input = document.getElementById('message-input');
  const content = input.value.trim();
  if (!content || !activePeer) return;
  try {
    await invoke('send_message', { peerId: activePeer, content });
    appendMessage(content, 'sent', 'You');
    input.value = '';
  } catch (e) { toast(`Send error: ${e}`, true); }
}

init();