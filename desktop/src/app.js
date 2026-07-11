// SRLTCP v0.3.2 Desktop Frontend

const STORAGE_KEY = 'srltcp_v0.3.2';
const LEGACY_STORAGE_KEYS = [
  'srltcp_v0.2.16', 'srltcp_v0.2.24', 'srltcp_v0.2.25',
  'srltcp_v0.2.26', 'srltcp_v0.2.27', 'srltcp_v0.2.28', 'srltcp_v0.2.29',
  'srltcp_v0.2.30', 'srltcp_v0.2.31', 'srltcp_v0.2.32', 'srltcp_v0.3.0', 'srltcp_v0.3.1',
];

function notifyDesktop(title, body) {
  try {
    if (!loadState().desktopNotifications) return;
    if (typeof Notification === 'undefined') return;
    if (Notification.permission === 'granted') {
      new Notification(title, { body: body || '', silent: false });
    } else if (Notification.permission !== 'denied') {
      Notification.requestPermission().then((p) => {
        if (p === 'granted') new Notification(title, { body: body || '' });
      });
    }
  } catch (_) {}
}

function loadState() {
  for (const key of [STORAGE_KEY, ...LEGACY_STORAGE_KEYS]) {
    try {
      const raw = localStorage.getItem(key);
      if (raw) return JSON.parse(raw);
    } catch (_) {}
  }
  return {};
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
const connectedPeers = new Set();
const peerStatus = new Map();
const remoteDisplayNames = new Map();
const peerVerified = new Map();
let activeCall = null;
const activeCallRef = { current: null };
let pendingSas = null;
const transfers = new Map();
let receiveDir = '';

function formatSpeed(bps) {
  const mb = bps / (1024 * 1024);
  return mb >= 0.01 ? ` · ${mb.toFixed(2)} MB/s` : '';
}

function updateReceiveDirUI() {
  const el = document.getElementById('receive-dir-path');
  if (el) el.textContent = receiveDir || '(unknown)';
}

async function revealPath(path) {
  if (!path) return;
  try {
    await invoke('reveal_in_folder', { path });
  } catch (e) {
    toast(`Reveal failed: ${e}`, true);
  }
}

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
    toast(`Reconnecting to ${contactLabel(contact.id)}…`);
    const result = await invoke('connect_and_verify', { remoteQr: contact.qr });
    const peerId = pick(result, 'peer_id', 'peerId');
    const autoTrusted = result?.auto_trusted ?? result?.autoTrusted;
    const err = result?.error || (result.sas?.startsWith?.('error:') ? result.sas : null);
    if (err) throw new Error(String(err).replace(/^error:\s*/i, '').trim());
    if (!peerId) throw new Error('Reconnect failed — peer may be offline');
    migratePeerId(contact.id, peerId);
    addPeerUnique(peerId);
    connectedPeer = peerId;
    connectedPeers.add(peerId);
    peerStatus.set(peerId, 'online');
    const name = contact.name || shortPeer(peerId);
    const existing = savedContacts.findIndex(c => c.id === peerId);
    const entry = { id: peerId, name, verified: true, qr: contact.qr };
    if (existing >= 0) savedContacts[existing] = { ...savedContacts[existing], ...entry };
    else savedContacts.push(entry);
    persistContacts();
    await invoke('register_saved_peer', { peerId, qr: contact.qr }).catch(() => {});
    await syncTrustedPubkeys();
    selectPeer(peerId);
    if (autoTrusted) {
      peerVerified.set(peerId, true);
      await syncDisplayName(peerId);
      toast('Reconnected to trusted peer');
    } else {
      const sas = pick(result, 'sas', 'Sas');
      if (sas) showSasModal(peerId, sas);
      else toast('Connected — confirm SAS if prompted');
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

function closeChatWindow() {
  activePeer = null;
  document.getElementById('empty-state')?.classList.remove('hidden');
  document.getElementById('messages')?.replaceChildren();
  updateChatHeader();
  updateInputState();
  updateVerifyBanner();
}

function contactStatus(id) {
  if (connectedPeers.has(id)) return { text: '● online', cls: 'online' };
  if (peerStatus.get(id) === 'reconnecting') return { text: '↻ reconnecting', cls: 'reconnecting' };
  if (peerStatus.get(id) === 'paused') return { text: '⏸ disconnected by you', cls: 'paused' };
  const c = savedContacts.find(x => x.id === id);
  if (c?.verified) return { text: '○ offline', cls: 'offline' };
  return { text: 'unverified', cls: 'offline' };
}

async function syncDisplayName(broadcastTo) {
  if (!displayName) return;
  try {
    await invoke('set_display_name', { name: displayName });
    if (broadcastTo) await invoke('broadcast_profile', { peerId: broadcastTo });
  } catch (_) {}
}

function softDisconnect(id) {
  // End call on both sides when user disconnects
  if (activeCall && activeCall.peer === id) {
    onCallEndedLocal(true);
  }
  invoke('disconnect_peer', { peerId: id }).catch((e) => toast(`Disconnect: ${e}`, true));
  clearTransfersForPeer(id);
  peers = peers.filter(p => p !== id);
  connectedPeers.delete(id);
  if (connectedPeer === id) connectedPeer = null;
  peerStatus.set(id, 'paused');
  if (activePeer === id) closeChatWindow();
  renderPeers();
  renderPeerChips();
  renderContactsList();
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
    try {
      const pk = await invoke('get_public_key');
      const fp = document.getElementById('local-fingerprint');
      if (fp && pk) fp.textContent = pk.slice(0, 16) + '…' + pk.slice(-8);
    } catch (_) {}
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

function setupFirstRun() {
  const banner = document.getElementById('first-run-banner');
  const dismiss = document.getElementById('dismiss-first-run');
  if (!banner) return;
  if (!loadState().onboardingDone) {
    banner.classList.remove('hidden');
  }
  dismiss?.addEventListener('click', () => {
    saveState({ onboardingDone: true });
    banner.classList.add('hidden');
  });
}

async function init() {
  setupFirstRun();
  try {
    const notifEl = document.getElementById('setting-desktop-notifications');
    if (notifEl) notifEl.checked = loadState().desktopNotifications !== false;
    await refreshOwnQr();

    await refreshSerialPorts();
    // Populate audio device lists (after optional permission)
    refreshAudioDeviceSelects().catch(() => {});

    document.getElementById('display-name').value = displayName;
    if (displayName) await syncDisplayName(null);
    restoreContacts();
    await syncTrustedPubkeys();
    for (const c of savedContacts.filter(x => x.verified && x.qr)) {
      try { await invoke('register_saved_peer', { peerId: c.id, qr: c.qr }); } catch (_) {}
    }
    try {
      receiveDir = await invoke('get_receive_dir');
      updateReceiveDirUI();
    } catch (_) {}

    try {
      const hasCamera = await invoke('has_local_camera');
      window.SrltcpWebRTC?.setLocalCameraAvailable?.(hasCamera);
      const camEl = document.getElementById('call-setting-camera');
      if (camEl) {
        camEl.checked = !!hasCamera;
        camEl.disabled = !hasCamera;
        camEl.title = hasCamera ? '' : 'No camera detected — video calls use receive-only mode';
      }
      window.SrltcpWebRTC?.setCallSettings?.({
        mic: document.getElementById('call-setting-mic')?.checked ?? true,
        camera: !!hasCamera && (camEl?.checked ?? false),
      });
      window.SrltcpWebRTC?.setHasActiveCall?.(() => !!activeCallRef.current);
    } catch (_) {}

    const existingPeers = await invoke('get_peers');
    existingPeers.forEach(id => {
      if (id.startsWith('peer:')) {
        addPeerUnique(id);
        connectedPeers.add(id);
        peerStatus.set(id, 'online');
      }
    });
    connectedPeer = existingPeers.find(id => id.startsWith('peer:')) || null;
    const lastId = loadState().lastActivePeer;
    const resume = savedContacts.find(c => c.id === lastId && c.verified && c.qr)
      || savedContacts.find(c => c.verified && c.qr);
    if (resume && !connectedPeer) reconnectContact(resume);
    else if (resume && connectedPeer === resume.id) selectPeer(resume.id);
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
      notifyDesktop(
        `Message from ${contactLabel(pick(p, 'sender', 'Sender') || pick(p, 'peer_id', 'peerId'))}`,
        (p.content || '').slice(0, 120),
      );
      break;
    case 'peer_connected': {
      const id = pick(p, 'peer_id', 'peerId');
      if (id) {
        addPeerUnique(id);
        if (id.startsWith('peer:')) {
          connectedPeer = id;
          connectedPeers.add(id);
          peerStatus.set(id, 'online');
        }
      }
      renderPeers();
      renderPeerChips();
      renderContactsList();
      updateChatHeader();
      updateInputState();
      break;
    }
    case 'peer_disconnected': {
      const id = pick(p, 'peer_id', 'peerId');
      const reason = p.reason || '';
      // Peer drop must end local call UI/media (both sides)
      if (activeCall && activeCall.peer === id) {
        onCallEndedLocal(false);
      }
      connectedPeers.delete(id);
      peers = peers.filter(pId => pId !== id);
      if (connectedPeer === id) connectedPeer = null;
      if (reason === 'connection lost') {
        peerStatus.set(id, 'reconnecting');
      } else if (reason === 'user disconnected') {
        peerStatus.set(id, 'paused');
      } else {
        peerStatus.set(id, 'offline');
      }
      clearTransfersForPeer(id);
      if (activePeer === id && reason !== 'connection lost') closeChatWindow();
      renderPeers();
      renderPeerChips();
      renderContactsList();
      updateChatHeader();
      updateInputState();
      if (reason === 'connection lost') {
        const contact = savedContacts.find(c => c.id === id && c.verified && c.qr);
        if (contact) reconnectContact(contact);
      }
      break;
    }
    case 'peer_profile': {
      const id = pick(p, 'peer_id', 'peerId');
      const name = pick(p, 'display_name', 'displayName') || p.content || '';
      if (id && name) {
        remoteDisplayNames.set(id, name);
        const idx = savedContacts.findIndex(c => c.id === id);
        if (idx >= 0 && !savedContacts[idx].name) {
          savedContacts[idx] = { ...savedContacts[idx], name };
          persistContacts();
        }
        renderPeers();
        renderPeerChips();
        renderContactsList();
        updateChatHeader();
      }
      break;
    }
    case 'message_queued':
      toast(`Queued for ${shortPeer(pick(p, 'peer_id', 'peerId'))} — will send on reconnect`);
      break;
    case 'reconnecting': {
      const id = pick(p, 'peer_id', 'peerId');
      if (id) peerStatus.set(id, 'reconnecting');
      renderContactsList();
      toast(`Reconnecting to ${contactLabel(id)}…`);
      break;
    }
    case 'sas_ready': {
      const id = pick(p, 'peer_id', 'peerId');
      const sas = pick(p, 'sas', 'Sas');
      const autoTrusted = p.auto_trusted ?? p.autoTrusted;
      if (id && autoTrusted) {
        peerVerified.set(id, true);
        connectedPeer = id;
        connectedPeers.add(id);
        peerStatus.set(id, 'online');
        const qr = window._lastConnectQr
          || savedContacts.find(c => c.id === id)?.qr
          || '';
        const name = remoteDisplayNames.get(id)
          || savedContacts.find(c => c.id === id)?.name
          || shortPeer(id);
        const existing = savedContacts.findIndex(c => c.id === id);
        const entry = { id, name, verified: true, qr };
        if (existing >= 0) savedContacts[existing] = { ...savedContacts[existing], ...entry };
        else savedContacts.push(entry);
        persistContacts();
        if (qr) invoke('register_saved_peer', { peerId: id, qr }).catch(() => {});
        syncTrustedPubkeys();
        selectPeer(id);
        updateChatHeader();
        updateInputState();
        updateVerifyBanner();
        syncDisplayName(id);
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
    case 'transfer_progress': {
      const tid = pick(p, 'id', 'Id');
      const peerId = pick(p, 'peer_id', 'peerId');
      const existing = transfers.get(tid);
      const outgoing = existing?.outgoing ?? false;
      const totalBytes = Number(p.total_bytes ?? p.totalBytes ?? p.message ?? existing?.totalBytes ?? 0) || 0;
      updateTransfer(tid, p.filename, p.progress, outgoing, totalBytes, peerId);
      break;
    }
    case 'transfer_complete': {
      const tid = pick(p, 'id', 'Id');
      const peerId = pick(p, 'peer_id', 'peerId');
      const fname = p.filename || 'file';
      const fpath = p.path || p.message || '';
      const wasOutgoing = transfers.get(tid)?.outgoing ?? false;
      removeTransfer(tid);
      if (wasOutgoing) {
        toast(`Upload complete: ${fname}`);
        break;
      }
      const kind = mediaKind(fname);
      const sender = shortPeer(peerId);
      if (peerId && peerId !== activePeer) selectPeer(peerId);
      if ((kind === 'image' || kind === 'video') && fpath) {
        appendMessage(fname, 'received', sender, { kind, path: fpath });
      } else {
        appendMessage(`📁 ${fname}`, 'received', sender, { kind: 'file', path: fpath || null });
      }
      toast(`Download complete: ${fname}`);
      break;
    }
    case 'transfer_cancelled':
      removeTransfer(pick(p, 'id', 'Id'));
      toast('Transfer cancelled');
      break;
    case 'call_offer':
    case 'call_answer':
    case 'call_ice':
    case 'call_end':
      window.SrltcpWebRTC?.handleIncomingCallSignal(
        p, invoke, activeCallRef, contactLabel, onCallEndedLocal,
      )
        .then((c) => { if (c) { activeCall = c; activeCallRef.current = c; updateCallUI(); } })
        .catch((e) => toast(`Call error: ${e}`, true));
      break;
    case 'call_ended': {
      const endedPeer = pick(p, 'peer_id', 'peerId');
      const endedCallId = pick(p, 'call_id', 'callId');
      if (!activeCall
        || (endedCallId && activeCall.callId === endedCallId)
        || (endedPeer && activeCall.peer === endedPeer)
        || (!endedCallId && !endedPeer)) {
        onCallEndedLocal(false);
      }
      break;
    }
    case 'peer_qr_refresh': {
      const id = pick(p, 'peer_id', 'peerId');
      const qr = p.qr || p.content || '';
      if (id && qr) updateContactQr(id, qr);
      break;
    }
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
  if (remoteDisplayNames.has(id)) return remoteDisplayNames.get(id);
  const c = savedContacts.find(x => x.id === id);
  return c?.name || shortPeer(id);
}

async function refreshSerialPorts() {
  try {
    const ports = await invoke('list_serial_ports');
    const select = document.getElementById('serial-port');
    if (!select) return;
    select.innerHTML = ports.length === 0
      ? '<option value="">No serial devices detected — plug in and refresh</option>'
      : ports.map(p => {
          const path = String(p.path ?? p);
          const label = String(p.label ?? path);
          return `<option value="${escapeHtml(path)}">${escapeHtml(label)}</option>`;
        }).join('');
  } catch (e) {
    const select = document.getElementById('serial-port');
    if (select) select.innerHTML = `<option value="">Error listing ports: ${escapeHtml(String(e))}</option>`;
  }
}

function getOnlinePeerIds() {
  return [...connectedPeers];
}

function persistContacts() {
  saveState({
    displayName,
    contacts: savedContacts,
    chatHistory: chatHistory,
    lastActivePeer: activePeer || loadState().lastActivePeer || '',
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
  const wasActive = activePeer === id;
  invoke('disconnect_peer', { peerId: id }).catch(() => {});
  removePeer(id);
  savedContacts = savedContacts.filter(c => c.id !== id);
  delete chatHistory[id];
  delete messageStore[id];
  if (wasActive) closeChatWindow();
  if (loadState().lastActivePeer === id) saveState({ lastActivePeer: '' });
  persistContacts();
  renderPeers();
  renderPeerChips();
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
    const online = connectedPeers.has(c.id);
    const st = contactStatus(c.id);
    const label = contactLabel(c.id);
    return `
    <div class="contact-row">
      <button class="contact-select" data-peer="${escapeHtml(c.id)}">
        <span class="contact-name">${escapeHtml(label)}</span>
        <span class="contact-meta ${st.cls}">${st.text}</span>
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
  const onlineIds = getOnlinePeerIds();
  document.getElementById('peer-count').textContent = onlineIds.length;
  noPeers.classList.toggle('hidden', onlineIds.length > 0);
  list.innerHTML = onlineIds.map(id => {
    const verified = peerVerified.get(id);
    const badge = verified ? '<span class="verified-badge">✓</span>' : '<span class="unverified-badge">!</span>';
    const st = peerStatus.get(id) === 'reconnecting' ? ' reconnecting' : ' online';
    return `<li class="${id === activePeer ? 'active' : ''}${verified ? ' verified' : ''}${st}" data-peer="${escapeHtml(id)}">
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
  const chipIds = savedContacts.length > 0
    ? savedContacts.map(c => c.id)
    : getOnlinePeerIds();
  if (chipIds.length === 0) { bar.classList.add('hidden'); return; }
  bar.classList.remove('hidden');
  bar.innerHTML = chipIds.map(id => {
    const v = peerVerified.get(id) ? ' ✓' : '';
    return `<button class="chip${id === activePeer ? ' active' : ''}${peerVerified.get(id) ? ' verified' : ''}" data-peer="${id}">${escapeHtml(contactLabel(id))}${v}</button>`;
  }).join('');
  bar.querySelectorAll('.chip').forEach(c => {
    c.onclick = () => selectPeer(c.dataset.peer);
  });
}

function focusChatPanel() {
  document.querySelector('.nav-btn[data-panel="peers"]')?.click();
}

function clearTransfersForPeer(peerId) {
  for (const [id, t] of [...transfers.entries()]) {
    if (t.peerId === peerId) transfers.delete(id);
  }
  renderTransfers();
}

function selectPeer(id) {
  activePeer = id;
  persistContacts();
  focusChatPanel();
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
    const st = contactStatus(activePeer);
    title.textContent = contactLabel(activePeer);
    const secure = verified ? '✓ Verified — end-to-end encrypted' : '⚠ Not verified — run SAS check';
    sub.textContent = `${st.text} · ${secure}`;
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
  const online = activePeer && connectedPeers.has(activePeer);
  const verified = activePeer && peerVerified.get(activePeer);
  const inCall = !!activeCall;
  const canChat = hasPeer && verified && online && !inCall;
  ['message-input', 'send-btn', 'send-file-btn'].forEach(id => {
    document.getElementById(id).disabled = !canChat;
  });
  document.getElementById('voice-call-btn').disabled = !hasPeer || !verified || inCall || !online;
  document.getElementById('video-call-btn').disabled = !hasPeer || !verified || inCall || !online;
  document.getElementById('disconnect-btn').disabled = !hasPeer || !online;
}

function updateCallUI() {
  const bar = document.getElementById('call-status');
  const endBtn = document.getElementById('end-call-btn');
  if (activeCall) {
    const kind = activeCall.video ? 'Video' : 'Voice';
    bar.innerHTML = `<span class="call-pulse"></span> ${kind} call — ${escapeHtml(contactLabel(activeCall.peer))}`;
    bar.classList.remove('hidden');
    endBtn.classList.remove('hidden');
  } else {
    bar.classList.add('hidden');
    endBtn.classList.add('hidden');
  }
  updateInputState();
}

function updateContactQr(peerId, qr) {
  // Only accept ticket refresh when Ed25519 peer id still matches (engine also enforces).
  if (peerId && qr && !peerId.startsWith('peer:')) return;
  const idx = savedContacts.findIndex(c => c.id === peerId);
  if (idx >= 0) {
    savedContacts[idx] = { ...savedContacts[idx], qr };
    persistContacts();
    invoke('register_saved_peer', { peerId, qr }).catch(() => {});
  }
}

async function onCallEndedLocal(notifyRemote = false) {
  const call = activeCall;
  activeCall = null;
  activeCallRef.current = null;
  window.SrltcpWebRTC?.cleanupCall();
  updateCallUI();
  if (notifyRemote && call) {
    try {
      await invoke('end_call', { peerId: call.peer, callId: call.callId });
    } catch (_) {}
  }
}

async function endActiveCall() {
  if (!activeCall) {
    onCallEndedLocal();
    return;
  }
  try {
    await invoke('end_call', { peerId: activeCall.peer, callId: activeCall.callId });
    onCallEndedLocal();
    toast('Call ended');
  } catch (e) { toast(`End call error: ${e}`, true); }
}

function buildVideoPlayer(path, filename) {
  const wrap = document.createElement('div');
  wrap.className = 'msg-video-wrap';

  const vid = document.createElement('video');
  vid.className = 'msg-media';
  vid.controls = true;
  vid.playsInline = true;
  vid.preload = 'metadata';
  vid.src = convertFileSrc(path);

  const toolbar = document.createElement('div');
  toolbar.className = 'msg-video-toolbar';

  const playBtn = document.createElement('button');
  playBtn.type = 'button';
  playBtn.className = 'btn-sm';
  playBtn.textContent = '▶ Play';
  playBtn.onclick = () => { vid.play().catch(() => toast('Playback failed', true)); };

  const pauseBtn = document.createElement('button');
  pauseBtn.type = 'button';
  pauseBtn.className = 'btn-sm';
  pauseBtn.textContent = '⏸ Pause';
  pauseBtn.onclick = () => vid.pause();

  const openBtn = document.createElement('button');
  openBtn.type = 'button';
  openBtn.className = 'btn-sm';
  openBtn.textContent = '↗ Open';
  openBtn.onclick = async () => {
    try {
      await window.__TAURI__?.opener?.open(path)
        ?? window.__TAURI__?.shell?.open(path);
    } catch (_) {
      toast(`Open file: ${path}`);
    }
  };

  vid.onerror = () => toast(`Video playback failed: ${filename}`, true);

  toolbar.append(playBtn, pauseBtn, openBtn);
  wrap.append(vid, toolbar);
  return wrap;
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
    img.onerror = () => {
      img.replaceWith(Object.assign(document.createElement('div'), {
        className: 'msg-text',
        textContent: `🖼 ${content} (preview blocked — use Open location)`,
      }));
    };
    div.appendChild(img);
  } else if (opts.kind === 'video' && opts.path) {
    div.appendChild(buildVideoPlayer(opts.path, content));
  } else {
    const text = document.createElement('div');
    text.className = 'msg-text';
    text.textContent = content;
    div.appendChild(text);
    if (opts.kind === 'file' && opts.path) {
      const openBtn = document.createElement('button');
      openBtn.type = 'button';
      openBtn.className = 'btn-sm msg-open-btn';
      openBtn.textContent = 'Open location';
      openBtn.onclick = () => revealPath(opts.path);
      div.appendChild(openBtn);
    }
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

function updateTransfer(id, filename, progress, outgoing, totalBytes, peerId = null) {
  const existing = transfers.get(id);
  const now = Date.now();
  const bytes = totalBytes || existing?.totalBytes || 0;
  let speedBps = existing?.speedBps || 0;
  if (existing && bytes > 0 && now > existing.lastUpdateMs) {
    const delta = Math.max(0, progress - existing.lastProgress);
    const dt = (now - existing.lastUpdateMs) / 1000;
    if (dt > 0.05) speedBps = (delta * bytes) / dt;
  }
  transfers.set(id, {
    filename,
    progress,
    outgoing,
    totalBytes: bytes,
    speedBps,
    lastProgress: progress,
    lastUpdateMs: now,
    peerId: peerId || existing?.peerId || activePeer,
  });
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
    <div class="transfer-item" data-transfer="${escapeHtml(id)}">
      <span>${t.outgoing ? '↑' : '↓'} ${escapeHtml(t.filename)} — ${Math.round(t.progress * 100)}%${formatSpeed(t.speedBps || 0)}</span>
      <div class="progress-track"><div class="progress-fill" style="width:${Math.round(t.progress * 100)}%"></div></div>
      ${t.outgoing ? `<button class="btn-sm transfer-cancel" data-cancel="${escapeHtml(id)}">Cancel</button>` : ''}
    </div>`).join('');
  panel.querySelectorAll('.transfer-cancel').forEach(btn => {
    btn.onclick = async () => {
      try {
        await invoke('cancel_transfer', { transferId: btn.dataset.cancel });
        removeTransfer(btn.dataset.cancel);
      } catch (e) { toast(`Cancel failed: ${e}`, true); }
    };
  });
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
      connectedPeers.add(peerId);
      peerStatus.set(peerId, 'online');
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
      if (qr) await invoke('register_saved_peer', { peerId, qr });
      await syncDisplayName(peerId);
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
    connectedPeers.add(pendingSas.peerId);
    peerStatus.set(pendingSas.peerId, 'online');
    const name = displayName || shortPeer(pendingSas.peerId);
    const existing = savedContacts.findIndex(c => c.id === pendingSas.peerId);
    const qr = window._lastConnectQr || '';
    const entry = { id: pendingSas.peerId, name, verified: true, qr };
    if (existing >= 0) savedContacts[existing] = entry;
    else savedContacts.push(entry);
    persistContacts();
    await syncTrustedPubkeys();
    if (qr) await invoke('register_saved_peer', { peerId: pendingSas.peerId, qr });
    await syncDisplayName(pendingSas.peerId);
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

document.getElementById('save-display-name')?.addEventListener('click', async () => {
  displayName = document.getElementById('display-name').value.trim();
  persistContacts();
  await syncDisplayName(connectedPeer);
  toast('Display name saved');
});

document.getElementById('check-updates')?.addEventListener('click', () => {
  toast('Run: git pull && ./run.sh  —  Releases: github.com/narl3yyy-svg/SRLTCPv2');
});

document.getElementById('open-receive-dir')?.addEventListener('click', async () => {
  if (!receiveDir) {
    try { receiveDir = await invoke('get_receive_dir'); updateReceiveDirUI(); } catch (_) {}
  }
  if (receiveDir) await revealPath(receiveDir);
  else toast('Receive folder not ready', true);
});

document.getElementById('copy-receive-dir')?.addEventListener('click', async () => {
  if (!receiveDir) {
    try { receiveDir = await invoke('get_receive_dir'); updateReceiveDirUI(); } catch (_) {}
  }
  if (receiveDir) {
    await navigator.clipboard.writeText(receiveDir);
    toast('Save folder path copied');
  } else {
    toast('Receive folder not ready', true);
  }
});

document.getElementById('test-media-perms')?.addEventListener('click', async () => {
  const hint = document.getElementById('media-status-hint');
  try {
    toast('Requesting mic/camera access…');
    const msg = await window.SrltcpWebRTC?.testMediaPermissions?.();
    if (hint) hint.textContent = msg || 'Media test complete';
    toast(msg || 'Media test complete');
    await refreshAudioDeviceSelects();
  } catch (e) {
    if (hint) hint.textContent = String(e);
    toast(`Media test failed: ${e}`, true);
  }
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
    connectedPeers.clear();
    list.forEach(id => {
      if (id.startsWith('peer:')) {
        addPeerUnique(id);
        connectedPeers.add(id);
        peerStatus.set(id, 'online');
      }
    });
    connectedPeer = list.find(id => id.startsWith('peer:')) || null;
    renderPeers();
    renderPeerChips();
    renderContactsList();
    toast(`Peers refreshed (${connectedPeers.size} online)`);
  } catch (e) { toast(`Refresh error: ${e}`, true); }
};

document.getElementById('refresh-serial')?.addEventListener('click', async () => {
  await refreshSerialPorts();
  toast('Serial devices refreshed');
});

async function refreshAudioDeviceSelects() {
  const inputs = document.getElementById('audio-input-select');
  const outputs = document.getElementById('audio-output-select');
  if (!inputs || !outputs) return;
  const savedIn = loadState().audioInputId || '';
  const savedOut = loadState().audioOutputId || '';
  try {
    const { inputs: ins, outputs: outs } = await window.SrltcpWebRTC.listAudioDevices();
    inputs.innerHTML = '<option value="">System default</option>'
      + ins.map((d) => `<option value="${d.id}">${escapeHtml(d.label)}</option>`).join('');
    outputs.innerHTML = '<option value="">System default</option>'
      + outs.map((d) => `<option value="${d.id}">${escapeHtml(d.label)}</option>`).join('');
    if (savedIn) inputs.value = savedIn;
    if (savedOut) outputs.value = savedOut;
    window.SrltcpWebRTC?.setCallSettings?.({
      audioInputId: inputs.value,
      audioOutputId: outputs.value,
    });
  } catch (_) {}
}

document.getElementById('call-setting-mic')?.addEventListener('change', (e) => {
  window.SrltcpWebRTC?.setCallSettings({ mic: e.target.checked });
});
document.getElementById('call-setting-camera')?.addEventListener('change', (e) => {
  window.SrltcpWebRTC?.setCallSettings({ camera: e.target.checked });
});
document.getElementById('audio-input-select')?.addEventListener('change', (e) => {
  saveState({ audioInputId: e.target.value });
  window.SrltcpWebRTC?.setCallSettings({ audioInputId: e.target.value });
});
document.getElementById('audio-output-select')?.addEventListener('change', (e) => {
  saveState({ audioOutputId: e.target.value });
  window.SrltcpWebRTC?.setCallSettings({ audioOutputId: e.target.value });
});
document.getElementById('refresh-audio-devices')?.addEventListener('click', async () => {
  await refreshAudioDeviceSelects();
  toast('Audio devices refreshed');
});
document.getElementById('setting-desktop-notifications')?.addEventListener('change', (e) => {
  saveState({ desktopNotifications: e.target.checked });
});
document.getElementById('request-notification-perm')?.addEventListener('click', async () => {
  try {
    if (typeof Notification === 'undefined') {
      toast('Notifications not available in this webview', true);
      return;
    }
    const p = await Notification.requestPermission();
    saveState({ desktopNotifications: p === 'granted' });
    const el = document.getElementById('setting-desktop-notifications');
    if (el) el.checked = p === 'granted';
    toast(p === 'granted' ? 'Notifications enabled' : `Permission: ${p}`);
  } catch (e) {
    toast(`Notification permission failed: ${e}`, true);
  }
});

document.getElementById('disconnect-btn').onclick = async () => {
  if (!activePeer) return;
  softDisconnect(activePeer);
};

document.getElementById('voice-call-btn').onclick = async () => {
  if (!activePeer || !connectedPeers.has(activePeer)) return;
  try {
    activeCall = await window.SrltcpWebRTC.startOutgoingCall(
      activePeer, false, invoke, contactLabel(activePeer), onCallEndedLocal,
    );
    activeCallRef.current = activeCall;
    updateCallUI();
    toast('Voice call started');
  } catch (e) { toast(`Voice call failed: ${e}`, true); }
};

document.getElementById('video-call-btn').onclick = async () => {
  if (!activePeer || !connectedPeers.has(activePeer)) return;
  try {
    activeCall = await window.SrltcpWebRTC.startOutgoingCall(
      activePeer, true, invoke, contactLabel(activePeer), onCallEndedLocal,
    );
    activeCallRef.current = activeCall;
    updateCallUI();
    toast('Video call started');
  } catch (e) { toast(`Video call failed: ${e}`, true); }
};

document.getElementById('end-call-btn').onclick = () => endActiveCall();
document.getElementById('call-end-overlay-btn')?.addEventListener('click', () => endActiveCall());

document.getElementById('incoming-call-answer')?.addEventListener('click', async () => {
  try {
    activeCall = await window.SrltcpWebRTC.answerIncomingCall(invoke, onCallEndedLocal);
    activeCallRef.current = activeCall;
    updateCallUI();
    toast('Call connected');
  } catch (e) { toast(`Answer failed: ${e}`, true); }
});

document.getElementById('incoming-call-decline')?.addEventListener('click', async () => {
  try {
    await window.SrltcpWebRTC.declineIncomingCall(invoke);
    toast('Call declined');
  } catch (e) { toast(`Decline error: ${e}`, true); }
});

document.getElementById('call-mute-btn')?.addEventListener('click', () => {
  const on = window.SrltcpWebRTC?.toggleMute();
  document.getElementById('call-mute-btn')?.classList.toggle('muted', on === false);
});

document.getElementById('call-camera-btn')?.addEventListener('click', () => {
  const on = window.SrltcpWebRTC?.toggleCamera();
  document.getElementById('call-camera-btn')?.classList.toggle('muted', on === false);
});

document.getElementById('send-file-btn').onclick = async () => {
  if (!activePeer || !peerVerified.get(activePeer)) return;
  const offline = activePeer && !connectedPeers.has(activePeer);
  let filePath;
  try { filePath = await openFileDialog({ multiple: false }); } catch (_) {}
  if (!filePath) return;
  try {
    let totalBytes = 0;
    try { totalBytes = await invoke('file_size', { path: filePath }); } catch (_) {}
    const result = await invoke('send_file', { peerId: activePeer, filePath });
    const kind = mediaKind(result.filename);
    if (result.transfer_id) {
      updateTransfer(result.transfer_id, result.filename, result.progress || 0, true, totalBytes);
    }
    if (kind === 'image' || kind === 'video') {
      appendMessage(result.filename, 'sent', 'You', { kind, path: filePath });
    } else if (result.filename?.startsWith('queued:')) {
      toast('File queued — reconnecting…');
    } else {
      appendMessage(`📤 ${result.filename}`, 'sent', 'You', { kind: 'file', path: filePath });
    }
    if (offline) toast('File queued — reconnecting…');
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
  const offline = activePeer && !connectedPeers.has(activePeer);
  try {
    await invoke('send_message', { peerId: activePeer, content });
    appendMessage(content, 'sent', 'You');
    input.value = '';
    if (offline) toast('Message queued — reconnecting…');
  } catch (e) { toast(`Send error: ${e}`, true); }
}

init();