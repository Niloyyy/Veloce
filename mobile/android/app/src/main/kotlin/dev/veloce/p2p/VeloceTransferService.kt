package dev.veloce.p2p

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager

/**
 * Foreground Service holding a partial WakeLock so active QUIC (BBR) transfers
 * complete reliably when the Android screen turns off or the user switches apps.
 */
class VeloceTransferService : Service() {
    private var wakeLock: PowerManager.WakeLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                releaseWakeLock()
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                return START_NOT_STICKY
            }
            else -> {
                val title = intent?.getStringExtra(EXTRA_TITLE) ?: "Veloce Transfer"
                val body = intent?.getStringExtra(EXTRA_BODY) ?: "Encrypted P2P transfer active"
                ensureChannel()
                acquireWakeLock()
                val notification = buildNotification(title, body)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    startForeground(
                        NOTIFICATION_ID,
                        notification,
                        ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
                    )
                } else {
                    startForeground(NOTIFICATION_ID, notification)
                }
                return START_STICKY
            }
        }
    }

    private fun ensureChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            val channel = NotificationChannel(
                CHANNEL_ID,
                "Veloce File Transfers",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Active encrypted P2P QUIC file transfers"
            }
            manager.createNotificationChannel(channel)
        }
    }

    private fun buildNotification(title: String, body: String): Notification {
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }
        return builder
            .setContentTitle(title)
            .setContentText(body)
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setOngoing(true)
            .build()
    }

    private fun acquireWakeLock() {
        val pm = getSystemService(Context.POWER_SERVICE) as? PowerManager ?: return
        if (wakeLock == null) {
            wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "veloce:transfer-wakelock")
                .apply { setReferenceCounted(false) }
        }
        if (wakeLock?.isHeld == false) {
            // Bounded 2-hour timeout prevents stuck wakelocks on crash.
            wakeLock?.acquire(2 * 60 * 60 * 1000L)
        }
    }

    private fun releaseWakeLock() {
        if (wakeLock?.isHeld == true) {
            wakeLock?.release()
        }
    }

    override fun onDestroy() {
        releaseWakeLock()
        super.onDestroy()
    }

    companion object {
        const val ACTION_START = "dev.veloce.p2p.action.START_TRANSFER"
        const val ACTION_STOP = "dev.veloce.p2p.action.STOP_TRANSFER"
        const val EXTRA_TITLE = "extra_title"
        const val EXTRA_BODY = "extra_body"
        private const val CHANNEL_ID = "veloce_transfers"
        private const val NOTIFICATION_ID = 4201
    }
}
