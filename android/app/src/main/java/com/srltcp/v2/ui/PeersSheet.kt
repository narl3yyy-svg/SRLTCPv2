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
    contacts: List<SavedContact>,
    activePeer: String?,
    connectedPeer: String?,
    onSelect: (SavedContact) -> Unit,
    onReconnect: (SavedContact) -> Unit,
    onRemove: (String) -> Unit,
    onDisconnect: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(modifier = Modifier.padding(20.dp).padding(bottom = 32.dp)) {
            Text("Saved Peers", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                "Tap a contact to open chat. Disconnect ends the session but keeps the contact. Remove revokes trust.",
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                lineHeight = 16.sp,
            )
            Spacer(modifier = Modifier.height(12.dp))

            if (contacts.isEmpty()) {
                Text("No saved peers yet. Connect via QR to add one.", fontSize = 13.sp)
            } else {
                contacts.forEach { contact ->
                    val label = contact.displayName.ifBlank { contact.peerId.removePrefix("peer:").take(12) }
                    val isActive = contact.peerId == activePeer
                    val isConnected = contact.peerId == connectedPeer
                    val status = when {
                        isConnected && contact.verified -> "● Online"
                        contact.verified -> "○ Offline — tap Reconnect"
                        else -> "Unverified"
                    }
                    Card(
                        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                        onClick = { onSelect(contact) },
                        colors = CardDefaults.cardColors(
                            containerColor = if (isActive) {
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
                                Text(label, fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
                                Text(status, fontSize = 10.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
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