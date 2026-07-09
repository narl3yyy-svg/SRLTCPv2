package com.srltcp.v2.ui

import androidx.compose.foundation.layout.*
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
    displayName: String,
    onDisplayNameChange: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var nameInput by remember(displayName) { mutableStateOf(displayName) }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(modifier = Modifier.padding(20.dp).padding(bottom = 32.dp)) {
            Text("Settings", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
            Spacer(modifier = Modifier.height(12.dp))

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