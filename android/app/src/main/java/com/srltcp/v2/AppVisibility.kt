package com.srltcp.v2

/**
 * Tracks whether [MainActivity] is in the foreground.
 * Used by [com.srltcp.v2.service.SrltcpForegroundService] to decide when to post alert notifications.
 */
object AppVisibility {
    @Volatile
    var isInForeground: Boolean = false
}