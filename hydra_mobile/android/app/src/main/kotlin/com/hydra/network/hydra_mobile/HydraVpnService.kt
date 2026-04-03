package com.hydra.network.hydra_mobile

import android.content.Intent
import android.net.VpnService
import android.os.ParcelFileDescriptor
import android.util.Log

class HydraVpnService : VpnService() {
    companion object {
        const val ACTION_CONNECT = "com.hydra.network.START_VPN"
        const val ACTION_DISCONNECT = "com.hydra.network.STOP_VPN"
        var isRunning = false
        var currentFd: Int = -1
        var onVpnStarted: ((Int) -> Unit)? = null
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_DISCONNECT) {
            stopVpn()
            return START_NOT_STICKY
        }
        
        startVpn()
        return START_STICKY
    }

    private fun startVpn() {
        if (isRunning || currentFd >= 0) return

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
            val rawFd = vpnInterface.detachFd()
            isRunning = true
            currentFd = rawFd
            Log.i("HydraVpnService", "VPN established with detached FD: $rawFd")
            onVpnStarted?.invoke(rawFd)
            
        } catch (e: Exception) {
            Log.e("HydraVpnService", "Failed to start VPN", e)
            stopVpn()
        }
    }

    private fun stopVpn() {
        isRunning = false
        currentFd = -1
        stopSelf()
    }

    override fun onDestroy() {
        stopVpn()
        super.onDestroy()
    }
}
