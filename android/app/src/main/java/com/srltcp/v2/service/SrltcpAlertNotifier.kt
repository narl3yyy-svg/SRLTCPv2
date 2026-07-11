package com.srltcp.v2.service

import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import com.srltcp.v2.AppVisibility
import com.srltcp.v2.MainActivity
import com.srltcp.v2.R

/**
 * High-priority alert notifications for messages and calls while the app is backgrounded.
 * The foreground-service notification (IMPORTANCE_LOW) alone does not alert the user.
 */
object SrltcpAlertNotifier {

    const val CHANNEL_MESSAGES = "srltcp_messages"
    const val CHANNEL_CALLS = "srltcp_calls"

    private const val ID_INCOMING_CALL = 2001
    private var messageCounter = 0

    fun ensureChannels(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

        val messages = android.app.NotificationChannel(
            CHANNEL_MESSAGES,
            context.getString(R.string.notification_channel_messages_name),
            NotificationManager.IMPORTANCE_HIGH,
        ).apply {
            description = context.getString(R.string.notification_channel_messages_desc)
            enableVibration(true)
            enableLights(true)
        }

        val calls = android.app.NotificationChannel(
            CHANNEL_CALLS,
            context.getString(R.string.notification_channel_calls_name),
            NotificationManager.IMPORTANCE_HIGH,
        ).apply {
            description = context.getString(R.string.notification_channel_calls_desc)
            enableVibration(true)
            enableLights(true)
        }

        manager.createNotificationChannel(messages)
        manager.createNotificationChannel(calls)
    }

    /** When true (default), alerts only fire while app is backgrounded. */
    @Volatile
    var onlyWhenBackground: Boolean = true

    fun notifyMessage(context: Context, peerLabel: String, preview: String) {
        if (onlyWhenBackground && AppVisibility.isInForeground) return
        ensureChannels(context)
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val id = 1000 + (messageCounter++ % 50)
        val text = preview.take(120).ifBlank { "New message" }
        manager.notify(id, buildAlert(context, CHANNEL_MESSAGES, "Message from $peerLabel", text, id, isCall = false))
    }

    fun notifyIncomingCall(context: Context, peerLabel: String, isVideo: Boolean) {
        // Calls always alert (even if UI is up) so user can answer from another screen
        ensureChannels(context)
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val kind = if (isVideo) "Video call" else "Voice call"
        manager.notify(
            ID_INCOMING_CALL,
            buildAlert(context, CHANNEL_CALLS, "Incoming $kind", "From $peerLabel", ID_INCOMING_CALL, isCall = true),
        )
    }

    fun cancelIncomingCall(context: Context) {
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.cancel(ID_INCOMING_CALL)
    }

    fun cancelMessageAlerts(context: Context) {
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        for (i in 0 until 50) {
            manager.cancel(1000 + i)
        }
    }

    fun cancelAllAlerts(context: Context) {
        cancelIncomingCall(context)
        cancelMessageAlerts(context)
    }

    private fun buildAlert(
        context: Context,
        channelId: String,
        title: String,
        text: String,
        requestCode: Int,
        isCall: Boolean = false,
    ): android.app.Notification {
        val pendingIntent = PendingIntent.getActivity(
            context,
            requestCode,
            Intent(context, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_NEW_TASK
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        val builder = NotificationCompat.Builder(context, channelId)
            .setContentTitle(title)
            .setContentText(text)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentIntent(pendingIntent)
            .setAutoCancel(true)
            .setOnlyAlertOnce(false)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setCategory(
                if (isCall) {
                    NotificationCompat.CATEGORY_CALL
                } else {
                    NotificationCompat.CATEGORY_MESSAGE
                },
            )
            .setPriority(NotificationCompat.PRIORITY_MAX)
            .setDefaults(NotificationCompat.DEFAULT_ALL)

        if (isCall) {
            builder.setFullScreenIntent(pendingIntent, true)
            builder.setOngoing(true)
            builder.setTimeoutAfter(60_000)
        }

        return builder.build()
    }
}