// SRLTCP v0.2.5 Desktop Frontend

const invoke = window.__TAURI__?.core?.invoke
  ?? (async (cmd, args) => { console.log(`[mock] ${cmd}`, args); return null; });
const listen = window.__TAURI__?.event?.listen ?? (async () => () => {});
const openFileDialog = window.__TAURI__?.dialog?.open ?? (async () => null);
const convertFileSrc = window.__TAURI__?.core?.convertFileSrc ?? ((p) => p);

let activePeer = null;
let peers = [];
const peerVerified = new Map();
let activeCall = null;
let pendingSas = null;
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

function shortPeer(id) {
  if (!id) return '';
  return id.replace('quic:', '').slice(0, 20);
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
    try {
      const img = await invoke('get_qr_image');
      document.getElementById('qr-image').src = img;
    } catch (_) {
      document.getElementById('qr-image').alt = 'QR unavailable';
    }

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
      appendMessage(p.content, 'received', shortPeer(p.sender));
      break;
    case 'peer_connected':
      addPeer(p.peer_id);
      toast(`Peer connected — verify with QR + SAS`);
      if (!activePeer) selectPeer(p.peer_id);
      updateVerifyBanner();
      break;
    case 'peer_disconnected':
      removePeer(p.peer_id);
      peerVerified.delete(p.peer_id);
      toast(`Disconnected: ${shortPeer(p.peer_id)}`);
      break;
    case 'sas_ready':
      showSasModal(p.peer_id, p.sas);
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
  setTimeout(() => el.remove(), 4500);
}

function addPeer(id) {
  if (peers.includes(id)) return;
  peers.push(id);
  if (!peerVerified.has(id)) peerVerified.set(id, false);
  renderPeers();
  renderPeerChips();
  if (!activePeer) selectPeer(id);
}

function removePeer(id) {
  peers = peers.filter(p => p !== id);
  peerVerified.delete(id);
  renderPeers();
  renderPeerChips();
  if (activePeer === id) {
    activePeer = peers[0] || null;
    updateChatHeader();
    updateInputState();
    updateVerifyBanner();
  }
}

function renderPeers() {
  const list = document.getElementById('peer-list');
  const noPeers = document.getElementById('no-peers');
  document.getElementById('peer-count').textContent = peers.length;
  noPeers.classList.toggle('hidden', peers.length > 0);
  list.innerHTML = peers.map(id => {
    const verified = peerVerified.get(id);
    const badge = verified ? '<span class="verified-badge">✓</span>' : '<span class="unverified-badge">!</span>';
    return `<li class="${id === activePeer ? 'active' : ''}${verified ? ' verified' : ''}" data-peer="${id}">
      <span class="peer-dot"></span>${badge}${escapeHtml(shortPeer(id))}
    </li>`;
  }).join('');
  list.querySelectorAll('li').forEach(li => {
    li.onclick = () => selectPeer(li.dataset.peer);
  });
}

function renderPeerChips() {
  const bar = document.getElementById('peer-chips');
  if (peers.length === 0) { bar.classList.add('hidden'); return; }
  bar.classList.remove('hidden');
  bar.innerHTML = peers.map(id => {
    const v = peerVerified.get(id) ? ' ✓' : '';
    return `<button class="chip${id === activePeer ? ' active' : ''}${peerVerified.get(id) ? ' verified' : ''}" data-peer="${id}">${escapeHtml(shortPeer(id))}${v}</button>`;
  }).join('');
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
  updateVerifyBanner();
  document.getElementById('empty-state')?.classList.add('hidden');
}

function updateChatHeader() {
  const title = document.getElementById('chat-title');
  const sub = document.getElementById('chat-subtitle');
  if (activePeer) {
    const verified = peerVerified.get(activePeer);
    title.textContent = shortPeer(activePeer);
    sub.textContent = verified ? '✓ Verified — end-to-end encrypted' : '⚠ Not verified — run SAS check';
  } else {
    title.textContent = 'Select a peer';
    sub.textContent = 'Share your QR to get started';
  }
}

function updateVerifyBanner() {
  const banner = document.getElementById('verify-banner');
  const needsVerify = activePeer && !peerVerified.get(activePeer);
  banner.classList.toggle('hidden', !needsVerify);
}

function updateInputState() {
  const hasPeer = !!activePeer;
  const verified = activePeer && peerVerified.get(activePeer);
  const inCall = !!activeCall;
  const canChat = hasPeer && verified && !inCall;
  ['message-input', 'send-btn', 'send-file-btn'].forEach(id => {
    document.getElementById(id).disabled = !canChat;
  });
  document.getElementById('voice-call-btn').disabled = !hasPeer || !verified || inCall;
  document.getElementById('video-call-btn').disabled = !hasPeer || !verified || inCall;
  document.getElementById('disconnect-btn').disabled = !hasPeer;
}

function updateCallUI() {
  const bar = document.getElementById('call-status');
  const endBtn = document.getElementById('end-call-btn');
  if (activeCall) {
    const kind = activeCall.video ? 'Video' : 'Voice';
    bar.innerHTML = `<span class="call-pulse"></span> ${kind} call — ${shortPeer(activeCall.peer)}`;
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

function showSasModal(peerId, sas) {
  pendingSas = { peerId, sas };
  document.getElementById('sas-code').textContent = sas;
  document.getElementById('sas-peer-label').textContent = `Peer: ${shortPeer(peerId)}`;
  document.getElementById('sas-modal').classList.remove('hidden');
}

function hideSasModal() {
  document.getElementById('sas-modal').classList.add('hidden');
  pendingSas = null;
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

async function runVerification() {
  const qr = document.getElementById('remote-qr').value.trim();
  if (!qr) {
    toast('Paste the peer QR code first', true);
    document.querySelector('[data-panel="connect"]').click();
    return;
  }

  try {
    toast('Running secure handshake…');
    const result = await invoke('connect_and_verify', { remoteQr: qr });
    if (result.peer_id && !peers.includes(result.peer_id)) addPeer(result.peer_id);
    selectPeer(result.peer_id);
    showSasModal(result.peer_id, result.sas);
  } catch (e) {
    toast(`Verification failed: ${e}`, true);
  }
}

// ── Event bindings ─────────────────────────────────────────────────
document.getElementById('copy-qr').onclick = async () => {
  await navigator.clipboard.writeText(document.getElementById('qr-payload').textContent);
  toast('QR payload copied — share with your peer');
};

document.getElementById('verify-secure').onclick = () => runVerification();
document.getElementById('verify-banner-btn').onclick = () => {
  document.querySelector('[data-panel="connect"]').click();
};

document.getElementById('connect-serial').onclick = async () => {
  const port = document.getElementById('serial-port').value;
  if (!port) return;
  try {
    await invoke('connect_serial', { portName: port, baudRate: 115200 });
    toast(`Serial connected: ${port}`);
  } catch (e) { toast(`Serial error: ${e}`, true); }
};

document.getElementById('sas-confirm').onclick = () => {
  if (pendingSas) {
    peerVerified.set(pendingSas.peerId, true);
    selectPeer(pendingSas.peerId);
    toast('Peer verified — secure channel established');
    hideSasModal();
    renderPeers();
    renderPeerChips();
    updateChatHeader();
    updateInputState();
    updateVerifyBanner();
  }
};

document.getElementById('sas-reject').onclick = async () => {
  if (pendingSas) {
    toast('SAS mismatch — disconnecting peer (possible MITM)', true);
    try { await invoke('disconnect_peer', { peerId: pendingSas.peerId }); } catch (_) {}
    removePeer(pendingSas.peerId);
    hideSasModal();
  }
};

document.querySelector('.modal-backdrop')?.addEventListener('click', () => {
  toast('You must confirm or reject the SAS code', true);
});

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
    toast(`Disconnected ${shortPeer(activePeer)}`);
  } catch (e) { toast(`Disconnect error: ${e}`, true); }
};

document.getElementById('voice-call-btn').onclick = async () => {
  if (!activePeer) return;
  try {
    const callId = await invoke('start_voice_call', { peerId: activePeer });
    if (callId.startsWith('error:')) throw new Error(callId);
    activeCall = { id: callId, peer: activePeer, video: false };
    updateCallUI();
    toast('Voice call started');
  } catch (e) { toast(`Call error: ${e}`, true); }
};

document.getElementById('video-call-btn').onclick = async () => {
  if (!activePeer) return;
  try {
    const callId = await invoke('start_video_call', { peerId: activePeer });
    if (callId.startsWith('error:')) throw new Error(callId);
    activeCall = { id: callId, peer: activePeer, video: true };
    updateCallUI();
    toast('Video call started');
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
  if (!peerVerified.get(activePeer)) {
    toast('Verify peer with SAS before messaging', true);
    return;
  }
  try {
    await invoke('send_message', { peerId: activePeer, content });
    appendMessage(content, 'sent', 'You');
    input.value = '';
  } catch (e) { toast(`Send error: ${e}`, true); }
}

init();