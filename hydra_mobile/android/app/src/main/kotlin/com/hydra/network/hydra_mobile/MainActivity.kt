package com.hydra.network.hydra_mobile

import android.content.Intent
import android.net.VpnService
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.annotation.NonNull
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.util.concurrent.Executors

class MainActivity: FlutterActivity() {
    private val CHANNEL = "com.hydra.network/vpn"
    private val VPN_REQUEST_CODE = 0x0F
    private var methodChannel: MethodChannel? = null
    private val appResolverExecutor = Executors.newSingleThreadExecutor()

    override fun configureFlutterEngine(@NonNull flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        
        AppResolver.init(this)
        
        methodChannel = MethodChannel(flutterEngine.dartExecutor.binaryMessenger, CHANNEL)
        
        HydraVpnService.onVpnStarted = { fd ->
            Handler(Looper.getMainLooper()).post {
                methodChannel?.invokeMethod("onVpnStarted", fd)
            }
        }
        
        methodChannel?.setMethodCallHandler { call, result ->
            when (call.method) {
                "startVpn" -> {
                    val intent = VpnService.prepare(this)
                    if (intent != null) {
                        startActivityForResult(intent, VPN_REQUEST_CODE)
                    } else {
                        onActivityResult(VPN_REQUEST_CODE, RESULT_OK, null)
                    }
                    result.success(true)
                }
                "stopVpn" -> {
                    val intent = Intent(this, HydraVpnService::class.java)
                    intent.action = HydraVpnService.ACTION_DISCONNECT
                    startService(intent)
                    result.success(true)
                }
                "getVpnFd" -> {
                    result.success(HydraVpnService.currentFd)
                }
                "getVpnActive" -> {
                    result.success(HydraVpnService.isRunning)
                }
                "resolveAppByConnection" -> {
                    val protocol = call.argument<Int>("protocol") ?: 6
                    val localIp = call.argument<String>("local_ip") ?: "0.0.0.0"
                    val localPort = call.argument<Int>("local_port") ?: 0
                    val remoteIp = call.argument<String>("remote_ip")
                    val remotePort = call.argument<Int>("remote_port")
                    if (remoteIp == null || remotePort == null) {
                        result.success(AppResolver.toJson(null))
                        return@setMethodCallHandler
                    }

                    appResolverExecutor.execute {
                        val appInfo = AppResolver.resolveByConnection(
                            protocol,
                            localIp,
                            localPort,
                            remoteIp,
                            remotePort,
                        )
                        Handler(Looper.getMainLooper()).post {
                            result.success(AppResolver.toJson(appInfo))
                        }
                    }
                }
                else -> {
                    result.notImplemented()
                }
            }
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        if (requestCode == VPN_REQUEST_CODE && resultCode == RESULT_OK) {
            val intent = Intent(this, HydraVpnService::class.java)
            intent.action = HydraVpnService.ACTION_CONNECT
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                startForegroundService(intent)
            } else {
                startService(intent)
            }
        }
        super.onActivityResult(requestCode, resultCode, data)
    }
}
