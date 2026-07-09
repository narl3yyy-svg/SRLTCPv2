// SRLTCP v0.2.15 Desktop Frontend

const STORAGE_KEY = 'srltcp_v0.2.15';

function loadState() {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}');
  } catch (_) {
    return {};
  }
}

function saveState(patch) {
  const state = { ...loadState(), ...patch };
  localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
}

let displayName = loadState().displayName || '';
let savedContacts = loadState().contacts || [];

function pick(result, ...keys) {
  for (const k of keys) {
    if (result?.[k] != null && result[k] !== '') return result[k];
  }
  return '';
}

const invoke = window.__TAURI__?.core?.invoke
  ?? (async (cmd, args) => { console.log(`[mock] ${cmd}`, args); return null; });
const listen = window.__TAURI__?.event?.listen ?? (async () => () => {});
const openFileDialog = window.__TAURI__?.dialog?.open ?? (async () => null);
const convertFileSrc = window.__TAURI__?.core?.convertFileSrc ?? ((p) => p);

let activePeer = null;
let connectedPeer = null;
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
  return id.replace(/^peer:/, '').replace(/^iroh:/, '').replace(/^quic:/, '').slice(0, 12);
}

function pubkeyFromPeerId(id) {
  if (!id) return '';
  return id.startsWith('peer:') ? id.slice(5).toLowerCase() : '';
}

function migratePeerId(oldId, newId) {
  if (!oldId || !newId || oldId === newId) return;
  peers = peers.map(p => (p === oldId ? newId : p));
  peers = peers.filter((p, i, a) => a.indexOf(p) === i);
  if (activePeer === oldId) activePeer = newId;
  if (peerVerified.has(oldId)) {
    peerVerified.set(newId, peerVerified.get(oldId));
    peerVerified.delete(oldId);
  }
  if (chatHistory[oldId]) {
    chatHistory[newId] = chatHistory[oldId];
    delete chatHistory[oldId];
  }
  savedContacts = savedContacts.map(c => (c.id === oldId ? { ...c, id: newId } : c));
  persistContacts();
  renderPeers();
  renderPeerChips();
  renderContactsList();
  updateChatHeader();
}

async function syncTrustedPubkeys() {
  const pubkeys = savedContacts
    .filter(c => c.verified)
    .map(c => pubkeyFromPeerId(c.id))
    .filter(Boolean);
  if (pubkeys.length) {
    try { await invoke('load_trusted_pubkeys', { pubkeys }); } catch (_) {}
  }
}

async function reconnectContact(contact) {
  if (!contact.qr) {
    toast('No QR saved — connect via QR again', true);
    return;
  }
  try {
    const result = await invoke('connect_and_verify', { remoteQr: contact.qr });
    const peerId = pick(result, 'peer_id', 'peerId');
    const autoTrusted = result?.auto_trusted ?? result?.autoTrusted;
    const err = result?.error || (result.sas?.startsWith?.('error:') ? result.sas : null);
    if (err) throw new Error(String(err).replace(/^error:\s*/i, '').trim());
    if (peerId) {
      migratePeerId(contact.id, peerId);
      addPeerUnique(peerId);
      connectedPeer = peerId;
      if (autoTrusted) {
        peerVerified.set(peerId, true);
        selectPeer(peerId);
        toast('Reconnected to trusted peer');
      } else {
        showSasModal(peerId, pick(result, 'sas', 'Sas'));
      }
    }
  } catch (e) {
    toast(`Reconnect failed: ${e}`, true);
  }
}

function addPeerUnique(id) {
  reconcilePeers();
  addPeer(id);
}

function reconcilePeers() {
  const canonical = new Set(savedContacts.map(c => c.id));
  peers = peers.filter(id => !(id.startsWith('quic:') || id.startsWith('iroh:')) || canonical.size === 0);
  peers = [...new Set(peers)];
}

function softDisconnect(id) {
  invoke('disconnect_peer', { peerId: id }).catch(() => {});
  peers = peers.filter(p => p !== id);
  if (connectedPeer === id) connectedPeer = null;
  renderPeers();
  renderPeerChips();
  updateChatHeader();
  updateInputState();
  toast('Disconnected — contact saved, reconnect from Contacts');
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

async function refreshOwnQr() {
  try {
    await invoke('wait_for_engine');
    const qr = await invoke('get_qr_payload');
    document.getElementById('qr-payload').textContent = qr;
    try {
      const img = await invoke('get_qr_image');
      document.getElementById('qr-image').src = img;
      document.getElementById('qr-image').alt = 'Your QR code';
    } catch (_) {
      document.getElementById('qr-image').alt = 'QR image unavailable';
    }
    const ticket = await invoke('get_iroh_ticket');
    document.getElementById('iroh-ticket').textContent =
      ticket || 'iroh ticket pending…';
    return true;
  } catch (e) {
    document.getElementById('iroh-ticket').textContent = `iroh not ready: ${e}`;
    toast(`QR not ready: ${e}`, true);
    return false;
  }
}

async function init() {
  try {
    await refreshOwnQr();

    const ports = await invoke('list_serial_ports');
    const select = document.getElementById('serial-port');
    select.innerHTML = ports.length === 0
      ? '<option value="">No ports found</option>'
      : ports.map(p => {
          const path = p.path ?? p;
          const label = p.label ?? p;
          return `<option value="${escapeHtml(path)}">${escapeHtml(label)}</option>`;
        }).join('');

    document.getElementById('display-name').value = displayName;
    restoreContacts();
    await syncTrustedPubkeys();

    const existingPeers = await invoke('get_peers');
    existingPeers.forEach(id => { if (id.startsWith('peer:')) addPeerUnique(id); });
    connectedPeer = existingPeers.find(id => id.startsWith('peer:')) || null;
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
    case 'started':
      setStatus('Online', true);
      refreshOwnQr();
      break;
    case 'error':
      toast(p.message || p.error || 'Engine error', true);
      break;
    case 'message':
      appendMessage(p.content, 'received', shortPeer(pick(p, 'sender', 'Sender')));
      break;
    case 'peer_connected': {
      const id = pick(p, 'peer_id', 'peerId');
      if (id) {
        addPeerUnique(id);
        if (id.startsWith('peer:')) connectedPeer = id;
      }
      break;
    }
    case 'peer_disconnected': {
      const id = pick(p, 'peer_id', 'peerId');
      peers = peers.filter(p => p !== id);
      if (connectedPeer === id) connectedPeer = null;
      renderPeers();
      renderPeerChips();
      if (activePeer === id) {
        document.getElementById('messages')?.replaceChildren();
        loadChatForPeer(id).forEach(m => appendMessage(m.content, m.direction, m.sender, m.opts || {}, false));
        updateChatHeader();
        updateInputState();
      }
      break;
    }
    case 'sas_ready': {
      const id = pick(p, 'peer_id', 'peerId');
      const sas = pick(p, 'sas', 'Sas');
      const autoTrusted = p.auto_trusted ?? p.autoTrusted;
      if (id && autoTrusted) {
        peerVerified.set(id, true);
        connectedPeer = id;
        selectPeer(id);
        updateChatHeader();
        updateInputState();
        updateVerifyBanner();
        toast('Reconnected to trusted peer');
      } else {
        showSasModal(id, sas);
      }
      break;
    }
    case 'peer_id_updated': {
      const oldId = pick(p, 'old_id', 'oldId');
      const newId = pick(p, 'new_id', 'newId');
      if (connectedPeer === oldId || connectedPeer === null) connectedPeer = newId;
      migratePeerId(oldId, newId);
      if (activePeer === oldId || activePeer === null) selectPeer(newId);
      break;
    }
    case 'transfer_progress':
      updateTransfer(pick(p, 'id', 'Id'), p.filename, p.progress, false);
      break;
    case 'transfer_complete':
      updateTransfer(pick(p, 'id', 'Id'), p.filename, 1, false);
      setTimeout(() => removeTransfer(pick(p, 'id', 'Id')), 2000);
      appendMessage(`📁 ${p.filename}`, 'system', 'Transfer complete');
      break;
    case 'call_started':
      activeCall = {
        id: pick(p, 'call_id', 'callId'),
        peer: pick(p, 'peer_id', 'peerId'),
        video: p.is_video ?? p.isVideo ?? false,
      };
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
  if (!peerVerified.has(id)) {
    const saved = savedContacts.find(c => c.id === id);
    peerVerified.set(id, saved?.verified ?? false);
  }
  renderPeers();
  renderPeerChips();
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

function contactLabel(id) {
  const c = savedContacts.find(x => x.id === id);
  return c?.name || shortPeer(id);
}

function persistContacts() {
  saveState({
    displayName,
    contacts: savedContacts,
    chatHistory: chatHistory,
  });
}

const chatHistory = loadState().chatHistory || {};

function saveChatForPeer(peerId) {
  if (!peerId) return;
  chatHistory[peerId] = getMessagesForPeer(peerId);
  persistContacts();
}

function loadChatForPeer(peerId) {
  return chatHistory[peerId] || [];
}

let messageStore = {}; // peerId -> DOM-independent message list for persistence

function getMessagesForPeer(peerId) {
  return messageStore[peerId] || chatHistory[peerId] || [];
}

function setMessagesForPeer(peerId, msgs) {
  messageStore[peerId] = msgs;
  chatHistory[peerId] = msgs;
}

function restoreContacts() {
  savedContacts.forEach(c => {
    if (!peers.includes(c.id)) addPeer(c.id);
    if (c.verified) peerVerified.set(c.id, true);
  });
  renderPeers();
  renderContactsList();
}

function removeTrustedContact(id) {
  invoke('disconnect_peer', { peerId: id }).catch(() => {});
  removePeer(id);
  savedContacts = savedContacts.filter(c => c.id !== id);
  delete chatHistory[id];
  persistContacts();
  renderContactsList();
  toast(`Removed ${contactLabel(id)}`);
}

function renderContactsList() {
  const el = document.getElementById('contacts-list');
  if (!el) return;
  if (savedContacts.length === 0) {
    el.innerHTML = '<p class="hint">Verified peers are saved here automatically.</p>';
    return;
  }
  el.innerHTML = savedContacts.map(c => {
    const online = connectedPeer === c.id;
    const status = online ? '● online' : (c.verified ? '○ offline' : 'unverified');
    return `
    <div class="contact-row">
      <button class="contact-select" data-peer="${escapeHtml(c.id)}">
        <span class="contact-name">${escapeHtml(c.name || shortPeer(c.id))}</span>
        <span class="contact-meta">${status}</span>
      </button>
      ${c.verified && !online && c.qr ? `<button class="btn-sm" data-reconnect="${escapeHtml(c.id)}" title="Reconnect">↻</button>` : ''}
      ${online ? `<button class="btn-sm" data-disconnect="${escapeHtml(c.id)}" title="Disconnect">⏏</button>` : ''}
      <button class="btn-sm danger-sm" data-remove="${escapeHtml(c.id)}" title="Remove">✕</button>
    </div>`;
  }).join('');
  el.querySelectorAll('.contact-select').forEach(btn => {
    btn.onclick = () => {
      const id = btn.dataset.peer;
      const contact = savedContacts.find(c => c.id === id);
      if (connectedPeer === id) selectPeer(id);
      else if (contact?.verified && contact.qr) reconnectContact(contact);
      else selectPeer(id);
    };
  });
  el.querySelectorAll('[data-reconnect]').forEach(btn => {
    btn.onclick = (e) => {
      e.stopPropagation();
      const contact = savedContacts.find(c => c.id === btn.dataset.reconnect);
      if (contact) reconnectContact(contact);
    };
  });
  el.querySelectorAll('[data-disconnect]').forEach(btn => {
    btn.onclick = (e) => {
      e.stopPropagation();
      softDisconnect(btn.dataset.disconnect);
    };
  });
  el.querySelectorAll('[data-remove]').forEach(btn => {
    btn.onclick = () => removeTrustedContact(btn.dataset.remove);
  });
}

function renderPeers() {
  const list = document.getElementById('peer-list');
  const noPeers = document.getElementById('no-peers');
  document.getElementById('peer-count').textContent = peers.length;
  noPeers.classList.toggle('hidden', peers.length > 0);
  list.innerHTML = peers.map(id => {
    const verified = peerVerified.get(id);
    const badge = verified ? '<span class="verified-badge">✓</span>' : '<span class="unverified-badge">!</span>';
    const online = connectedPeer === id;
    return `<li class="${id === activePeer ? 'active' : ''}${verified ? ' verified' : ''}${online ? ' online' : ''}" data-peer="${escapeHtml(id)}">
      <span class="peer-dot"></span>${badge}${escapeHtml(contactLabel(id))}
      <button class="peer-remove" data-disconnect="${escapeHtml(id)}" title="Disconnect">⏏</button>
    </li>`;
  }).join('');
  list.querySelectorAll('li').forEach(li => {
    li.onclick = (e) => {
      if (e.target.classList.contains('peer-remove')) return;
      selectPeer(li.dataset.peer);
    };
  });
  list.querySelectorAll('.peer-remove').forEach(btn => {
    btn.onclick = (e) => {
      e.stopPropagation();
      softDisconnect(btn.dataset.disconnect);
    };
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
  const container = document.getElementById('messages');
  if (container) {
    container.replaceChildren();
    const stored = loadChatForPeer(id);
    stored.forEach(m => appendMessage(m.content, m.direction, m.sender, m.opts || {}, false));
  }
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
  const online = activePeer && connectedPeer === activePeer;
  const verified = activePeer && peerVerified.get(activePeer);
  const inCall = !!activeCall;
  const canChat = hasPeer && online && verified && !inCall;
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

function appendMessage(content, direction, sender, opts = {}, persist = true) {
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

  if (persist && activePeer) {
    const msgs = getMessagesForPeer(activePeer);
    msgs.push({ content, direction, sender, opts, time: nowTime() });
    setMessagesForPeer(activePeer, msgs);
    persistContacts();
  }
}

function escapeHtml(t) {
  const d = document.createElement('div');
  d.textContent = t;
  return d.innerHTML;
}

function showSasModal(peerId, sas) {
  const code = String(sas || '').trim();
  if (!code || code.length < 4) {
    toast('SAS code unavailable — retry verification', true);
    return;
  }
  pendingSas = { peerId, sas: code };
  const el = document.getElementById('sas-code');
  el.textContent = code;
  el.setAttribute('aria-label', `Security code ${code}`);
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

  const btn = document.getElementById('verify-secure');
  btn.disabled = true;
  btn.textContent = 'Connecting…';

  try {
    toast('Connecting and running secure handshake…');
    const result = await invoke('connect_and_verify', { remoteQr: qr });
    const err = result?.error;
    const peerId = pick(result, 'peer_id', 'peerId');
    const sas = pick(result, 'sas', 'Sas');
    const autoTrusted = result?.auto_trusted ?? result?.autoTrusted;
    if (err || sas?.startsWith?.('error:')) {
      throw new Error((err || sas).replace(/^error:\s*/i, '').trim());
    }
    if (!peerId) {
      throw new Error('No peer connected — check QR is v4 (iroh) and peer is online');
    }
    if (peerId) {
      addPeerUnique(peerId);
      connectedPeer = peerId;
      selectPeer(peerId);
    }
    document.getElementById('remote-qr').value = '';
    window._lastConnectQr = qr;
    if (autoTrusted) {
      peerVerified.set(peerId, true);
      const name = displayName || shortPeer(peerId);
      const existing = savedContacts.findIndex(c => c.id === peerId);
      const entry = { id: peerId, name, verified: true, qr };
      if (existing >= 0) savedContacts[existing] = entry;
      else savedContacts.push(entry);
      persistContacts();
      await syncTrustedPubkeys();
      updateChatHeader();
      updateInputState();
      updateVerifyBanner();
      toast('Reconnected to trusted peer — secure channel ready');
    } else {
      showSasModal(peerId, sas);
      toast('Connected — confirm the SAS code with your peer');
    }
  } catch (e) {
    toast(`Verification failed: ${e}`, true);
  } finally {
    btn.disabled = false;
    btn.innerHTML = '<span class="btn-icon">🔒</span> Connect &amp; Verify (QR + SAS)';
  }
}

// ── Event bindings ─────────────────────────────────────────────────
document.getElementById('copy-qr').onclick = async () => {
  await navigator.clipboard.writeText(document.getElementById('qr-payload').textContent);
  toast('QR payload copied — share with your peer');
};

document.getElementById('verify-secure').onclick = () => runVerification();
document.getElementById('clear-remote-qr')?.addEventListener('click', () => {
  document.getElementById('remote-qr').value = '';
});
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

document.getElementById('sas-confirm').onclick = async () => {
  // async handler
  if (pendingSas) {
    try {
      await invoke('confirm_peer_trusted', { peerId: pendingSas.peerId });
    } catch (e) {
      toast(`Trust confirmation failed: ${e}`, true);
      return;
    }
    peerVerified.set(pendingSas.peerId, true);
    connectedPeer = pendingSas.peerId;
    const name = displayName || shortPeer(pendingSas.peerId);
    const existing = savedContacts.findIndex(c => c.id === pendingSas.peerId);
    const qr = window._lastConnectQr || '';
    const entry = { id: pendingSas.peerId, name, verified: true, qr };
    if (existing >= 0) savedContacts[existing] = entry;
    else savedContacts.push(entry);
    persistContacts();
    await syncTrustedPubkeys();
    selectPeer(pendingSas.peerId);
    toast('Peer verified — secure channel established');
    hideSasModal();
    renderPeers();
    renderPeerChips();
    renderContactsList();
    updateChatHeader();
    updateInputState();
    updateVerifyBanner();
  }
};

document.getElementById('save-display-name')?.addEventListener('click', () => {
  displayName = document.getElementById('display-name').value.trim();
  persistContacts();
  toast('Display name saved');
});

document.getElementById('check-updates')?.addEventListener('click', () => {
  toast('Run: git pull && ./run.sh  —  Releases: github.com/narl3yyy-svg/SRLTCPv2');
});

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
  softDisconnect(activePeer);
};

document.getElementById('voice-call-btn').onclick = async () => {
  toast('Voice calls require platform WebRTC (coming soon)', true);
};

document.getElementById('video-call-btn').onclick = async () => {
  toast('Video calls require platform WebRTC (coming soon)', true);
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
  if (connectedPeer !== activePeer) {
    toast('Peer offline — reconnect from Contacts', true);
    return;
  }
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
  if (connectedPeer !== activePeer) {
    toast('Peer offline — reconnect from Contacts', true);
    return;
  }
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