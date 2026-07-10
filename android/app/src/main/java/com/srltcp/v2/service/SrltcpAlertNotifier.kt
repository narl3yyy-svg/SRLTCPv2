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

    fun notifyMessage(context: Context, peerLabel: String, preview: String) {
        if (AppVisibility.isInForeground) return
        ensureChannels(context)
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val id = 1000 + (messageCounter++ % 50)
        val text = preview.take(120).ifBlank { "New message" }
        manager.notify(id, buildAlert(context, CHANNEL_MESSAGES, "Message from $peerLabel", text, id))
    }

    fun notifyIncomingCall(context: Context, peerLabel: String, isVideo: Boolean) {
        if (AppVisibility.isInForeground) return
        ensureChannels(context)
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val kind = if (isVideo) "Video call" else "Voice call"
        manager.notify(
            ID_INCOMING_CALL,
            buildAlert(context, CHANNEL_CALLS, "Incoming $kind", "From $peerLabel", ID_INCOMING_CALL),
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
    ): android.app.Notification {
        val pendingIntent = PendingIntent.getActivity(
            context,
            requestCode,
            Intent(context, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        return NotificationCompat.Builder(context, channelId)
            .setContentTitle(title)
            .setContentText(text)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentIntent(pendingIntent)
            .setAutoCancel(true)
            .setCategory(
                if (channelId == CHANNEL_CALLS) {
                    NotificationCompat.CATEGORY_CALL
                } else {
                    NotificationCompat.CATEGORY_MESSAGE
                },
            )
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setDefaults(NotificationCompat.DEFAULT_ALL)
            .build()
    }
}