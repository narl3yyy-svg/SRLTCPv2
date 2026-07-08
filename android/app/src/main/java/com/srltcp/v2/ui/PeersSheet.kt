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
    onSelect: (String) -> Unit,
    onRemove: (String) -> Unit,
    onDisconnect: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(modifier = Modifier.padding(20.dp).padding(bottom = 32.dp)) {
            Text("Saved Peers", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                "Remove a contact to revoke trust and clear saved data.",
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(modifier = Modifier.height(12.dp))

            if (contacts.isEmpty()) {
                Text("No saved peers yet.", fontSize = 13.sp)
            } else {
                contacts.forEach { contact ->
                    val label = contact.displayName.ifBlank { contact.peerId.take(20) }
                    val verified = if (contact.verified) " ✓" else ""
                    Card(
                        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
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
                                Text("$label$verified", fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
                                Text(contact.peerId.take(28), fontSize = 10.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                            if (contact.peerId == activePeer) {
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