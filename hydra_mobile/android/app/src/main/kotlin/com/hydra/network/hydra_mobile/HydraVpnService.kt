package com.hydra.network.hydra_mobile

import android.content.Intent
import android.net.VpnService
import android.os.ParcelFileDescriptor
import android.util.Log

class HydraVpnService : VpnService() {
    private var vpnInterface: ParcelFileDescriptor? = null

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
        if (vpnInterface != null) return

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

            vpnInterface = builder.establish()
            isRunning = true
            
            val fd = vpnInterface?.fd ?: return
            currentFd = fd
            Log.i("HydraVpnService", "VPN established with FD: $fd")
            onVpnStarted?.invoke(fd)
            
        } catch (e: Exception) {
            Log.e("HydraVpnService", "Failed to start VPN", e)
            stopVpn()
        }
    }

    private fun stopVpn() {
        try {
            vpnInterface?.close()
        } catch (e: Exception) {
            Log.e("HydraVpnService", "Error closing VPN interface", e)
        }
        vpnInterface = null
        isRunning = false
        currentFd = -1
        stopSelf()
    }

    override fun onDestroy() {
        stopVpn()
        super.onDestroy()
    }
}
