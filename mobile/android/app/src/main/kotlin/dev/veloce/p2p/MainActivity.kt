package dev.veloce.p2p

import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.provider.MediaStore
import android.webkit.MimeTypeMap
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File
import java.net.Inet4Address
import java.net.Inet6Address
import java.util.concurrent.Executors

class MainActivity : FlutterActivity() {
    private val channelName = "dev.veloce.p2p/platform"
    private var multicastLock: WifiManager.MulticastLock? = null
    private val ioExecutor = Executors.newSingleThreadExecutor()
    private val mainHandler = Handler(Looper.getMainLooper())

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
                    "exportToPublicDownloads" -> {
                        val paths = call.argument<List<String>>("paths") ?: emptyList()
                        ioExecutor.execute {
                            val exported = exportFilesToPublicDownloads(paths)
                            mainHandler.post { result.success(exported) }
                        }
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
     * Copies completed sandboxed transfer files out of app-private storage into the
     * user-visible Downloads/Veloce directory via MediaStore.Downloads (Android 10+)
     * or public Downloads (Android 7–9), removing the sandbox copy only after verified write.
     */
    private fun exportFilesToPublicDownloads(paths: List<String>): List<String> {
        val results = mutableListOf<String>()
        for (rawPath in paths) {
            val srcFile = File(rawPath)
            if (!srcFile.exists() || !srcFile.isFile) {
                results.add(rawPath)
                continue
            }
            val exportedPath = try {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    exportViaMediaStore(srcFile)
                } else {
                    exportViaLegacyDownloads(srcFile)
                }
            } catch (_: Exception) {
                null
            }
            if (exportedPath != null) {
                srcFile.delete()
                results.add(exportedPath)
            } else {
                // Keep the verified sandbox file intact if public export fails.
                results.add(rawPath)
            }
        }
        return results
    }

    private fun exportViaMediaStore(srcFile: File): String? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return null
        val resolver = applicationContext.contentResolver
        val mimeType = guessMimeType(srcFile.name)
        val values = ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, srcFile.name)
            put(MediaStore.Downloads.MIME_TYPE, mimeType)
            put(
                MediaStore.Downloads.RELATIVE_PATH,
                "${Environment.DIRECTORY_DOWNLOADS}/Veloce"
            )
            put(MediaStore.Downloads.IS_PENDING, 1)
        }

        val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
            ?: return null

        try {
            resolver.openOutputStream(uri)?.use { out ->
                srcFile.inputStream().use { input ->
                    input.copyTo(out, bufferSize = 256 * 1024)
                }
                out.flush()
            } ?: run {
                resolver.delete(uri, null, null)
                return null
            }

            val publishValues = ContentValues().apply {
                put(MediaStore.Downloads.IS_PENDING, 0)
            }
            resolver.update(uri, publishValues, null, null)
            return "${Environment.DIRECTORY_DOWNLOADS}/Veloce/${srcFile.name}"
        } catch (err: Exception) {
            resolver.delete(uri, null, null)
            throw err
        }
    }

    @Suppress("DEPRECATION")
    private fun exportViaLegacyDownloads(srcFile: File): String? {
        val publicRoot = Environment.getExternalStoragePublicDirectory(
            Environment.DIRECTORY_DOWNLOADS
        ) ?: return null
        val targetDir = File(publicRoot, "Veloce")
        if (!targetDir.exists() && !targetDir.mkdirs()) return null
        val destFile = resolveUniqueFile(targetDir, srcFile.name)
        srcFile.inputStream().use { input ->
            destFile.outputStream().use { out ->
                input.copyTo(out, bufferSize = 256 * 1024)
                out.flush()
            }
        }
        return destFile.absolutePath
    }

    private fun resolveUniqueFile(dir: File, filename: String): File {
        var candidate = File(dir, filename)
        if (!candidate.exists()) return candidate
        val dot = filename.lastIndexOf('.')
        val stem = if (dot > 0) filename.substring(0, dot) else filename
        val ext = if (dot > 0) filename.substring(dot) else ""
        var counter = 1
        while (candidate.exists()) {
            candidate = File(dir, "$stem ($counter)$ext")
            counter++
        }
        return candidate
    }

    private fun guessMimeType(filename: String): String {
        val ext = filename.substringAfterLast('.', "").lowercase()
        if (ext.isEmpty()) return "application/octet-stream"
        return MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext)
            ?: "application/octet-stream"
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
        ioExecutor.shutdown()
        super.onDestroy()
    }
}
