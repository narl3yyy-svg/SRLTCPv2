package com.srltcp.v2.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import com.srltcp.v2.MainActivity
import com.srltcp.v2.R
import com.srltcp.v2.SrltcpEngineHolder
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import uniffi.srltcp_core.SrltcpEvent

/**
 * Foreground service that keeps the Rust P2P core alive in the background.
 *
 * - Survives swipe-away and Home button
 * - Only fully stops on Force Stop from App Info (process kill)
 * - Explicit ACTION_STOP shuts down gracefully (for testing)
 */
class SrltcpForegroundService : Service() {

    companion object {
        const val TAG = "SrltcpService"
        const val CHANNEL_ID = "srltcp_p2p"
        const val NOTIFICATION_ID = 1

        const val ACTION_START = "com.srltcp.v2.START"
        const val ACTION_STOP = "com.srltcp.v2.STOP"

        @Volatile
        var isRunning = false
            private set
    }

    private var eventListener: ((SrltcpEvent) -> Unit)? = null
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        Log.i(TAG, "Foreground service created")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                stopServiceGracefully()
                return START_NOT_STICKY
            }
            else -> startForegroundService()
        }
        // START_STICKY: OS restarts service if killed (engine stays in holder)
        return START_STICKY
    }

    private fun startForegroundService() {
        val notification = buildNotification("Listening for peers…")

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }

        isRunning = true
        startP2pCore()
        Log.i(TAG, "Foreground service started — P2P core active")
    }

    private fun startP2pCore() {
        SrltcpEngineHolder.startInBackground()
        eventListener = { event ->
            when (event.eventType) {
                "peer_connected" -> updateNotification("Connected: ${event.peerId}")
                "message" -> updateNotification("Message from ${event.peerId}")
                "started" -> updateNotification("Online — iroh NAT traversal")
                "stopped" -> updateNotification("Stopped")
                "error" -> updateNotification("Error: ${event.error}")
            }
            Log.d(TAG, "Engine event: ${event.eventType}")
        }
        eventListener?.let { SrltcpEngineHolder.addEventListener(it) }

        scope.launch {
            try {
                val engine = SrltcpEngineHolder.awaitEngine()
                val peers = engine.connectedPeers()
                Log.i(
                    TAG,
                    "Rust engine running=${engine.isRunning()} peers=${peers.size} key=${engine.publicKeyHex().take(16)}…"
                )
            } catch (e: Exception) {
                Log.e(TAG, "Failed to start P2P core", e)
                updateNotification("Error — check logcat")
            }
        }
    }

    private fun stopServiceGracefully() {
        Log.i(TAG, "Explicit stop requested")
        eventListener?.let {
            SrltcpEngineHolder.removeEventListener(it)
            eventListener = null
        }
        // Only explicit stop — Force Stop kills process without this path
        SrltcpEngineHolder.shutdown()
        isRunning = false
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun buildNotification(contentText: String): Notification {
        val pendingIntent = PendingIntent.getActivity(
            this, 0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("SRLTCP")
            .setContentText(contentText)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentIntent(pendingIntent)
            .setOngoing(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }

    private fun updateNotification(text: String) {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.notify(NOTIFICATION_ID, buildNotification(text))
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "SRLTCP P2P Service",
            NotificationManager.IMPORTANCE_LOW
        ).apply {
            description = "Keeps SRLTCP connected for incoming messages"
            setShowBadge(false)
        }
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.createNotificationChannel(channel)
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        // Do NOT shutdown engine here — user may have only swiped UI away.
        // Engine stays in SrltcpEngineHolder; START_STICKY restarts service.
        eventListener?.let { SrltcpEngineHolder.removeEventListener(it) }
        isRunning = false
        super.onDestroy()
        Log.i(TAG, "Service destroyed (engine kept alive in holder)")
    }
}