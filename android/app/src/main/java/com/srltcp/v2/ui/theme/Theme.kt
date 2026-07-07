package com.srltcp.v2.ui.theme

import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val SRLTCPDarkColorScheme = darkColorScheme(
    primary = Color(0xFF6C5CE7),
    onPrimary = Color.White,
    secondary = Color(0xFFA29BFE),
    background = Color(0xFF0F1117),
    surface = Color(0xFF1A1D27),
    surfaceVariant = Color(0xFF242836),
    onBackground = Color(0xFFE8EAED),
    onSurface = Color(0xFFE8EAED),
    onSurfaceVariant = Color(0xFF9AA0B0),
)

@Composable
fun SRLTCPTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = SRLTCPDarkColorScheme,
        typography = Typography(),
        content = content
    )
}