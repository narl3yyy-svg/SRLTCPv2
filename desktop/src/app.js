// SRLTCP v0.2.0 Desktop Frontend

const invoke = window.__TAURI__?.core?.invoke
  ?? (async (cmd, args) => {
      console.log(`[mock] ${cmd}`, args);
      return null;
    });

const listen = window.__TAURI__?.event?.listen
  ?? (async () => () => {});

const openFileDialog = window.__TAURI__?.dialog?.open
  ?? (async () => null);

let activePeer = null;
let peers = [];

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

    document.getElementById('status').textContent = 'Online';
  } catch (e) {
    console.error('Init failed:', e);
    document.getElementById('status').textContent = 'Offline';
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
      break;
    case 'peer_disconnected':
      removePeer(payload.peer_id);
      break;
    case 'sas_ready':
      showSas(payload.sas);
      break;
    case 'transfer_progress':
      showTransferStatus(`${payload.filename}: ${Math.round(payload.progress * 100)}%`);
      break;
    case 'transfer_complete':
      showTransferStatus(`Transfer complete: ${payload.filename}`);
      appendMessage(`📁 File sent: ${payload.filename}`, 'sent', 'System');
      break;
    case 'started':
      document.getElementById('status').textContent = 'Online';
      break;
    case 'stopped':
      document.getElementById('status').textContent = 'Offline';
      break;
    case 'error':
      console.error('Engine error:', payload.message);
      break;
  }
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
  list.innerHTML = '';
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
}

function updateChatHeader() {
  document.getElementById('chat-title').textContent =
    activePeer ? `Chat with ${activePeer}` : 'Select or connect a peer';
}

function updateInputState() {
  const enabled = !!activePeer;
  document.getElementById('message-input').disabled = !enabled;
  document.getElementById('send-btn').disabled = !enabled;
  document.getElementById('file-input').disabled = !enabled;
  document.getElementById('send-file-btn').disabled = !enabled;
}

function appendMessage(content, direction, sender) {
  const div = document.createElement('div');
  div.className = `message ${direction}`;
  div.innerHTML = `${escapeHtml(content)}<div class="meta">${sender || ''}</div>`;
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

function showTransferStatus(text) {
  const el = document.getElementById('transfer-status');
  el.textContent = text;
  el.classList.remove('hidden');
}

document.getElementById('copy-qr').onclick = async () => {
  const qr = document.getElementById('qr-payload').textContent;
  await navigator.clipboard.writeText(qr);
};

document.getElementById('connect-serial').onclick = async () => {
  const port = document.getElementById('serial-port').value;
  if (!port) return;
  await invoke('connect_serial', { portName: port, baudRate: 115200 });
};

document.getElementById('connect-quic').onclick = async () => {
  const addr = document.getElementById('quic-addr').value;
  if (!addr) return;
  await invoke('connect_quic', { addr });
};

document.getElementById('verify-peer').onclick = async () => {
  const qr = document.getElementById('remote-qr').value;
  if (!qr || !activePeer) return;
  const sas = await invoke('handshake', { peerId: activePeer, remoteQr: qr });
  showSas(sas);
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
    console.warn('Dialog open failed, falling back to file input:', e);
  }

  if (!filePath) {
    const fileInput = document.getElementById('file-input');
    const file = fileInput.files[0];
    if (!file) return;
    filePath = file.path || file.name;
  }

  try {
    const result = await invoke('send_file', { peerId: activePeer, filePath });
    showTransferStatus(`Sending ${result.filename}…`);
    appendMessage(`📁 Sending: ${result.filename}`, 'sent', 'You');
  } catch (e) {
    console.error('File send failed:', e);
    showTransferStatus(`Error: ${e}`);
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
    console.error('Send failed:', e);
  }
}

init();