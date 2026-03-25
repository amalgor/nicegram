package com.hydra.network.hydra_mobile

import android.content.Intent
import android.net.VpnService
import androidx.annotation.NonNull
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import android.os.Handler
import android.os.Looper

class MainActivity: FlutterActivity() {
    private val CHANNEL = "com.hydra.network/vpn"
    private val VPN_REQUEST_CODE = 0x0F
    private var methodChannel: MethodChannel? = null

    override fun configureFlutterEngine(@NonNull flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        
        methodChannel = MethodChannel(flutterEngine.dartExecutor.binaryMessenger, CHANNEL)
        
        HydraVpnService.onVpnStarted = { fd ->
            Handler(Looper.getMainLooper()).post {
                methodChannel?.invokeMethod("onVpnStarted", fd)
            }
        }
        
        methodChannel?.setMethodCallHandler { call, result ->
            if (call.method == "startVpn") {
                val intent = VpnService.prepare(this)
                if (intent != null) {
                    startActivityForResult(intent, VPN_REQUEST_CODE)
                } else {
                    onActivityResult(VPN_REQUEST_CODE, RESULT_OK, null)
                }
                result.success(true)
            } else if (call.method == "stopVpn") {
                val intent = Intent(this, HydraVpnService::class.java)
                intent.action = HydraVpnService.ACTION_DISCONNECT
                startService(intent)
                result.success(true)
            } else if (call.method == "getVpnFd") {
                result.success(HydraVpnService.currentFd)
            } else {
                result.notImplemented()
            }
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        if (requestCode == VPN_REQUEST_CODE && resultCode == RESULT_OK) {
            val intent = Intent(this, HydraVpnService::class.java)
            intent.action = HydraVpnService.ACTION_CONNECT
            startService(intent)
        }
        super.onActivityResult(requestCode, resultCode, data)
    }
}
