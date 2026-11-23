package dev.veloce.p2p

import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager
import android.os.Build
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.net.Inet4Address
import java.net.Inet6Address

class MainActivity : FlutterActivity() {
    private val channelName = "dev.veloce.p2p/platform"
    private var multicastLock: WifiManager.MulticastLock? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, channelName)
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "acquireMulticastLock" -> {
                        acquireMulticastLock()
                        result.success(null)
                    }
                    "releaseMulticastLock" -> {
                        releaseMulticastLock()
                        result.success(null)
                    }
                    "startForegroundService" -> {
                        val title = call.argument<String>("title") ?: "Veloce Transfer"
                        val body = call.argument<String>("body") ?: "Transfer in progress"
                        startTransferForegroundService(title, body)
                        result.success(null)
                    }
                    "stopForegroundService" -> {
                        stopTransferForegroundService()
                        result.success(null)
                    }
                    "getLocalWifiAddresses" -> {
                        result.success(queryActiveWifiAddresses())
                    }
                    else -> result.notImplemented()
                }
            }
    }

    private fun acquireMulticastLock() {
        val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
            ?: return
        if (multicastLock == null) {
            multicastLock = wifi.createMulticastLock("veloce-mdns-lock").apply {
                setReferenceCounted(false)
            }
        }
        if (multicastLock?.isHeld == false) {
            multicastLock?.acquire()
        }
    }

    private fun releaseMulticastLock() {
        if (multicastLock?.isHeld == true) {
            multicastLock?.release()
        }
    }

    private fun startTransferForegroundService(title: String, body: String) {
        val intent = Intent(this, VeloceTransferService::class.java).apply {
            action = VeloceTransferService.ACTION_START
            putExtra(VeloceTransferService.EXTRA_TITLE, title)
            putExtra(VeloceTransferService.EXTRA_BODY, body)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
    }

    private fun stopTransferForegroundService() {
        val intent = Intent(this, VeloceTransferService::class.java).apply {
            action = VeloceTransferService.ACTION_STOP
        }
        startService(intent)
    }

    /**
     * Queries active Wi-Fi/Ethernet link IP addresses via Android ConnectivityManager so
     * Android 11+ (API 30+) devices reliably advertise physical LAN IPs even when
     * SELinux restricts RTM_GETLINK netlink enumeration.
     */
    private fun queryActiveWifiAddresses(): List<String> {
        val cm = applicationContext.getSystemService(Context.CONNECTIVITY_SERVICE)
            as? ConnectivityManager ?: return emptyList()
        val results = mutableListOf<String>()
        val activeNetwork = cm.activeNetwork ?: return emptyList()
        val caps = cm.getNetworkCapabilities(activeNetwork) ?: return emptyList()
        val isLanTransport = caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ||
            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
        if (!isLanTransport) return emptyList()

        val linkProps = cm.getLinkProperties(activeNetwork) ?: return emptyList()
        for (linkAddr in linkProps.linkAddresses) {
            val addr = linkAddr.address ?: continue
            if (addr.isLoopbackAddress || addr.isLinkLocalAddress || addr.isMulticastAddress) {
                continue
            }
            val host = addr.hostAddress?.substringBefore('%') ?: continue
            if (addr is Inet4Address) {
                results.add(0, host)
            } else if (addr is Inet6Address) {
                results.add(host)
            }
        }
        return results.distinct()
    }

    override fun onDestroy() {
        releaseMulticastLock()
        super.onDestroy()
    }
}
