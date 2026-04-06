package com.hydra.network.hydra_mobile

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Intent
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log

class HydraVpnService : VpnService() {
    companion object {
        private const val NOTIFICATION_CHANNEL_ID = "hydra_vpn"
        private const val NOTIFICATION_ID = 1001
        const val ACTION_CONNECT = "com.hydra.network.START_VPN"
        const val ACTION_DISCONNECT = "com.hydra.network.STOP_VPN"
        var isRunning = false
        var currentFd: Int = -1
        var onVpnStarted: ((Int) -> Unit)? = null
        private var currentInterface: ParcelFileDescriptor? = null
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        Log.i("HydraVpnService", "onStartCommand action=${intent?.action} startId=$startId")
        if (intent?.action == ACTION_DISCONNECT) {
            stopVpn()
            return START_NOT_STICKY
        }

        startForegroundCompat()
        startVpn()
        return START_STICKY
    }

    private fun startVpn() {
        if (isRunning || currentInterface != null) return

        try {
            val builder = Builder()
                .addAddress("10.0.0.2", 24)
                .addDnsServer("8.8.8.8")
                .addRoute("0.0.0.0", 0)
                .setSession("Hydra Network")
                .setMtu(1500)
                // We must exclude our own app traffic to prevent routing loops, or bind our SOCKS proxy sockets using VpnService.protect()
                // For simplicity, we can exclude the app itself, so P2P traffic goes directly via standard network
                .addDisallowedApplication(packageName)

            val vpnInterface = builder.establish()
                ?: throw IllegalStateException("VpnService.establish() returned null")
            val rustTunnelFd = ParcelFileDescriptor.dup(vpnInterface.fileDescriptor).detachFd()
            currentInterface = vpnInterface
            isRunning = true
            currentFd = rustTunnelFd
            Log.i("HydraVpnService", "VPN established with duplicated Rust FD: $rustTunnelFd")
            onVpnStarted?.invoke(rustTunnelFd)

        } catch (e: Exception) {
            Log.e("HydraVpnService", "Failed to start VPN", e)
            stopVpn()
        }
    }

    private fun stopVpn() {
        Log.i("HydraVpnService", "Stopping VPN service")
        isRunning = false
        currentFd = -1
        currentInterface?.close()
        currentInterface = null
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onDestroy() {
        Log.i("HydraVpnService", "onDestroy")
        stopVpn()
        super.onDestroy()
    }

    private fun startForegroundCompat() {
        val manager = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                NOTIFICATION_CHANNEL_ID,
                "Hydra VPN",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Keeps the Hydra VPN tunnel active"
            }
            manager.createNotificationChannel(channel)
        }

        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, NOTIFICATION_CHANNEL_ID)
        } else {
            Notification.Builder(this)
        }

        val notification = builder
            .setContentTitle("Hydra VPN")
            .setContentText("Hydra tunnel is active")
            .setSmallIcon(R.mipmap.ic_launcher)
            .setOngoing(true)
            .setCategory(Notification.CATEGORY_SERVICE)
            .build()

        startForeground(NOTIFICATION_ID, notification)
        Log.i("HydraVpnService", "Foreground notification started")
    }
}
