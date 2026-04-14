package com.hydra.network.hydra_mobile

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log
import android.util.LruCache
import java.net.InetAddress
import java.net.InetSocketAddress

data class AppInfo(
    val uid: Int,
    val packageName: String?,
    val appLabel: String?
)

object AppResolver {
    private const val TAG = "AppResolver"
    private const val CACHE_SIZE = 512
    
    private val uidCache = LruCache<Int, AppInfo>(CACHE_SIZE)
    
    private var connectivityManager: ConnectivityManager? = null
    private var packageManager: PackageManager? = null
    
    fun init(context: Context) {
        connectivityManager = context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
        packageManager = context.packageManager
        Log.i(TAG, "AppResolver initialized")
    }
    
    fun resolveByConnection(
        protocol: Int,
        localIp: String,
        localPort: Int,
        remoteIp: String,
        remotePort: Int
    ): AppInfo? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            Log.d(TAG, "getConnectionOwnerUid requires API 29+")
            return null
        }
        
        val cm = connectivityManager ?: return null
        val pm = packageManager ?: return null
        
        return try {
            val localInetAddr = InetAddress.getByName(localIp)
            val remoteInetAddr = InetAddress.getByName(remoteIp)
            val localAddr = InetSocketAddress(localInetAddr, localPort)
            val remoteAddr = InetSocketAddress(remoteInetAddr, remotePort)
            
            val uid = cm.getConnectionOwnerUid(protocol, localAddr, remoteAddr)
            if (uid == android.os.Process.INVALID_UID) {
                return null
            }
            
            val appInfo = getOrCreateAppInfo(uid, pm)
            Log.d(TAG, "Found owner: $localIp:$localPort -> $remoteIp:$remotePort uid=$uid package=${appInfo?.packageName} label=${appInfo?.appLabel}")
            appInfo
        } catch (e: Exception) {
            Log.w(TAG, "Failed to resolve: $localIp:$localPort -> $remoteIp:$remotePort: ${e.message}")
            null
        }
    }

    @JvmStatic
    fun resolveByConnectionJson(
        protocol: Int,
        localIp: String,
        localPort: Int,
        remoteIp: String,
        remotePort: Int
    ): String = toJson(
        resolveByConnection(protocol, localIp, localPort, remoteIp, remotePort)
    )

    private fun toSocketAddress(host: String, port: Int): InetSocketAddress {
        return if (isIpLiteral(host)) {
            InetSocketAddress(InetAddress.getByName(host), port)
        } else {
            InetSocketAddress.createUnresolved(host, port)
        }
    }

    private fun isIpLiteral(host: String): Boolean {
        return try {
            InetAddress.getByName(host)
            !host.any { it.isLetter() }
        } catch (_: Exception) {
            false
        }
    }
    
    private fun getOrCreateAppInfo(uid: Int, pm: PackageManager): AppInfo {
        uidCache.get(uid)?.let { return it }

        val info = resolveAppInfoForUid(uid, pm)
        uidCache.put(uid, info)
        return info
    }

    private fun resolveAppInfoForUid(uid: Int, pm: PackageManager): AppInfo {
        safePackagesForUid(pm, uid)
            ?.firstOrNull()
            ?.let { packageName ->
                return buildPackageAppInfo(uid, packageName, pm)
            }

        findPackageByAppId(uid, pm)?.let { packageName ->
            Log.d(TAG, "Resolved uid=$uid via appId fallback to package=$packageName")
            return buildPackageAppInfo(uid, packageName, pm)
        }

        return syntheticAppInfo(uid, safeNameForUid(pm, uid))
    }

    private fun safePackagesForUid(pm: PackageManager, uid: Int): Array<String>? {
        return try {
            pm.getPackagesForUid(uid)
        } catch (e: Exception) {
            Log.w(TAG, "getPackagesForUid failed for uid=$uid: ${e.message}")
            null
        }
    }

    private fun safeNameForUid(pm: PackageManager, uid: Int): String? {
        return try {
            pm.getNameForUid(uid)
        } catch (e: Exception) {
            Log.w(TAG, "getNameForUid failed for uid=$uid: ${e.message}")
            null
        }
    }

    private fun findPackageByAppId(uid: Int, pm: PackageManager): String? {
        val targetAppId = appIdForUid(uid)
        val installedApps = try {
            @Suppress("DEPRECATION")
            pm.getInstalledApplications(0)
        } catch (e: Exception) {
            Log.w(TAG, "getInstalledApplications failed for uid=$uid: ${e.message}")
            return null
        }

        installedApps.firstOrNull { it.uid == uid }?.let { return it.packageName }

        val candidates = installedApps.filter { appInfo ->
            appIdForUid(appInfo.uid) == targetAppId
        }

        return candidates
            .sortedWith(
                compareBy<android.content.pm.ApplicationInfo> { appInfo ->
                    (appInfo.flags and android.content.pm.ApplicationInfo.FLAG_SYSTEM) != 0
                }.thenBy { appInfo -> appInfo.packageName }
            )
            .firstOrNull()
            ?.packageName
    }

    private fun appIdForUid(uid: Int): Int {
        val perUserRange = 100000
        return if (uid >= 0) uid % perUserRange else uid
    }

    private fun buildPackageAppInfo(
        uid: Int,
        packageName: String,
        pm: PackageManager
    ): AppInfo {
        val appLabel = try {
            val appInfo = pm.getApplicationInfo(packageName, 0)
            pm.getApplicationLabel(appInfo).toString()
        } catch (e: Exception) {
            Log.w(TAG, "getApplicationLabel failed for uid=$uid package=$packageName: ${e.message}")
            packageName.substringAfterLast('.')
        }

        return AppInfo(uid, packageName, appLabel)
    }

    private fun syntheticAppInfo(uid: Int, packageHint: String?): AppInfo {
        val (packageName, label) = when {
            uid == 0 -> "android.system.root" to "Root"
            uid == 1000 -> "android.system" to "System"
            uid in 1001..9999 -> "android.system.$uid" to "System ($uid)"
            !packageHint.isNullOrBlank() -> packageHint to packageHint.substringAfterLast(':')
            else -> "uid.$uid" to "UID $uid"
        }
        return AppInfo(uid, packageName, label)
    }
    
    fun toJson(appInfo: AppInfo?): String {
        if (appInfo == null) {
            return """{"uid":-1,"package_name":null,"app_label":null}"""
        }
        val pkgJson = appInfo.packageName?.let { "\"${it.replace("\"", "\\\"")}\"" } ?: "null"
        val labelJson = appInfo.appLabel?.let { "\"${it.replace("\"", "\\\"")}\"" } ?: "null"
        return """{"uid":${appInfo.uid},"package_name":$pkgJson,"app_label":$labelJson}"""
    }
}

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

        AppResolver.init(this)
        
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
