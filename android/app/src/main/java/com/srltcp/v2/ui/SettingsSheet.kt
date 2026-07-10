package com.srltcp.v2.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsSheet(
    version: String,
    receiveDir: String,
    displayName: String,
    onCopyReceiveDir: () -> Unit,
    onRequestCallPermissions: () -> Unit,
    onRequestNotificationPermission: () -> Unit,
    onDisplayNameChange: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var nameInput by remember(displayName) { mutableStateOf(displayName) }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            modifier = Modifier
                .padding(20.dp)
                .padding(bottom = 32.dp)
                .verticalScroll(rememberScrollState()),
        ) {
            Text("Settings", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
            Spacer(modifier = Modifier.height(12.dp))

            Text("Files save to", fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
            Text(
                receiveDir.ifBlank { "(not set)" },
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                lineHeight = 14.sp,
            )
            Spacer(modifier = Modifier.height(6.dp))
            OutlinedButton(onClick = onCopyReceiveDir, modifier = Modifier.fillMaxWidth()) {
                Text("Copy save folder path")
            }

            HorizontalDivider(modifier = Modifier.padding(vertical = 16.dp))

            Text("Display name", fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
            OutlinedTextField(
                value = nameInput,
                onValueChange = { nameInput = it },
                modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("Shown after peer verification") },
                singleLine = true,
            )
            Spacer(modifier = Modifier.height(8.dp))
            Button(
                onClick = {
                    onDisplayNameChange(nameInput.trim())
                    onDismiss()
                },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("Save display name")
            }

            HorizontalDivider(modifier = Modifier.padding(vertical = 16.dp))

            Text("Calls", fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
            Text(
                "Grant microphone and camera before voice/video calls.",
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(modifier = Modifier.height(6.dp))
            OutlinedButton(onClick = onRequestCallPermissions, modifier = Modifier.fillMaxWidth()) {
                Text("Grant mic & camera permissions")
            }

            HorizontalDivider(modifier = Modifier.padding(vertical = 16.dp))

            Text("Notifications", fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
            Text(
                "Allow alerts for background messages and incoming calls.",
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(modifier = Modifier.height(6.dp))
            OutlinedButton(onClick = onRequestNotificationPermission, modifier = Modifier.fillMaxWidth()) {
                Text("Enable notification alerts")
            }

            HorizontalDivider(modifier = Modifier.padding(vertical = 16.dp))

            Text("Update app", fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
            Text(
                "On your computer: git pull && ./run.sh\nReleases: github.com/narl3yyy-svg/SRLTCPv2",
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                lineHeight = 16.sp,
            )
            Spacer(modifier = Modifier.height(8.dp))
            Text("Version $version", fontSize = 11.sp, color = MaterialTheme.colorScheme.primary)
        }
    }
}