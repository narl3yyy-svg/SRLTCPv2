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
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
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
                if (activePeer == null) activePeer = id
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
                transfers[id] = TransferState(id, filename, 1f, transfers[id]?.isOutgoing ?: false, true)
                val savedPath = File(context.filesDir, "received/$filename").absolutePath
                if (File(savedPath).exists()) {
                    addMediaMessage(savedPath, filename, false, event.peerId ?: "peer")
                } else {
                    messages = messages + ChatMessage(
                        content = "📁 Received: $filename",
                        isSent = false,
                        sender = event.peerId ?: "peer",
                        kind = MessageKind.FILE,
                    )
                }
            }
            "voice_call_started" -> {
                callState = CallState(
                    callId = event.callId ?: "",
                    peerId = event.peerId ?: activePeer ?: "",
                    isVideo = false,
                )
            }
            "video_call_started" -> {
                callState = CallState(
                    callId = event.callId ?: "",
                    peerId = event.peerId ?: activePeer ?: "",
                    isVideo = true,
                )
            }
            "call_ended" -> callState = null
            "error" -> {
                messages = messages + ChatMessage(
                    content = "⚠ ${event.error ?: "Unknown error"}",
                    isSent = false,
                    sender = "System",
                )
            }
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
            if (result.transferId.isNotEmpty()) {
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

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text("SRLTCP", fontWeight = FontWeight.Bold)
                        Text(
                            "v0.2.0 • ${if (engineOnline) "Online" else "Offline"} • bg active",
                            fontSize = 12.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                },
                actions = {
                    IconButton(
                        onClick = { filePicker.launch("*/*") },
                        enabled = activePeer != null,
                    ) {
                        Icon(Icons.Default.AttachFile, contentDescription = "Send file")
                    }
                    IconButton(
                        onClick = {
                            val peer = activePeer ?: return@IconButton
                            scope.launch(Dispatchers.IO) {
                                SrltcpEngineHolder.getOrCreate().startVoiceCall(peer)
                            }
                        },
                        enabled = activePeer != null && callState == null,
                    ) {
                        Icon(Icons.Default.Call, contentDescription = "Voice call")
                    }
                    IconButton(
                        onClick = {
                            val peer = activePeer ?: return@IconButton
                            scope.launch(Dispatchers.IO) {
                                SrltcpEngineHolder.getOrCreate().startVideoCall(peer)
                            }
                        },
                        enabled = activePeer != null && callState == null,
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
                            enabled = activePeer != null,
                        )
                        Spacer(modifier = Modifier.width(8.dp))
                        FilledIconButton(
                            onClick = {
                                val peer = activePeer ?: return@FilledIconButton
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
                            enabled = activePeer != null,
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
                Text(
                    "QR: ${qrPayload.take(48)}…",
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
                    fontSize = 10.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (peers.isNotEmpty()) {
                Text(
                    "Peers: ${peers.joinToString()}",
                    modifier = Modifier.padding(horizontal = 16.dp),
                    fontSize = 12.sp,
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
                    modifier = Modifier.fillMaxSize().padding(horizontal = 16.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    contentPadding = PaddingValues(vertical = 8.dp),
                ) {
                    items(messages, key = { it.id }) { msg -> MessageBubble(msg) }
                }
            }
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
                if (message.kind != MessageKind.IMAGE) {
                    Text(
                        text = message.content,
                        color = if (message.isSent) MaterialTheme.colorScheme.onPrimary
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