package com.srltcp.v2.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.srltcp.v2.data.SavedContact

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PeersSheet(
    onlinePeers: List<String>,
    contacts: List<SavedContact>,
    activePeer: String?,
    connectedPeer: String?,
    remoteDisplayNames: Map<String, String>,
    peerStatus: Map<String, String>,
    onSelect: (SavedContact) -> Unit,
    onSelectOnline: (String) -> Unit,
    onReconnect: (SavedContact) -> Unit,
    onRemove: (String) -> Unit,
    onDisconnect: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    fun label(peerId: String): String {
        remoteDisplayNames[peerId]?.takeIf { it.isNotBlank() }?.let { return it }
        contacts.find { it.peerId == peerId }?.displayName?.takeIf { it.isNotBlank() }?.let { return it }
        return peerId.removePrefix("peer:").take(12)
    }

    @Composable
    fun statusText(peerId: String): Pair<String, androidx.compose.ui.graphics.Color> {
        return when {
            connectedPeer == peerId -> "● Online" to MaterialTheme.colorScheme.primary
            peerStatus[peerId] == "reconnecting" -> "↻ Reconnecting" to MaterialTheme.colorScheme.tertiary
            peerStatus[peerId] == "paused" -> "⏸ Disconnected by you" to MaterialTheme.colorScheme.onSurfaceVariant
            contacts.find { it.peerId == peerId }?.verified == true -> "○ Offline" to MaterialTheme.colorScheme.onSurfaceVariant
            else -> "Unverified" to MaterialTheme.colorScheme.onSurfaceVariant
        }
    }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(modifier = Modifier.padding(20.dp).padding(bottom = 32.dp)) {
            Text("Peers", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                "Peers Online shows active sessions. Saved Contacts keeps verified peers for reconnect.",
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                lineHeight = 16.sp,
            )
            Spacer(modifier = Modifier.height(12.dp))

            Text(
                "Peers Online (${onlinePeers.size})",
                fontWeight = FontWeight.SemiBold,
                fontSize = 13.sp,
            )
            Spacer(modifier = Modifier.height(6.dp))

            if (onlinePeers.isEmpty()) {
                Text("No peers connected right now.", fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            } else {
                onlinePeers.forEach { peerId ->
                    val (status, color) = statusText(peerId)
                    Card(
                        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                        onClick = { onSelectOnline(peerId) },
                        colors = CardDefaults.cardColors(
                            containerColor = if (peerId == activePeer) {
                                MaterialTheme.colorScheme.primaryContainer
                            } else {
                                MaterialTheme.colorScheme.surfaceVariant
                            },
                        ),
                    ) {
                        Row(
                            modifier = Modifier.padding(12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Column(modifier = Modifier.weight(1f)) {
                                Text(label(peerId), fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
                                Text(status, fontSize = 10.sp, color = color)
                            }
                            TextButton(onClick = { onDisconnect(peerId) }) {
                                Text("Disconnect", fontSize = 11.sp)
                            }
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(16.dp))
            Text("Saved Contacts (${contacts.size})", fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
            Spacer(modifier = Modifier.height(6.dp))

            if (contacts.isEmpty()) {
                Text("No saved contacts yet. Connect via QR to add one.", fontSize = 13.sp)
            } else {
                contacts.forEach { contact ->
                    val peerLabel = label(contact.peerId)
                    val (status, color) = statusText(contact.peerId)
                    val isConnected = connectedPeer == contact.peerId
                    Card(
                        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                        onClick = { onSelect(contact) },
                        colors = CardDefaults.cardColors(
                            containerColor = if (contact.peerId == activePeer) {
                                MaterialTheme.colorScheme.primaryContainer
                            } else {
                                MaterialTheme.colorScheme.surfaceVariant
                            },
                        ),
                    ) {
                        Row(
                            modifier = Modifier.padding(12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Column(modifier = Modifier.weight(1f)) {
                                Text(peerLabel, fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
                                Text(status, fontSize = 10.sp, color = color)
                            }
                            if (contact.verified && !isConnected && contact.qrPayload.isNotBlank()) {
                                TextButton(onClick = { onReconnect(contact) }) {
                                    Text("Reconnect", fontSize = 11.sp)
                                }
                            }
                            if (isConnected) {
                                TextButton(onClick = { onDisconnect(contact.peerId) }) {
                                    Text("Disconnect", fontSize = 11.sp)
                                }
                            }
                            IconButton(onClick = { onRemove(contact.peerId) }) {
                                Icon(Icons.Default.Delete, contentDescription = "Remove contact")
                            }
                        }
                    }
                }
            }
        }
    }
}