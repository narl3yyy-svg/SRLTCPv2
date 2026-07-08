package com.srltcp.v2

import android.content.Context
import android.net.Uri
import android.os.Bundle
import android.webkit.MimeTypeMap
import android.widget.VideoView
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
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
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import coil.compose.AsyncImage
import com.srltcp.v2.service.SrltcpForegroundService
import com.srltcp.v2.ui.theme.SRLTCPTheme
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.srltcp_core.SrltcpEvent
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
)

data class CallState(
    val callId: String,
    val peerId: String,
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
    val peers = remember { mutableStateListOf<String>() }
    var engineOnline by remember { mutableStateOf(false) }
    val transfers = remember { mutableStateMapOf<String, TransferState>() }
    var callState by remember { mutableStateOf<CallState?>(null) }
    var snackbarMessage by remember { mutableStateOf<String?>(null) }
    var showConnectSheet by remember { mutableStateOf(false) }
    var showSasDialog by remember { mutableStateOf(false) }
    var sasCode by remember { mutableStateOf("") }
    var sasPeerId by remember { mutableStateOf<String?>(null) }
    var remoteQrInput by remember { mutableStateOf("") }
    val peerVerified = remember { mutableStateMapOf<String, Boolean>() }
    val listState = rememberLazyListState()

    fun showSnackbar(msg: String) { snackbarMessage = msg }

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
            "started" -> engineOnline = true
            "stopped" -> engineOnline = false
            "peer_connected" -> event.peerId?.let { id ->
                if (!peers.contains(id)) peers.add(id)
                if (!peerVerified.containsKey(id)) peerVerified[id] = false
                if (activePeer == null) activePeer = id
                showSnackbar("Peer connected — verify with QR + SAS")
            }
            "sas_ready" -> {
                event.sas?.let { sas ->
                    sasCode = sas
                    sasPeerId = event.peerId
                    showSasDialog = true
                }
            }
            "peer_disconnected" -> event.peerId?.let { peers.remove(it) }
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
                transfers[id] = TransferState(
                    id = id,
                    filename = filename,
                    progress = progress,
                    isOutgoing = existing?.isOutgoing ?: false,
                )
            }
            "transfer_complete" -> event.transferId?.let { id ->
                val filename = event.filename ?: "file"
                val wasOutgoing = transfers[id]?.isOutgoing ?: false
                transfers.remove(id)
                val cachePath = File(context.cacheDir, filename)
                val recvPath = File(context.filesDir, "received/$filename")
                val mediaPath = when {
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
                    )
                }
                showSnackbar(if (wasOutgoing) "Upload complete: $filename" else "Download complete: $filename")
            }
            "voice_call_started" -> {
                val callId = event.callId ?: ""
                if (callId.startsWith("error:")) {
                    showSnackbar(callId.removePrefix("error: ").trim())
                } else {
                    callState = CallState(callId, event.peerId ?: activePeer ?: "", false)
                }
            }
            "video_call_started" -> {
                val callId = event.callId ?: ""
                if (callId.startsWith("error:")) {
                    showSnackbar(callId.removePrefix("error: ").trim())
                } else {
                    callState = CallState(callId, event.peerId ?: activePeer ?: "", true)
                }
            }
            "call_ended" -> {
                callState = null
                showSnackbar("Call ended")
            }
            "error" -> showSnackbar(event.error ?: "Unknown error")
        }
    }

    val filePicker = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        val peer = activePeer ?: return@rememberLauncherForActivityResult
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val path = copyUriToCache(context, uri) ?: return@launch
            val filename = File(path).name
            val result = withContext(Dispatchers.IO) {
                SrltcpEngineHolder.getOrCreate().sendFile(peer, path)
            }
            if (result.filename.startsWith("error:")) {
                showSnackbar(result.filename.removePrefix("error: ").trim())
            } else if (result.transferId.isNotEmpty()) {
                transfers[result.transferId] = TransferState(
                    id = result.transferId,
                    filename = result.filename,
                    progress = result.progress.toFloat(),
                    isOutgoing = true,
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
        val engine = SrltcpEngineHolder.getOrCreate()
        engineOnline = engine.isRunning()
        qrPayload = engine.qrPayload()
        engine.connectedPeers().forEach { if (!peers.contains(it)) peers.add(it) }
        if (peers.isNotEmpty() && activePeer == null) activePeer = peers[0]
        SrltcpEngineHolder.addEventListener(eventListener)
        onDispose { SrltcpEngineHolder.removeEventListener(eventListener) }
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
        snackbarHost = { SnackbarHost(snackbarHostState) },
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text("SRLTCP", fontWeight = FontWeight.Bold)
                        Text(
                            "v0.2.5 • ${if (engineOnline) "Online" else "Offline"} • bg active",
                            fontSize = 12.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                },
                actions = {
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
                            scope.launch(Dispatchers.IO) {
                                val id = SrltcpEngineHolder.getOrCreate().startVoiceCall(peer)
                                if (id.startsWith("error:")) {
                                    showSnackbar(id.removePrefix("error: ").trim())
                                }
                            }
                        },
                        enabled = activePeer != null && peerVerified[activePeer] == true && callState == null,
                    ) {
                        Icon(Icons.Default.Call, contentDescription = "Voice call")
                    }
                    IconButton(
                        onClick = {
                            val peer = activePeer ?: return@IconButton
                            scope.launch(Dispatchers.IO) {
                                val id = SrltcpEngineHolder.getOrCreate().startVideoCall(peer)
                                if (id.startsWith("error:")) {
                                    showSnackbar(id.removePrefix("error: ").trim())
                                }
                            }
                        },
                        enabled = activePeer != null && peerVerified[activePeer] == true && callState == null,
                    ) {
                        Icon(Icons.Default.Videocam, contentDescription = "Video call")
                    }
                },
            )
        },
        bottomBar = {
            Column {
                callState?.let { call ->
                    CallStatusBar(
                        call = call,
                        onEndCall = {
                            scope.launch(Dispatchers.IO) {
                                SrltcpEngineHolder.getOrCreate().endCall(call.callId)
                            }
                        },
                    )
                }
                transfers.values.filter { !it.isComplete }.forEach { transfer ->
                    TransferProgressBar(transfer)
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
                        )
                        Spacer(modifier = Modifier.width(8.dp))
                        FilledIconButton(
                            onClick = {
                                val peer = activePeer ?: return@FilledIconButton
                                if (peerVerified[peer] != true) {
                                    showSnackbar("Verify peer with SAS first")
                                    return@FilledIconButton
                                }
                                if (inputText.isNotBlank()) {
                                    SrltcpEngineHolder.getOrCreate().sendMessage(peer, inputText)
                                    messages = messages + ChatMessage(
                                        content = inputText,
                                        isSent = true,
                                        sender = "You",
                                    )
                                    inputText = ""
                                }
                            },
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
            if (qrPayload.isNotEmpty()) {
                Card(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 12.dp, vertical = 4.dp),
                    colors = CardDefaults.cardColors(
                        containerColor = MaterialTheme.colorScheme.primaryContainer,
                    ),
                ) {
                    Column(modifier = Modifier.padding(12.dp)) {
                        Text("Your QR (share with peers)", fontWeight = FontWeight.SemiBold, fontSize = 12.sp)
                        Text(
                            qrPayload,
                            fontSize = 9.sp,
                            modifier = Modifier.padding(top = 4.dp),
                            color = MaterialTheme.colorScheme.onPrimaryContainer,
                        )
                    }
                }
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
            if (peers.isNotEmpty()) {
                PeerChipRow(
                    peers = peers,
                    activePeer = activePeer,
                    onSelect = { activePeer = it },
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
                    modifier = Modifier.fillMaxSize().padding(horizontal = 16.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    contentPadding = PaddingValues(vertical = 8.dp),
                ) {
                    items(messages, key = { it.id }) { msg -> MessageBubble(msg) }
                }
            }
        }
    }

    if (showConnectSheet) {
        ConnectPeerSheet(
            qrPayload = qrPayload,
            remoteQr = remoteQrInput,
            onRemoteQrChange = { remoteQrInput = it },
            onDismiss = { showConnectSheet = false },
            onVerify = {
                scope.launch(Dispatchers.IO) {
                    val engine = SrltcpEngineHolder.getOrCreate()
                    val qr = remoteQrInput.trim()
                    if (qr.isEmpty()) {
                        showSnackbar("Paste peer QR code first")
                        return@launch
                    }
                    var peer = activePeer
                    if (peer == null) {
                        val connected = engine.connectedPeers()
                        peer = connected.firstOrNull()
                    }
                    if (peer == null) {
                        showSnackbar("No peer connected — share your QR first")
                        return@launch
                    }
                    val sas = engine.handshakeWith(peer, qr)
                    withContext(Dispatchers.Main) {
                        if (sas.startsWith("error:")) {
                            showSnackbar(sas.removePrefix("error: ").trim())
                        } else {
                            sasCode = sas
                            sasPeerId = peer
                            showSasDialog = true
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
                sasPeerId?.let { peerVerified[it] = true }
                showSasDialog = false
                showSnackbar("Peer verified — secure channel established")
            },
            onReject = {
                sasPeerId?.let { peer ->
                    scope.launch(Dispatchers.IO) {
                        SrltcpEngineHolder.getOrCreate().disconnectPeer(peer)
                    }
                    peers.remove(peer)
                    peerVerified.remove(peer)
                    if (activePeer == peer) activePeer = peers.firstOrNull()
                }
                showSasDialog = false
                showSnackbar("SAS mismatch — peer disconnected")
            },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ConnectPeerSheet(
    qrPayload: String,
    remoteQr: String,
    onRemoteQrChange: (String) -> Unit,
    onDismiss: () -> Unit,
    onVerify: () -> Unit,
) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(modifier = Modifier.padding(20.dp).padding(bottom = 32.dp)) {
            Text("Connect Securely", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                "Paste the peer's QR code to verify identity via SAS.",
                fontSize = 13.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(modifier = Modifier.height(12.dp))
            if (qrPayload.isNotEmpty()) {
                Text("Your QR:", fontSize = 11.sp, fontWeight = FontWeight.SemiBold)
                Text(qrPayload, fontSize = 9.sp, modifier = Modifier.padding(vertical = 4.dp))
                HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp))
            }
            OutlinedTextField(
                value = remoteQr,
                onValueChange = onRemoteQrChange,
                label = { Text("Peer QR code") },
                modifier = Modifier.fillMaxWidth(),
                minLines = 2,
            )
            Spacer(modifier = Modifier.height(12.dp))
            Button(onClick = onVerify, modifier = Modifier.fillMaxWidth()) {
                Icon(Icons.Default.Lock, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(modifier = Modifier.width(8.dp))
                Text("Verify Peer (QR + SAS)")
            }
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                "Peers connect via QR exchange — no manual IP entry needed.",
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
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
fun CallStatusBar(call: CallState, onEndCall: () -> Unit) {
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
                text = if (call.isVideo) "Video call with ${call.peerId}" else "Voice call with ${call.peerId}",
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
fun TransferProgressBar(transfer: TransferState) {
    val label = if (transfer.isOutgoing) "Sending" else "Receiving"
    Column(modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)) {
        Text("$label: ${transfer.filename} (${(transfer.progress * 100).toInt()}%)", fontSize = 12.sp)
        LinearProgressIndicator(
            progress = { transfer.progress },
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
fun MessageBubble(message: ChatMessage) {
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
                    MessageKind.IMAGE -> message.mediaPath?.let { path ->
                        AsyncImage(
                            model = File(path),
                            contentDescription = message.content,
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(max = 240.dp),
                            contentScale = ContentScale.Fit,
                        )
                    }
                    MessageKind.VIDEO -> message.mediaPath?.let { path ->
                        VideoPreview(path = path)
                    }
                    else -> {}
                }
                if (message.kind != MessageKind.IMAGE && message.kind != MessageKind.VIDEO) {
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
    var playing by remember { mutableStateOf(false) }
    if (playing) {
        AndroidView(
            factory = { ctx ->
                VideoView(ctx).apply {
                    setVideoPath(path)
                    setOnPreparedListener { mp -> mp.isLooping = false; start() }
                }
            },
            modifier = Modifier.fillMaxWidth().height(200.dp),
        )
    } else {
        Box(
            modifier = Modifier.fillMaxWidth().height(120.dp),
            contentAlignment = Alignment.Center,
        ) {
            FilledTonalButton(onClick = { playing = true }) {
                Icon(Icons.Default.PlayArrow, contentDescription = "Play video")
                Spacer(modifier = Modifier.width(4.dp))
                Text(File(path).name)
            }
        }
    }
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
        context.contentResolver.openInputStream(uri)?.use { input ->
            out.outputStream().use { output -> input.copyTo(output) }
        }
        out.absolutePath
    } catch (_: Exception) {
        null
    }
}