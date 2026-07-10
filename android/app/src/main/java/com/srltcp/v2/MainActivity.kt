package com.srltcp.v2

import com.srltcp.v2.webrtc.WebRtcCallManagerHolder
import android.Manifest
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Bundle
import android.webkit.MimeTypeMap
import android.widget.MediaController
import android.widget.VideoView
import androidx.core.content.ContextCompat
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import android.graphics.BitmapFactory
import android.util.Base64
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import coil.compose.AsyncImage
import org.webrtc.SurfaceViewRenderer
import com.srltcp.v2.data.AppPreferences
import com.srltcp.v2.data.SavedContact
import com.srltcp.v2.service.SrltcpForegroundService
import com.srltcp.v2.ui.PeersSheet
import com.srltcp.v2.ui.SettingsSheet
import com.srltcp.v2.ui.theme.SRLTCPTheme
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.srltcp_core.SrltcpEvent
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.UUID

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        startP2pService()
        setContent {
            SRLTCPTheme {
                ChatScreen()
            }
        }
    }

    private fun startP2pService() {
        val intent = android.content.Intent(this, SrltcpForegroundService::class.java).apply {
            action = SrltcpForegroundService.ACTION_START
        }
        startForegroundService(intent)
    }

    override fun onDestroy() {
        super.onDestroy()
    }
}

enum class MessageKind { TEXT, IMAGE, VIDEO, FILE }

data class ChatMessage(
    val id: String = UUID.randomUUID().toString(),
    val content: String,
    val isSent: Boolean,
    val sender: String = "",
    val kind: MessageKind = MessageKind.TEXT,
    val mediaPath: String? = null,
)

data class TransferState(
    val id: String,
    val filename: String,
    val progress: Float,
    val isOutgoing: Boolean,
    val isComplete: Boolean = false,
    val totalBytes: Long = 0L,
    val speedBps: Double = 0.0,
    val lastProgress: Float = 0f,
    val lastUpdateMs: Long = 0L,
)

data class CallState(
    val callId: String,
    val peerId: String,
    val isVideo: Boolean,
)

data class PendingCall(
    val peerId: String,
    val callId: String,
    val sdp: String,
    val isVideo: Boolean,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatScreen() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var messages by remember { mutableStateOf(listOf<ChatMessage>()) }
    var inputText by remember { mutableStateOf("") }
    var activePeer by remember { mutableStateOf<String?>(null) }
    var qrPayload by remember { mutableStateOf("") }
    var qrImageDataUrl by remember { mutableStateOf("") }
    val peers = remember { mutableStateListOf<String>() }
    var engineOnline by remember { mutableStateOf(false) }
    var engineReady by remember { mutableStateOf(false) }
    var receiveDirPath by remember { mutableStateOf("") }
    val transfers = remember { mutableStateMapOf<String, TransferState>() }
    var callState by remember { mutableStateOf<CallState?>(null) }
    var snackbarMessage by remember { mutableStateOf<String?>(null) }
    var showConnectSheet by remember { mutableStateOf(false) }
    var showSasDialog by remember { mutableStateOf(false) }
    var sasCode by remember { mutableStateOf("") }
    var sasPeerId by remember { mutableStateOf<String?>(null) }
    var remoteQrInput by remember { mutableStateOf("") }
    var showSettingsSheet by remember { mutableStateOf(false) }
    var showPeersSheet by remember { mutableStateOf(false) }
    var displayName by remember { mutableStateOf("") }
    var showIncomingCallDialog by remember { mutableStateOf(false) }
    var pendingIncomingCall by remember { mutableStateOf<PendingCall?>(null) }

    val savedContacts = remember { mutableStateListOf<SavedContact>() }
    val prefs = remember { AppPreferences(context) }
    val peerVerified = remember { mutableStateMapOf<String, Boolean>() }
    val peerStatus = remember { mutableStateMapOf<String, String>() }
    val remoteDisplayNames = remember { mutableStateMapOf<String, String>() }
    var connectedPeer by remember { mutableStateOf<String?>(null) }
    val listState = rememberLazyListState()
    var pendingCallAction by remember { mutableStateOf<(() -> Unit)?>(null) }

    fun showSnackbar(msg: String) { snackbarMessage = msg }

    val callPermissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { results ->
        if (results.values.all { it }) {
            pendingCallAction?.invoke()
        } else {
            showSnackbar("Microphone/camera permission required for calls")
        }
        pendingCallAction = null
    }

    fun ensureCallPermissions(video: Boolean, onGranted: () -> Unit) {
        val needed = buildList {
            add(Manifest.permission.RECORD_AUDIO)
            if (video) add(Manifest.permission.CAMERA)
        }
        val missing = needed.filter {
            ContextCompat.checkSelfPermission(context, it) != PackageManager.PERMISSION_GRANTED
        }
        if (missing.isEmpty()) {
            onGranted()
        } else {
            pendingCallAction = onGranted
            callPermissionLauncher.launch(missing.toTypedArray())
        }
    }

    fun endActiveCall() {
        val call = callState ?: return
        scope.launch(Dispatchers.IO) {
            SrltcpEngineHolder.getOrCreate().endCall(call.peerId, call.callId)
            WebRtcCallManagerHolder.end()
            withContext(Dispatchers.Main) { callState = null }
        }
    }

    fun startOutgoingCall(peer: String, video: Boolean) {
        ensureCallPermissions(video) {
            scope.launch {
                try {
                    WebRtcCallManagerHolder.startOutgoing(context, peer, video) { callState = it }
                } catch (e: Exception) {
                    showSnackbar("Call failed: ${e.message ?: e}")
                }
            }
        }
    }

    fun contactLabel(peerId: String): String {
        remoteDisplayNames[peerId]?.takeIf { it.isNotBlank() }?.let { return it }
        savedContacts.find { it.peerId == peerId }?.displayName?.takeIf { it.isNotBlank() }?.let { return it }
        return peerId.removePrefix("peer:").take(12)
    }

    fun syncDisplayName(broadcastTo: String? = null) {
        if (displayName.isBlank()) return
        scope.launch(Dispatchers.IO) {
            val engine = SrltcpEngineHolder.getOrCreate()
            engine.setDisplayName(displayName)
            broadcastTo?.let { engine.broadcastProfile(it) }
        }
    }

    val reconcilePeers: () -> Unit = {
        val canonical = savedContacts.map { it.peerId }.toSet()
        peers.removeAll { id ->
            (id.startsWith("quic:") || id.startsWith("iroh:")) && canonical.any { c -> c != id }
        }
        val seen = mutableSetOf<String>()
        peers.removeAll { !seen.add(it) }
    }

    val addPeerUnique: (String) -> Unit = { id ->
        reconcilePeers()
        if (!peers.contains(id)) peers.add(id)
    }

    val migratePeerId: (String, String) -> Unit = { oldId, newId ->
        if (oldId != newId) {
            val idx = peers.indexOf(oldId)
            if (idx >= 0) peers[idx] = newId else addPeerUnique(newId)
            peers.remove(oldId)
            peerVerified[newId] = peerVerified.remove(oldId) ?: peerVerified[newId] ?: false
            if (activePeer == oldId) activePeer = newId
            if (connectedPeer == oldId) connectedPeer = newId
            val history = prefs.loadChatHistory(oldId)
            if (history != "[]") {
                prefs.saveChatHistory(newId, history)
                prefs.removeChatHistory(oldId)
            }
            val cIdx = savedContacts.indexOfFirst { it.peerId == oldId }
            if (cIdx >= 0) {
                val c = savedContacts[cIdx]
                savedContacts[cIdx] = c.copy(peerId = newId)
                prefs.upsertContact(savedContacts[cIdx])
            }
            reconcilePeers()
        }
    }

    fun refreshConnectedPeer(engine: uniffi.srltcp_core.SrltcpEngine? = SrltcpEngineHolder.engineOrNull()) {
        val eng = engine ?: SrltcpEngineHolder.engineOrNull() ?: return
        val list = eng.connectedPeers()
        connectedPeer = list.firstOrNull { it.startsWith("peer:") } ?: list.firstOrNull()
    }

    fun persistMessages(peerId: String, msgs: List<ChatMessage>) {
        val arr = JSONArray()
        msgs.forEach { m ->
            arr.put(
                JSONObject()
                    .put("id", m.id)
                    .put("content", m.content)
                    .put("isSent", m.isSent)
                    .put("sender", m.sender)
                    .put("kind", m.kind.name)
                    .put("mediaPath", m.mediaPath ?: ""),
            )
        }
        prefs.saveChatHistory(peerId, arr.toString())
    }

    fun loadMessagesForPeer(peerId: String): List<ChatMessage> {
        return try {
            val arr = JSONArray(prefs.loadChatHistory(peerId))
            buildList {
                for (i in 0 until arr.length()) {
                    val o = arr.getJSONObject(i)
                    add(
                        ChatMessage(
                            id = o.optString("id", UUID.randomUUID().toString()),
                            content = o.optString("content", ""),
                            isSent = o.optBoolean("isSent", false),
                            sender = o.optString("sender", ""),
                            kind = runCatching { MessageKind.valueOf(o.optString("kind", "TEXT")) }
                                .getOrDefault(MessageKind.TEXT),
                            mediaPath = o.optString("mediaPath").ifBlank { null },
                        ),
                    )
                }
            }
        } catch (_: Exception) {
            emptyList()
        }
    }

    fun softDisconnect(peerId: String) {
        scope.launch(Dispatchers.IO) {
            SrltcpEngineHolder.getOrCreate().disconnectPeer(peerId)
        }
        transfers.clear()
        peers.remove(peerId)
        if (connectedPeer == peerId) connectedPeer = null
        peerStatus[peerId] = "paused"
        if (activePeer == peerId) {
            activePeer = null
            messages = emptyList()
        }
        showSnackbar("Disconnected — contact saved, tap Reconnect to chat again")
    }

    fun reconnectContact(contact: SavedContact, openChat: Boolean = true) {
        if (contact.qrPayload.isBlank()) {
            showSnackbar("No QR saved for this contact — connect via QR again")
            return
        }
        scope.launch(Dispatchers.IO) {
            val engine = SrltcpEngineHolder.getOrCreate()
            engine.waitUntilReady(30u)
            val result = engine.connectAndVerify(contact.qrPayload)
            withContext(Dispatchers.Main) {
                val err = result.error
                    ?: result.sas.takeIf { it.startsWith("error:") }?.removePrefix("error: ")?.trim()
                if (!err.isNullOrBlank()) {
                    showSnackbar(err)
                    return@withContext
                }
                if (result.peerId.isBlank()) {
                    showSnackbar("Reconnect failed — peer may be offline")
                    return@withContext
                }
                migratePeerId(contact.peerId, result.peerId)
                addPeerUnique(result.peerId)
                engine.registerSavedPeer(result.peerId, contact.qrPayload)
                if (result.autoTrusted) {
                    peerVerified[result.peerId] = true
                }
                connectedPeer = result.peerId
                peerStatus[result.peerId] = "online"
                if (openChat) {
                    activePeer = result.peerId
                    messages = loadMessagesForPeer(result.peerId)
                }
                syncDisplayName(result.peerId)
                if (!result.autoTrusted && result.sas.isNotBlank()) {
                    sasCode = result.sas
                    sasPeerId = result.peerId
                    showSasDialog = true
                } else {
                    showSnackbar("Connected to ${contact.displayName.ifBlank { "peer" }}")
                }
            }
        }
    }

    fun sendCurrentMessage() {
        val peer = activePeer ?: return
        if (peerVerified[peer] != true) {
            showSnackbar("Verify peer with SAS first")
            return
        }
        val offline = connectedPeer != peer
        val text = inputText.trim()
        if (text.isEmpty()) return
        val sender = displayName.ifBlank { "You" }
        messages = messages + ChatMessage(content = text, isSent = true, sender = sender)
        inputText = ""
        if (offline) showSnackbar("Message queued — reconnecting…")
        scope.launch(Dispatchers.IO) {
            runCatching { SrltcpEngineHolder.getOrCreate().sendMessage(peer, text) }
                .onFailure { withContext(Dispatchers.Main) { showSnackbar("Send failed: ${it.message}") } }
        }
    }

    fun removeContact(peerId: String) {
        scope.launch(Dispatchers.IO) {
            SrltcpEngineHolder.getOrCreate().disconnectPeer(peerId)
        }
        peers.remove(peerId)
        peerVerified.remove(peerId)
        savedContacts.removeAll { it.peerId == peerId }
        prefs.removeContact(peerId)
        prefs.removeChatHistory(peerId)
        if (activePeer == peerId) {
            activePeer = null
            messages = emptyList()
        }
        if (connectedPeer == peerId) connectedPeer = null
        showSnackbar("Contact removed")
    }

    fun syncTrustedPubkeys(engine: uniffi.srltcp_core.SrltcpEngine) {
        val pubkeys = savedContacts
            .filter { it.verified && it.peerId.startsWith("peer:") }
            .map { it.peerId.removePrefix("peer:").lowercase() }
        if (pubkeys.isNotEmpty()) engine.loadTrustedPubkeys(pubkeys)
    }

    LaunchedEffect(Unit) {
        displayName = prefs.displayName
        val recvDir = File(context.filesDir, "received").apply { mkdirs() }
        receiveDirPath = recvDir.absolutePath
        try {
            val engine = withContext(Dispatchers.IO) {
                SrltcpEngineHolder.awaitEngine()
            }
            engine.setReceiveDir(recvDir.absolutePath)
            engineOnline = engine.isRunning()
            savedContacts.clear()
            savedContacts.addAll(prefs.loadContacts())
            prefs.loadContacts().forEach { c ->
                if (!peers.contains(c.peerId)) peers.add(c.peerId)
                peerVerified[c.peerId] = c.verified
            }
            engineReady = true
            withContext(Dispatchers.IO) {
                qrPayload = engine.qrPayload()
                qrImageDataUrl = engine.qrImageDataUrl()
                syncTrustedPubkeys(engine)
                savedContacts.filter { it.verified && it.qrPayload.isNotBlank() }.forEach { c ->
                    engine.registerSavedPeer(c.peerId, c.qrPayload)
                }
                syncDisplayName(null)
                reconcilePeers()
                refreshConnectedPeer(engine)
            }
            connectedPeer?.let { peerStatus[it] = "online" }
        } catch (e: Exception) {
            showSnackbar("Engine failed to start: ${e.message ?: e}")
        }
    }

    LaunchedEffect(activePeer) {
        activePeer?.let { messages = loadMessagesForPeer(it) }
    }

    LaunchedEffect(messages, activePeer) {
        activePeer?.let { if (messages.isNotEmpty()) persistMessages(it, messages) }
    }

    LaunchedEffect(showConnectSheet) {
        if (!showConnectSheet) remoteQrInput = ""
    }

    fun addMediaMessage(path: String, filename: String, isSent: Boolean, sender: String) {
        val kind = mediaKindForPath(path, filename)
        messages = messages + ChatMessage(
            content = filename,
            isSent = isSent,
            sender = sender,
            kind = kind,
            mediaPath = path,
        )
    }

    val eventListener: (SrltcpEvent) -> Unit = { event ->
        when (event.eventType) {
            "started" -> {
                engineOnline = true
                scope.launch(Dispatchers.IO) {
                    val eng = SrltcpEngineHolder.getOrCreate()
                    eng.waitUntilReady(30u)
                    val payload = eng.qrPayload()
                    val img = eng.qrImageDataUrl()
                    withContext(Dispatchers.Main) {
                        qrPayload = payload
                        qrImageDataUrl = img
                    }
                }
            }
            "stopped" -> engineOnline = false
            "peer_connected" -> event.peerId?.let { id ->
                addPeerUnique(id)
                if (id.startsWith("peer:")) {
                    connectedPeer = id
                    peerStatus[id] = "online"
                }
                refreshConnectedPeer()
            }
            "peer_profile" -> event.peerId?.let { id ->
                event.content?.takeIf { it.isNotBlank() }?.let { name ->
                    remoteDisplayNames[id] = name
                }
            }
            "peer_id_updated" -> {
                val oldId = event.message
                val newId = event.peerId
                if (oldId != null && newId != null) {
                    migratePeerId(oldId, newId)
                    if (connectedPeer == oldId || connectedPeer == null) connectedPeer = newId
                    if (activePeer == oldId || activePeer == null) {
                        activePeer = newId
                        messages = loadMessagesForPeer(newId)
                    }
                }
            }
            "sas_ready" -> {
                event.sas?.let { sas ->
                    val peer = event.peerId ?: return@let
                    addPeerUnique(peer)
                    if (event.autoTrusted == true) {
                        peerVerified[peer] = true
                        connectedPeer = peer
                        peerStatus[peer] = "online"
                        activePeer = peer
                        messages = loadMessagesForPeer(peer)
                        showSnackbar("Reconnected to trusted peer")
                    } else {
                        activePeer = peer
                        messages = loadMessagesForPeer(peer)
                        sasCode = sas
                        sasPeerId = peer
                        showSasDialog = true
                    }
                }
            }
            "peer_disconnected" -> event.peerId?.let { id ->
                transfers.clear()
                peers.remove(id)
                if (connectedPeer == id) connectedPeer = null
                val reason = event.message.orEmpty()
                peerStatus[id] = when (reason) {
                    "connection lost" -> "reconnecting"
                    "user disconnected" -> "paused"
                    else -> "offline"
                }
                if (activePeer == id && reason != "connection lost") {
                    activePeer = null
                    messages = emptyList()
                }
                if (reason == "connection lost") {
                    savedContacts.find { it.peerId == id && it.verified && it.qrPayload.isNotBlank() }
                        ?.let { reconnectContact(it, openChat = activePeer == id) }
                }
            }
            "message_queued" -> showSnackbar("Message queued — will send on reconnect")
            "reconnecting" -> {
                event.peerId?.let { peerStatus[it] = "reconnecting" }
                showSnackbar("Reconnecting…")
            }
            "message" -> event.content?.let { content ->
                messages = messages + ChatMessage(
                    content = content,
                    isSent = false,
                    sender = event.peerId ?: "peer",
                )
            }
            "transfer_progress" -> event.transferId?.let { id ->
                val filename = event.filename ?: "file"
                val progress = event.progress?.toFloat() ?: 0f
                val existing = transfers[id]
                val now = System.currentTimeMillis()
                val totalBytes = event.message?.toLongOrNull() ?: existing?.totalBytes ?: 0L
                var speedBps = existing?.speedBps ?: 0.0
                if (existing != null && totalBytes > 0 && now > existing.lastUpdateMs) {
                    val delta = (progress - existing.lastProgress).coerceAtLeast(0f)
                    val dt = (now - existing.lastUpdateMs) / 1000.0
                    if (dt > 0.05) speedBps = (delta * totalBytes) / dt
                }
                transfers[id] = TransferState(
                    id = id,
                    filename = filename,
                    progress = progress,
                    isOutgoing = existing?.isOutgoing ?: false,
                    totalBytes = totalBytes,
                    speedBps = speedBps,
                    lastProgress = progress,
                    lastUpdateMs = now,
                )
            }
            "transfer_cancelled" -> event.transferId?.let {
                transfers.remove(it)
                showSnackbar("Transfer cancelled")
            }
            "transfer_complete" -> event.transferId?.let { id ->
                val filename = event.filename ?: "file"
                val wasOutgoing = transfers[id]?.isOutgoing ?: false
                transfers.remove(id)
                val explicitPath = event.message
                val cachePath = File(context.cacheDir, filename)
                val recvPath = File(context.filesDir, "received/$filename")
                val mediaPath = when {
                    !explicitPath.isNullOrBlank() && File(explicitPath).exists() -> explicitPath
                    recvPath.exists() -> recvPath.absolutePath
                    cachePath.exists() -> cachePath.absolutePath
                    else -> null
                }
                if (mediaPath != null) {
                    addMediaMessage(mediaPath, filename, wasOutgoing, if (wasOutgoing) "You" else event.peerId ?: "peer")
                } else {
                    messages = messages + ChatMessage(
                        content = if (wasOutgoing) "📤 Sent: $filename" else "📁 Received: $filename",
                        isSent = wasOutgoing,
                        sender = if (wasOutgoing) "You" else event.peerId ?: "peer",
                        kind = MessageKind.FILE,
                        mediaPath = recvPath.takeIf { it.exists() }?.absolutePath,
                    )
                }
                showSnackbar(if (wasOutgoing) "Upload complete: $filename" else "Download complete: $filename")
            }
            "call_offer" -> {
                val peer = event.peerId
                val callId = event.callId
                if (peer != null && callId != null) {
                    val payload = event.message ?: ""
                    val isVideo = event.autoTrusted == true
                    if (callState != null || pendingIncomingCall != null) {
                        scope.launch(Dispatchers.IO) {
                            SrltcpEngineHolder.getOrCreate().sendCallSignal(peer, callId, "end", "", isVideo)
                        }
                    } else {
                        pendingIncomingCall = PendingCall(peer, callId, payload, isVideo)
                        showIncomingCallDialog = true
                    }
                }
            }
            "call_answer", "call_ice" -> {
                val peer = event.peerId ?: activePeer
                val callId = event.callId
                if (peer != null && callId != null) {
                    val payload = event.message ?: ""
                    val isVideo = event.autoTrusted == true
                    scope.launch {
                        try {
                            WebRtcCallManagerHolder.handleSignal(
                                context, peer, callId, event.eventType.removePrefix("call_"),
                                payload, isVideo,
                            ) { state -> callState = state }
                        } catch (e: Exception) {
                            showSnackbar("Call error: ${e.message ?: e}")
                        }
                    }
                }
            }
            "call_ended" -> {
                WebRtcCallManagerHolder.end()
                callState = null
                showIncomingCallDialog = false
                pendingIncomingCall = null
                showSnackbar("Call ended")
            }
            "error" -> showSnackbar(event.error ?: "Unknown error")
        }
    }

    val filePicker = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        val peer = activePeer ?: return@rememberLauncherForActivityResult
        if (connectedPeer != peer) {
            showSnackbar("Peer offline — tap Reconnect in Saved Peers")
            return@rememberLauncherForActivityResult
        }
        if (peerVerified[peer] != true) {
            showSnackbar("Verify peer with SAS first")
            return@rememberLauncherForActivityResult
        }
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val path = copyUriToCache(context, uri)
            if (path == null) {
                showSnackbar("Upload failed — could not read file")
                return@launch
            }
            val filename = File(path).name
            val fileSize = File(path).length()
            val result = withContext(Dispatchers.IO) {
                SrltcpEngineHolder.awaitEngine().sendFile(peer, path)
            }
            if (result.filename.startsWith("error:")) {
                showSnackbar(result.filename.removePrefix("error: ").trim())
            } else if (result.transferId.isNotEmpty()) {
                transfers[result.transferId] = TransferState(
                    id = result.transferId,
                    filename = result.filename,
                    progress = result.progress.toFloat(),
                    isOutgoing = true,
                    totalBytes = fileSize,
                )
                val kind = mediaKindForPath(path, filename)
                if (kind == MessageKind.IMAGE || kind == MessageKind.VIDEO) {
                    addMediaMessage(path, filename, true, "You")
                } else {
                    messages = messages + ChatMessage(
                        content = "📤 Sending: $filename",
                        isSent = true,
                        sender = "You",
                        kind = MessageKind.FILE,
                    )
                }
            }
        }
    }

    DisposableEffect(Unit) {
        SrltcpEngineHolder.addEventListener(eventListener)
        onDispose { SrltcpEngineHolder.removeEventListener(eventListener) }
    }

    if (!engineReady) {
        Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                CircularProgressIndicator()
                Spacer(modifier = Modifier.height(12.dp))
                Text("Starting secure P2P engine…", fontSize = 14.sp)
            }
        }
        return
    }

    LaunchedEffect(messages.size) {
        if (messages.isNotEmpty()) {
            listState.animateScrollToItem(messages.size - 1)
        }
    }

    val snackbarHostState = remember { SnackbarHostState() }
    LaunchedEffect(snackbarMessage) {
        snackbarMessage?.let {
            snackbarHostState.showSnackbar(it)
            snackbarMessage = null
        }
    }

    Scaffold(
        modifier = Modifier.fillMaxSize(),
        snackbarHost = { SnackbarHost(snackbarHostState) },
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text("SRLTCP", fontWeight = FontWeight.Bold)
                        Text(
                            "v0.2.22 • ${if (engineOnline) "Online" else "Offline"} • bg active",
                            fontSize = 12.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                },
                actions = {
                    if (activePeer != null) {
                        IconButton(onClick = { softDisconnect(activePeer!!) }) {
                            Icon(Icons.Default.LinkOff, contentDescription = "Disconnect")
                        }
                    }
                    IconButton(onClick = { showPeersSheet = true }) {
                        Icon(Icons.Default.People, contentDescription = "Peers")
                    }
                    IconButton(onClick = { showSettingsSheet = true }) {
                        Icon(Icons.Default.Settings, contentDescription = "Settings")
                    }
                    IconButton(onClick = { showConnectSheet = true }) {
                        Icon(Icons.Default.QrCode, contentDescription = "Connect peer")
                    }
                    IconButton(
                        onClick = { filePicker.launch("*/*") },
                        enabled = activePeer != null && peerVerified[activePeer] == true,
                    ) {
                        Icon(Icons.Default.AttachFile, contentDescription = "Send file")
                    }
                    IconButton(
                        onClick = {
                            val peer = activePeer ?: return@IconButton
                            if (connectedPeer != peer) {
                                showSnackbar("Peer offline — tap Reconnect")
                                return@IconButton
                            }
                            startOutgoingCall(peer, false)
                        },
                        enabled = activePeer != null && connectedPeer == activePeer && peerVerified[activePeer] == true && callState == null,
                    ) {
                        Icon(Icons.Default.Call, contentDescription = "Voice call")
                    }
                    IconButton(
                        onClick = {
                            val peer = activePeer ?: return@IconButton
                            if (connectedPeer != peer) {
                                showSnackbar("Peer offline — tap Reconnect")
                                return@IconButton
                            }
                            startOutgoingCall(peer, true)
                        },
                        enabled = activePeer != null && connectedPeer == activePeer && peerVerified[activePeer] == true && callState == null,
                    ) {
                        Icon(Icons.Default.Videocam, contentDescription = "Video call")
                    }
                },
            )
        },
        bottomBar = {
            Column(Modifier.imePadding()) {
                transfers.values.forEach { transfer ->
                    TransferProgressBar(
                        transfer = transfer,
                        onCancel = {
                            scope.launch(Dispatchers.IO) {
                                SrltcpEngineHolder.getOrCreate().cancelTransfer(transfer.id)
                            }
                            transfers.remove(transfer.id)
                        },
                    )
                }
                Surface(tonalElevation = 3.dp) {
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        OutlinedTextField(
                            value = inputText,
                            onValueChange = { inputText = it },
                            modifier = Modifier.weight(1f),
                            placeholder = { Text("Message…") },
                            singleLine = true,
                            enabled = activePeer != null && peerVerified[activePeer] == true,
                            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                            keyboardActions = KeyboardActions(onSend = {
                                sendCurrentMessage()
                                true
                            }),
                        )
                        Spacer(modifier = Modifier.width(8.dp))
                        FilledIconButton(
                            onClick = { sendCurrentMessage() },
                            enabled = activePeer != null && peerVerified[activePeer] == true,
                        ) {
                            Icon(Icons.AutoMirrored.Filled.Send, contentDescription = "Send")
                        }
                    }
                }
            }
        },
    ) { padding ->
        Column(modifier = Modifier.fillMaxSize().padding(padding)) {
            if (activePeer == null && qrPayload.isNotEmpty() && savedContacts.isEmpty()) {
                QrShareCard(
                    qrPayload = qrPayload,
                    qrImageDataUrl = qrImageDataUrl,
                    onCopy = {
                        copyTextToClipboard(context, "SRLTCP QR", qrPayload)
                        showSnackbar("QR copied to clipboard")
                    },
                )
            }
            activePeer?.let { peer ->
                if (peerVerified[peer] != true) {
                    Card(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 12.dp, vertical = 4.dp),
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.errorContainer,
                        ),
                    ) {
                        Row(
                            modifier = Modifier.padding(12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                "⚠ Peer not verified",
                                modifier = Modifier.weight(1f),
                                fontSize = 12.sp,
                                color = MaterialTheme.colorScheme.onErrorContainer,
                            )
                            TextButton(onClick = { showConnectSheet = true }) {
                                Text("Verify", fontSize = 12.sp)
                            }
                        }
                    }
                }
            }
            if (savedContacts.isNotEmpty()) {
                PeerChipRow(
                    peers = savedContacts.map { it.peerId },
                    activePeer = activePeer,
                    onSelect = { id ->
                        savedContacts.find { it.peerId == id }?.let { reconnectContact(it) }
                            ?: run { activePeer = id; messages = loadMessagesForPeer(id) }
                    },
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 4.dp),
                )
            }
            if (messages.isEmpty()) {
                Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text("Secure P2P Messaging", style = MaterialTheme.typography.headlineSmall)
                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            "Swipe away safely — service keeps running",
                            fontSize = 12.sp,
                            color = MaterialTheme.colorScheme.primary,
                        )
                    }
                }
            } else {
                LazyColumn(
                    state = listState,
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(horizontal = 16.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    contentPadding = PaddingValues(vertical = 8.dp),
                ) {
                    items(messages, key = { it.id }) { msg ->
                        MessageBubble(
                            message = msg,
                            onOpenFile = { path -> openFileOrFolder(context, path) { showSnackbar(it) } },
                        )
                    }
                }
            }
        }
    }

    if (showConnectSheet) {
        ConnectPeerSheet(
            remoteQr = remoteQrInput,
            onRemoteQrChange = { remoteQrInput = it },
            onClearQr = { remoteQrInput = "" },
            onDismiss = { showConnectSheet = false },
            onVerify = {
                scope.launch(Dispatchers.IO) {
                    val engine = SrltcpEngineHolder.getOrCreate()
                    val qr = remoteQrInput.trim()
                    if (qr.isEmpty()) {
                        showSnackbar("Paste peer QR code first")
                        return@launch
                    }
                    engine.waitUntilReady(30u)
                    val result = engine.connectAndVerify(qr)
                    withContext(Dispatchers.Main) {
                        val err = result.error
                            ?: result.sas.takeIf { it.startsWith("error:") }?.removePrefix("error: ")?.trim()
                        if (!err.isNullOrBlank()) {
                            showSnackbar(err)
                        } else if (result.peerId.isBlank()) {
                            showSnackbar("Connect failed — ensure peer is online with a fresh v4 QR")
                        } else {
                            val peer = result.peerId
                            addPeerUnique(peer)
                            activePeer = peer
                            connectedPeer = peer
                            messages = loadMessagesForPeer(peer)
                            remoteQrInput = ""
                            if (result.autoTrusted) {
                                peerVerified[peer] = true
                                val contact = SavedContact(
                                    peerId = peer,
                                    displayName = displayName.ifBlank { peer.take(12) },
                                    verified = true,
                                    qrPayload = qr,
                                )
                                prefs.upsertContact(contact)
                                val idx = savedContacts.indexOfFirst { it.peerId == peer }
                                if (idx >= 0) savedContacts[idx] = contact else savedContacts.add(contact)
                                syncTrustedPubkeys(engine)
                                showSnackbar("Reconnected to trusted peer")
                            } else {
                                sasCode = result.sas
                                sasPeerId = peer
                                showSasDialog = true
                            }
                            showConnectSheet = false
                        }
                    }
                }
            },
        )
    }

    if (showSasDialog) {
        SasVerificationDialog(
            sasCode = sasCode,
            peerId = sasPeerId ?: "",
            onConfirm = {
                val peerId = sasPeerId ?: return@SasVerificationDialog
                showSasDialog = false
                scope.launch(Dispatchers.IO) {
                    val engine = SrltcpEngineHolder.getOrCreate()
                    engine.confirmPeerTrusted(peerId)
                    val savedQr = savedContacts.find { it.peerId == peerId }?.qrPayload
                        ?: remoteQrInput.trim()
                    if (savedQr.isNotBlank()) engine.registerSavedPeer(peerId, savedQr)
                    val contact = SavedContact(
                        peerId = peerId,
                        displayName = displayName.ifBlank { peerId.take(20) },
                        verified = true,
                        qrPayload = savedQr,
                    )
                    prefs.upsertContact(contact)
                    syncTrustedPubkeys(engine)
                    syncDisplayName(peerId)
                    withContext(Dispatchers.Main) {
                        peerVerified[peerId] = true
                        connectedPeer = peerId
                        val idx = savedContacts.indexOfFirst { it.peerId == peerId }
                        if (idx >= 0) savedContacts[idx] = contact else savedContacts.add(contact)
                        showSnackbar("Peer verified — secure channel established")
                    }
                }
            },
            onReject = {
                sasPeerId?.let { softDisconnect(it) }
                showSasDialog = false
                showSnackbar("SAS mismatch — not trusted")
            },
        )
    }

    if (showSettingsSheet) {
        SettingsSheet(
            version = "0.2.23",
            receiveDir = receiveDirPath,
            displayName = displayName,
            onCopyReceiveDir = {
                copyTextToClipboard(context, "receive dir", receiveDirPath)
                showSnackbar("Save folder path copied")
            },
            onRequestCallPermissions = {
                ensureCallPermissions(true) { showSnackbar("Mic/camera permissions granted") }
            },
            onDisplayNameChange = { name ->
                displayName = name
                prefs.displayName = name
                syncDisplayName(connectedPeer)
            },
            onDismiss = { showSettingsSheet = false },
        )
    }

    if (showIncomingCallDialog) {
        val incoming = pendingIncomingCall
        if (incoming != null) {
            IncomingCallDialog(
                peerLabel = contactLabel(incoming.peerId),
                isVideo = incoming.isVideo,
                onAnswer = {
                    showIncomingCallDialog = false
                    val call = pendingIncomingCall
                    pendingIncomingCall = null
                    if (call != null) {
                        ensureCallPermissions(call.isVideo) {
                            scope.launch {
                                try {
                                    WebRtcCallManagerHolder.handleSignal(
                                        context, call.peerId, call.callId, "offer",
                                        call.sdp, call.isVideo,
                                    ) { state -> callState = state }
                                } catch (e: Exception) {
                                    showSnackbar("Answer failed: ${e.message ?: e}")
                                    scope.launch(Dispatchers.IO) {
                                        SrltcpEngineHolder.getOrCreate()
                                            .sendCallSignal(call.peerId, call.callId, "end", "", call.isVideo)
                                    }
                                }
                            }
                        }
                    }
                },
                onDecline = {
                    showIncomingCallDialog = false
                    val call = pendingIncomingCall
                    pendingIncomingCall = null
                    if (call != null) {
                        scope.launch(Dispatchers.IO) {
                            SrltcpEngineHolder.getOrCreate()
                                .sendCallSignal(call.peerId, call.callId, "end", "", call.isVideo)
                        }
                    }
                    showSnackbar("Call declined")
                },
            )
        }
    }

    if (showPeersSheet) {
        val onlinePeers = SrltcpEngineHolder.engineOrNull()
            ?.connectedPeers()
            ?.filter { it.startsWith("peer:") }
            ?.ifEmpty { connectedPeer?.let { listOf(it) } }
            ?: connectedPeer?.let { listOf(it) }
            ?: emptyList()
        PeersSheet(
            onlinePeers = onlinePeers,
            contacts = savedContacts.toList(),
            activePeer = activePeer,
            connectedPeer = connectedPeer,
            remoteDisplayNames = remoteDisplayNames,
            peerStatus = peerStatus,
            onSelect = { contact ->
                showPeersSheet = false
                if (connectedPeer == contact.peerId) {
                    activePeer = contact.peerId
                    messages = loadMessagesForPeer(contact.peerId)
                } else if (contact.verified && contact.qrPayload.isNotBlank()) {
                    reconnectContact(contact)
                }
            },
            onSelectOnline = { peerId ->
                showPeersSheet = false
                activePeer = peerId
                messages = loadMessagesForPeer(peerId)
            },
            onReconnect = { contact -> reconnectContact(contact) },
            onRemove = { removeContact(it) },
            onDisconnect = { softDisconnect(it) },
            onDismiss = { showPeersSheet = false },
        )
    }

    callState?.let { call ->
        ActiveCallOverlay(
            call = call,
            peerLabel = contactLabel(call.peerId),
            onEnd = { endActiveCall() },
        )
    }
}

@Composable
fun QrShareCard(
    qrPayload: String,
    qrImageDataUrl: String,
    onCopy: () -> Unit,
) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 4.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant,
        ),
    ) {
        Column(
            modifier = Modifier.padding(12.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "Your QR (share with peers)",
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 12.sp,
                    modifier = Modifier.weight(1f),
                )
                IconButton(onClick = onCopy) {
                    Icon(Icons.Default.ContentCopy, contentDescription = "Copy QR")
                }
            }
            val bitmap = remember(qrImageDataUrl) {
                decodeQrDataUrl(qrImageDataUrl)
            }
            bitmap?.let { bmp ->
                Image(
                    bitmap = bmp.asImageBitmap(),
                    contentDescription = "QR code",
                    modifier = Modifier
                        .size(180.dp)
                        .padding(vertical = 8.dp),
                )
            }
            Surface(
                color = MaterialTheme.colorScheme.inverseSurface,
                shape = MaterialTheme.shapes.small,
                modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
            ) {
                Text(
                    qrPayload,
                    fontSize = 10.sp,
                    modifier = Modifier.padding(10.dp),
                    color = MaterialTheme.colorScheme.inverseOnSurface,
                    lineHeight = 14.sp,
                )
            }
        }
    }
}

private fun decodeQrDataUrl(dataUrl: String): android.graphics.Bitmap? {
    if (!dataUrl.startsWith("data:image/png;base64,")) return null
    val b64 = dataUrl.removePrefix("data:image/png;base64,")
    return try {
        val bytes = Base64.decode(b64, Base64.DEFAULT)
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
    } catch (_: Exception) {
        null
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ConnectPeerSheet(
    remoteQr: String,
    onRemoteQrChange: (String) -> Unit,
    onClearQr: () -> Unit,
    onDismiss: () -> Unit,
    onVerify: () -> Unit,
) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(modifier = Modifier.padding(20.dp).padding(bottom = 32.dp)) {
            Text("Connect New Peer", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                "Paste the peer's QR code. Your QR is on the home screen when no chat is open.",
                fontSize = 13.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                lineHeight = 18.sp,
            )
            Spacer(modifier = Modifier.height(12.dp))
            OutlinedTextField(
                value = remoteQr,
                onValueChange = onRemoteQrChange,
                label = { Text("Peer QR code") },
                modifier = Modifier.fillMaxWidth(),
                minLines = 3,
                trailingIcon = {
                    if (remoteQr.isNotEmpty()) {
                        IconButton(onClick = onClearQr) {
                            Icon(Icons.Default.Clear, contentDescription = "Clear")
                        }
                    }
                },
            )
            Spacer(modifier = Modifier.height(12.dp))
            Button(onClick = onVerify, modifier = Modifier.fillMaxWidth()) {
                Icon(Icons.Default.Lock, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(modifier = Modifier.width(8.dp))
                Text("Verify Peer (QR + SAS)")
            }
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                "Paste the peer's QR — connection starts automatically, then confirm SAS.",
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}

@Composable
fun SasVerificationDialog(
    sasCode: String,
    peerId: String,
    onConfirm: () -> Unit,
    onReject: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = {},
        title = { Text("Verify Security Code") },
        text = {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text(
                    "Compare this code with your peer. Both must show the same number.",
                    fontSize = 13.sp,
                )
                Spacer(modifier = Modifier.height(16.dp))
                Text(
                    sasCode,
                    fontSize = 36.sp,
                    fontWeight = FontWeight.Bold,
                    letterSpacing = 6.sp,
                    color = MaterialTheme.colorScheme.primary,
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text("Peer: ${peerId.take(24)}", fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        },
        confirmButton = {
            Button(onClick = onConfirm) { Text("Codes Match") }
        },
        dismissButton = {
            TextButton(onClick = onReject) { Text("Don't Match") }
        },
    )
}

@Composable
fun PeerChipRow(
    peers: List<String>,
    activePeer: String?,
    onSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier.horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        peers.forEach { peer ->
            FilterChip(
                selected = peer == activePeer,
                onClick = { onSelect(peer) },
                label = { Text(peer.take(16), fontSize = 11.sp) },
            )
        }
    }
}

@Composable
fun IncomingCallDialog(
    peerLabel: String,
    isVideo: Boolean,
    onAnswer: () -> Unit,
    onDecline: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDecline,
        title = { Text(if (isVideo) "Incoming video call" else "Incoming voice call") },
        text = { Text("$peerLabel is calling") },
        confirmButton = {
            Button(onClick = onAnswer) { Text("Answer") }
        },
        dismissButton = {
            TextButton(onClick = onDecline) { Text("Decline") }
        },
    )
}

@Composable
fun CallStatusBar(call: CallState, peerLabel: String, onEndCall: () -> Unit) {
    Surface(
        color = MaterialTheme.colorScheme.primaryContainer,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                if (call.isVideo) Icons.Default.Videocam else Icons.Default.Call,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onPrimaryContainer,
            )
            Spacer(modifier = Modifier.width(8.dp))
            Text(
                text = if (call.isVideo) "Video call with $peerLabel" else "Voice call with $peerLabel",
                modifier = Modifier.weight(1f),
                color = MaterialTheme.colorScheme.onPrimaryContainer,
            )
            FilledTonalButton(onClick = onEndCall) {
                Icon(Icons.Default.CallEnd, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(modifier = Modifier.width(4.dp))
                Text("End")
            }
        }
    }
}

@Composable
fun TransferProgressBar(transfer: TransferState, onCancel: (() -> Unit)? = null) {
    val label = if (transfer.isOutgoing) "Sending" else "Receiving"
    val speedMb = transfer.speedBps / (1024.0 * 1024.0)
    val speedLabel = if (speedMb >= 0.01) " • ${"%.2f".format(speedMb)} MB/s" else ""
    Column(modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                "$label: ${transfer.filename} (${(transfer.progress * 100).toInt()}%$speedLabel)",
                fontSize = 12.sp,
                modifier = Modifier.weight(1f),
            )
            if (transfer.isOutgoing && onCancel != null) {
                TextButton(onClick = onCancel) { Text("Cancel") }
            }
        }
        LinearProgressIndicator(
            progress = { transfer.progress },
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
fun MessageBubble(message: ChatMessage, onOpenFile: ((String) -> Unit)? = null) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = if (message.isSent) Arrangement.End else Arrangement.Start,
    ) {
        Surface(
            shape = MaterialTheme.shapes.medium,
            color = if (message.isSent) MaterialTheme.colorScheme.primary
            else MaterialTheme.colorScheme.surfaceVariant,
            modifier = Modifier.widthIn(max = 300.dp),
        ) {
            Column(modifier = Modifier.padding(8.dp)) {
                when (message.kind) {
                    MessageKind.IMAGE -> {
                        message.mediaPath?.let { path ->
                            AsyncImage(
                                model = File(path),
                                contentDescription = message.content,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .heightIn(max = 240.dp),
                                contentScale = ContentScale.Fit,
                            )
                        } ?: Text(
                            text = "🖼 ${message.content}",
                            color = if (message.isSent) MaterialTheme.colorScheme.onPrimary
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    MessageKind.VIDEO -> message.mediaPath?.let { path ->
                        VideoPreview(path = path)
                    }
                    else -> {}
                }
                if (message.kind == MessageKind.FILE && message.mediaPath != null) {
                    Text(
                        text = message.content,
                        color = if (message.isSent) MaterialTheme.colorScheme.onPrimary
                        else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    TextButton(onClick = { onOpenFile?.invoke(message.mediaPath!!) }) {
                        Text("Open file", fontSize = 11.sp)
                    }
                } else if (message.kind != MessageKind.IMAGE && message.kind != MessageKind.VIDEO) {
                    Text(
                        text = message.content,
                        color = if (message.isSent) MaterialTheme.colorScheme.onPrimary
                        else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else if (message.kind == MessageKind.VIDEO) {
                    Text(
                        text = message.content,
                        fontSize = 11.sp,
                        color = if (message.isSent) MaterialTheme.colorScheme.onPrimary.copy(0.8f)
                        else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (message.sender.isNotEmpty()) {
                    Text(
                        message.sender,
                        fontSize = 10.sp,
                        color = if (message.isSent) MaterialTheme.colorScheme.onPrimary.copy(alpha = 0.7f)
                        else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

@Composable
fun VideoPreview(path: String) {
    Column(modifier = Modifier.fillMaxWidth()) {
        var videoView by remember { mutableStateOf<VideoView?>(null) }
        AndroidView(
            factory = { ctx ->
                VideoView(ctx).also { vv ->
                    videoView = vv
                    val controller = MediaController(ctx)
                    vv.setMediaController(controller)
                    controller.setAnchorView(vv)
                    vv.setVideoPath(path)
                    vv.setOnPreparedListener { mp ->
                        mp.isLooping = false
                        controller.show(0)
                    }
                    vv.setOnErrorListener { _, _, _ -> false }
                }
            },
            update = { vv ->
                if (vv.tag != path) {
                    vv.tag = path
                    vv.setVideoPath(path)
                }
            },
            modifier = Modifier.fillMaxWidth().height(220.dp),
        )
        Row(
            modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            TextButton(onClick = { videoView?.start() }) { Text("Play") }
            TextButton(onClick = { videoView?.pause() }) { Text("Pause") }
        }
        Text(
            File(path).name,
            fontSize = 11.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = 4.dp),
        )
    }
}

@Composable
fun ActiveCallOverlay(
    call: CallState,
    peerLabel: String,
    onEnd: () -> Unit,
) {
    var muted by remember { mutableStateOf(false) }
    Dialog(
        onDismissRequest = {},
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Surface(modifier = Modifier.fillMaxSize()) {
            Column(
                modifier = Modifier.fillMaxSize().padding(12.dp),
                verticalArrangement = Arrangement.SpaceBetween,
            ) {
                Column {
                    Text(
                        if (call.isVideo) "Video call" else "Voice call",
                        fontWeight = FontWeight.Bold,
                        fontSize = 16.sp,
                    )
                    Text(peerLabel, fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                if (call.isVideo) {
                    Row(
                        modifier = Modifier.weight(1f).fillMaxWidth().padding(vertical = 8.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        AndroidView(
                            factory = { ctx ->
                                SurfaceViewRenderer(ctx).apply {
                                    WebRtcCallManagerHolder.bindRemote(this)
                                }
                            },
                            modifier = Modifier.weight(1f).fillMaxHeight(),
                        )
                        AndroidView(
                            factory = { ctx ->
                                SurfaceViewRenderer(ctx).apply {
                                    WebRtcCallManagerHolder.bindLocal(this)
                                }
                            },
                            modifier = Modifier.weight(0.45f).fillMaxHeight(),
                        )
                    }
                } else {
                    Box(
                        modifier = Modifier.weight(1f).fillMaxWidth(),
                        contentAlignment = Alignment.Center,
                    ) {
                        Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            Icon(Icons.Default.Call, contentDescription = null, modifier = Modifier.size(72.dp))
                            Spacer(modifier = Modifier.height(8.dp))
                            Text("Voice call active", fontSize = 14.sp)
                        }
                    }
                }
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    OutlinedButton(
                        onClick = {
                            muted = !muted
                            WebRtcCallManagerHolder.setMute(muted)
                        },
                        modifier = Modifier.weight(1f),
                    ) {
                        Icon(if (muted) Icons.Default.MicOff else Icons.Default.Mic, contentDescription = null)
                        Spacer(modifier = Modifier.width(4.dp))
                        Text(if (muted) "Unmute" else "Mute")
                    }
                    Button(
                        onClick = onEnd,
                        modifier = Modifier.weight(1f),
                        colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.error),
                    ) {
                        Icon(Icons.Default.CallEnd, contentDescription = null)
                        Spacer(modifier = Modifier.width(4.dp))
                        Text("End")
                    }
                }
            }
        }
    }
}

private fun openFileOrFolder(context: Context, path: String, onMsg: (String) -> Unit) {
    val file = File(path)
    if (!file.exists()) {
        onMsg("File not found: $path")
        return
    }
    try {
        val uri = androidx.core.content.FileProvider.getUriForFile(
            context,
            "${context.packageName}.fileprovider",
            file,
        )
        val mime = context.contentResolver.getType(uri)
            ?: MimeTypeMap.getSingleton().getMimeTypeFromExtension(file.extension.lowercase())
            ?: "*/*"
        val intent = android.content.Intent(android.content.Intent.ACTION_VIEW).apply {
            setDataAndType(uri, mime)
            addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        context.startActivity(android.content.Intent.createChooser(intent, "Open with"))
    } catch (_: Exception) {
        val folder = file.parentFile?.absolutePath ?: path
        copyTextToClipboard(context, "file path", folder)
        onMsg("Saved at: $folder")
    }
}

private fun copyTextToClipboard(context: Context, label: String, text: String) {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    clipboard.setPrimaryClip(ClipData.newPlainText(label, text))
}

private fun mediaKindForPath(path: String, filename: String): MessageKind {
    val ext = filename.substringAfterLast('.', "").lowercase()
    val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext) ?: ""
    return when {
        mime.startsWith("image/") || ext in setOf("jpg", "jpeg", "png", "gif", "webp", "bmp") -> MessageKind.IMAGE
        mime.startsWith("video/") || ext in setOf("mp4", "webm", "mkv", "3gp", "mov") -> MessageKind.VIDEO
        else -> MessageKind.FILE
    }
}

private suspend fun copyUriToCache(context: Context, uri: Uri): String? = withContext(Dispatchers.IO) {
    try {
        val mime = context.contentResolver.getType(uri)
        val ext = MimeTypeMap.getSingleton().getExtensionFromMimeType(mime ?: "") ?: "bin"
        val name = "upload_${System.currentTimeMillis()}.$ext"
        val out = File(context.cacheDir, name)
        val input = context.contentResolver.openInputStream(uri)
            ?: return@withContext null
        input.use { stream ->
            out.outputStream().use { output -> stream.copyTo(output) }
        }
        if (!out.exists() || out.length() == 0L) return@withContext null
        out.absolutePath
    } catch (_: Exception) {
        null
    }
}