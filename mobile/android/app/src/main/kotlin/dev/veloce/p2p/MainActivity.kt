package dev.veloce.p2p

import android.app.Activity
import android.app.DownloadManager
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.provider.Settings
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
    private val requestCodePickFolder = 4091
    private var multicastLock: WifiManager.MulticastLock? = null
    private val ioExecutor = Executors.newSingleThreadExecutor()
    private val mainHandler = Handler(Looper.getMainLooper())
    private var pendingFolderResult: MethodChannel.Result? = null

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
                    "getDeviceModelName" -> {
                        result.success(resolveDeviceModelName())
                    }
                    "pickCustomSaveFolder" -> {
                        launchCustomFolderPicker(result)
                    }
                    "releaseCustomSaveFolder" -> {
                        val uriStr = call.argument<String>("uri")
                        releasePersistedFolderGrant(uriStr)
                        result.success(null)
                    }
                    "exportToPublicDownloads" -> {
                        val paths = call.argument<List<String>>("paths") ?: emptyList()
                        val customTreeUri = call.argument<String>("customTreeUri")
                        val customDisplayPath = call.argument<String>("customDisplayPath")
                        val stagingRoot = call.argument<String>("stagingRoot")
                        ioExecutor.execute {
                            val exported = exportCompletedFiles(
                                paths = paths,
                                customTreeUri = customTreeUri,
                                customDisplayPath = customDisplayPath,
                                stagingRoot = stagingRoot,
                            )
                            mainHandler.post { result.success(exported.map { it.displayPath }) }
                        }
                    }
                    "exportToPublicDownloadsWithUris" -> {
                        val paths = call.argument<List<String>>("paths") ?: emptyList()
                        val customTreeUri = call.argument<String>("customTreeUri")
                        val customDisplayPath = call.argument<String>("customDisplayPath")
                        val stagingRoot = call.argument<String>("stagingRoot")
                        ioExecutor.execute {
                            val exported = exportCompletedFiles(
                                paths = paths,
                                customTreeUri = customTreeUri,
                                customDisplayPath = customDisplayPath,
                                stagingRoot = stagingRoot,
                            )
                            val mapped = exported.map {
                                mapOf(
                                    "displayPath" to it.displayPath,
                                    "openUri" to it.openUri,
                                )
                            }
                            mainHandler.post { result.success(mapped) }
                        }
                    }
                    "openFile" -> {
                        val uriStr = call.argument<String>("uri")
                        val displayName = call.argument<String>("displayName")
                        result.success(openExportedFile(uriStr, displayName))
                    }
                    "openFolder" -> {
                        val treeUri = call.argument<String>("treeUri")
                        result.success(openDestinationFolder(treeUri))
                    }
                    "openExternalUrl" -> {
                        val url = call.argument<String>("url")
                        result.success(openHttpUrl(url))
                    }
                    else -> result.notImplemented()
                }
            }
    }

    private fun resolveDeviceModelName(): String {
        val resolver = applicationContext.contentResolver
        val genericNames = setOf(
            "android",
            "localhost",
            "unknown",
            "generic",
            "device",
            "phone",
            "mobile",
            "veloce android",
            "veloce mobile",
        )

        fun isUsefulName(raw: String?): Boolean {
            val cleaned = raw?.trim() ?: return false
            if (cleaned.isEmpty()) return false
            val lower = cleaned.lowercase()
            if (lower in genericNames) return false
            if (lower.startsWith("sdk_gphone") || lower.startsWith("generic_")) return false
            return true
        }

        // 1. Prefer Android's user/OEM friendly device name (e.g., "Galaxy S24 Ultra", "Pixel 8 Pro",
        //    "Redmi Note 13 Pro", or whatever custom name the user gave their phone in Android Settings).
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N_MR1) {
            try {
                val globalName = Settings.Global.getString(resolver, Settings.Global.DEVICE_NAME)
                if (isUsefulName(globalName)) {
                    return globalName.trim().take(64)
                }
            } catch (_: Exception) {
                // Ignore restricted OEM builds.
            }
        }

        // 2. Fallback to Bluetooth/marketing device name setting when readable without extra permissions.
        try {
            val btName = Settings.Secure.getString(resolver, "bluetooth_name")
            if (isUsefulName(btName)) {
                return btName.trim().take(64)
            }
        } catch (_: Exception) {
            // Ignore restricted settings on newer Android releases.
        }

        // 3. Fallback to Build.MANUFACTURER + Build.MODEL cleanly formatted.
        val rawManufacturer = Build.MANUFACTURER?.trim().orEmpty()
        val rawModel = Build.MODEL?.trim().orEmpty()
        val manufacturer = when (rawManufacturer.lowercase()) {
            "samsung" -> "Samsung"
            "google" -> "Google"
            "xiaomi" -> "Xiaomi"
            "redmi" -> "Redmi"
            "poco" -> "POCO"
            "oneplus" -> "OnePlus"
            "oppo" -> "OPPO"
            "vivo" -> "vivo"
            "realme" -> "realme"
            "motorola" -> "Motorola"
            "nothing" -> "Nothing"
            "sony" -> "Sony"
            "asus" -> "ASUS"
            "huawei" -> "HUAWEI"
            "honor" -> "HONOR"
            "lenovo" -> "Lenovo"
            "nokia", "hmd global" -> "Nokia"
            else -> rawManufacturer.replaceFirstChar { ch ->
                if (ch.isLowerCase()) ch.titlecase() else ch.toString()
            }
        }

        val combined = when {
            rawModel.isEmpty() -> manufacturer
            manufacturer.isEmpty() -> rawModel
            rawModel.lowercase().startsWith(manufacturer.lowercase()) -> rawModel
            rawModel.lowercase().startsWith("pixel ") && manufacturer.equals("Google", ignoreCase = true) ->
                rawModel
            else -> "$manufacturer $rawModel"
        }.trim()

        return if (combined.isNotEmpty()) combined.take(64) else "Android Device"
    }

    private fun launchCustomFolderPicker(result: MethodChannel.Result) {
        pendingFolderResult?.success(null)
        pendingFolderResult = result
        try {
            val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
                addFlags(
                    Intent.FLAG_GRANT_READ_URI_PERMISSION or
                        Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                        Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION or
                        Intent.FLAG_GRANT_PREFIX_URI_PERMISSION
                )
            }
            @Suppress("DEPRECATION")
            startActivityForResult(intent, requestCodePickFolder)
        } catch (_: Exception) {
            pendingFolderResult = null
            result.success(null)
        }
    }

    private fun releasePersistedFolderGrant(uriStr: String?) {
        if (uriStr.isNullOrBlank()) return
        try {
            val uri = Uri.parse(uriStr)
            val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            applicationContext.contentResolver.releasePersistableUriPermission(uri, flags)
        } catch (_: Exception) {
            // Ignore if permission was already revoked.
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        if (requestCode == requestCodePickFolder) {
            val callback = pendingFolderResult
            pendingFolderResult = null
            if (callback != null) {
                val treeUri = if (resultCode == Activity.RESULT_OK) data?.data else null
                if (treeUri != null) {
                    val persistFlags = (data?.flags ?: 0) and
                        (Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
                    val effectiveFlags = if (persistFlags != 0) {
                        persistFlags
                    } else {
                        Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                    }
                    try {
                        applicationContext.contentResolver.takePersistableUriPermission(
                            treeUri,
                            effectiveFlags
                        )
                    } catch (_: Exception) {
                        // Some OEM file providers grant tree access without persistable flag.
                    }
                    callback.success(
                        mapOf(
                            "uri" to treeUri.toString(),
                            "displayPath" to formatTreeUriDisplayName(treeUri),
                        )
                    )
                } else {
                    callback.success(null)
                }
            }
            return
        }
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
    }

    private fun formatTreeUriDisplayName(treeUri: Uri): String {
        return try {
            val docId = DocumentsContract.getTreeDocumentId(treeUri)
            val colonIdx = docId.indexOf(':')
            if (colonIdx >= 0) {
                val volume = docId.substring(0, colonIdx)
                val rel = docId.substring(colonIdx + 1).trim('/')
                if (volume.equals("primary", ignoreCase = true)) {
                    when {
                        rel.isEmpty() -> "Internal Storage"
                        rel.equals("Download", ignoreCase = true) ||
                            rel.equals("Downloads", ignoreCase = true) -> "Downloads"
                        rel.startsWith("Download/", ignoreCase = true) ->
                            "Downloads/${rel.substringAfter('/')}"
                        else -> "Internal Storage/$rel"
                    }
                } else {
                    if (rel.isEmpty()) "External Storage ($volume)" else "External Storage/$rel"
                }
            } else if (docId.isNotBlank()) {
                docId
            } else {
                treeUri.lastPathSegment ?: "Custom Folder"
            }
        } catch (_: Exception) {
            treeUri.lastPathSegment ?: "Custom Folder"
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

    private data class ExportedFileRecord(
        val displayPath: String,
        val openUri: String,
    )

    private fun openExportedFile(uriStr: String?, displayName: String?): Boolean {
        val cleaned = uriStr?.trim().orEmpty()
        if (!cleaned.startsWith("content://")) return false
        return try {
            val uri = Uri.parse(cleaned)
            val resolvedMime = applicationContext.contentResolver.getType(uri)
                ?: displayName?.let { guessMimeType(it) }
                ?: "*/*"
            val intent = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, resolvedMime)
                addFlags(
                    Intent.FLAG_GRANT_READ_URI_PERMISSION or
                        Intent.FLAG_ACTIVITY_NEW_TASK
                )
            }
            try {
                startActivity(intent)
                true
            } catch (_: Exception) {
                val fallback = Intent(Intent.ACTION_VIEW).apply {
                    setDataAndType(uri, "*/*")
                    addFlags(
                        Intent.FLAG_GRANT_READ_URI_PERMISSION or
                            Intent.FLAG_ACTIVITY_NEW_TASK
                    )
                }
                startActivity(Intent.createChooser(fallback, "Open with"))
                true
            }
        } catch (_: Exception) {
            false
        }
    }

    private fun openDestinationFolder(treeUriStr: String?): Boolean {
        val cleaned = treeUriStr?.trim().orEmpty()
        if (cleaned.startsWith("content://")) {
            try {
                val treeUri = Uri.parse(cleaned)
                val docId = DocumentsContract.getTreeDocumentId(treeUri)
                val docUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, docId)
                val intent = Intent(Intent.ACTION_VIEW).apply {
                    setDataAndType(docUri, DocumentsContract.Document.MIME_TYPE_DIR)
                    addFlags(
                        Intent.FLAG_GRANT_READ_URI_PERMISSION or
                            Intent.FLAG_ACTIVITY_NEW_TASK
                    )
                }
                startActivity(intent)
                return true
            } catch (_: Exception) {
                // Fall back to system Downloads viewer below.
            }
        }
        return try {
            val downloadsIntent = Intent(DownloadManager.ACTION_VIEW_DOWNLOADS).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            startActivity(downloadsIntent)
            true
        } catch (_: Exception) {
            false
        }
    }

    private fun openHttpUrl(rawUrl: String?): Boolean {
        val cleaned = rawUrl?.trim().orEmpty()
        if (
            !cleaned.startsWith("http://", ignoreCase = true) &&
            !cleaned.startsWith("https://", ignoreCase = true)
        ) {
            return false
        }
        return try {
            val intent = Intent(Intent.ACTION_VIEW, Uri.parse(cleaned)).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            startActivity(intent)
            true
        } catch (_: Exception) {
            false
        }
    }

    /**
     * Copies completed sandboxed transfer files out of app-private storage into either:
     * 1. The user's chosen Android SAF custom folder (`customTreeUri`, preserving subdirectories), or
     * 2. User-visible Downloads/Veloce via MediaStore.Downloads (Android 10+) / legacy Downloads,
     * removing the temporary sandbox copy only after a verified write.
     */
    private fun exportCompletedFiles(
        paths: List<String>,
        customTreeUri: String?,
        customDisplayPath: String?,
        stagingRoot: String?,
    ): List<ExportedFileRecord> {
        val parsedTree = if (!customTreeUri.isNullOrBlank()) {
            try {
                Uri.parse(customTreeUri)
            } catch (_: Exception) {
                null
            }
        } else {
            null
        }
        val treeLabel = customDisplayPath?.takeIf { it.isNotBlank() }
            ?: parsedTree?.let { formatTreeUriDisplayName(it) }
            ?: "Custom Folder"

        val results = mutableListOf<ExportedFileRecord>()
        for (rawPath in paths) {
            val srcFile = File(rawPath)
            if (!srcFile.exists() || !srcFile.isFile) {
                results.add(ExportedFileRecord(rawPath, rawPath))
                continue
            }

            val exportedRecord = if (parsedTree != null) {
                val safResult = try {
                    exportViaSafTree(srcFile, parsedTree, treeLabel, stagingRoot)
                } catch (_: Exception) {
                    null
                }
                safResult ?: try {
                    exportToDefaultDownloads(srcFile)
                } catch (_: Exception) {
                    null
                }
            } else {
                try {
                    exportToDefaultDownloads(srcFile)
                } catch (_: Exception) {
                    null
                }
            }

            if (exportedRecord != null) {
                srcFile.delete()
                results.add(exportedRecord)
            } else {
                // Keep the verified sandbox file intact if export fails.
                results.add(ExportedFileRecord(rawPath, rawPath))
            }
        }
        return results
    }

    private fun exportToDefaultDownloads(srcFile: File): ExportedFileRecord? {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            exportViaMediaStore(srcFile)
        } else {
            exportViaLegacyDownloads(srcFile)
        }
    }

    private fun exportViaSafTree(
        srcFile: File,
        treeUri: Uri,
        treeLabel: String,
        stagingRoot: String?,
    ): ExportedFileRecord? {
        val resolver = applicationContext.contentResolver
        val rootDocId = DocumentsContract.getTreeDocumentId(treeUri)
        var targetParentDocId = rootDocId
        val subDirs = extractRelativeSubdirectories(srcFile, stagingRoot)

        for (dirName in subDirs) {
            val nextDocId = findOrCreateSafDirectory(treeUri, targetParentDocId, dirName)
                ?: return null
            targetParentDocId = nextDocId
        }

        val existingNames = listSafChildNames(treeUri, targetParentDocId)
        val uniqueName = resolveUniqueName(existingNames, srcFile.name)
        val parentDocUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, targetParentDocId)
        val mimeType = guessMimeType(uniqueName)
        val createdUri = DocumentsContract.createDocument(
            resolver,
            parentDocUri,
            mimeType,
            uniqueName
        ) ?: return null

        try {
            resolver.openOutputStream(createdUri)?.use { out ->
                srcFile.inputStream().use { input ->
                    input.copyTo(out, bufferSize = 256 * 1024)
                }
                out.flush()
            } ?: run {
                try {
                    DocumentsContract.deleteDocument(resolver, createdUri)
                } catch (_: Exception) {}
                return null
            }
            val relPrefix = if (subDirs.isEmpty()) "" else "${subDirs.joinToString("/")}/"
            return ExportedFileRecord(
                displayPath = "$treeLabel/$relPrefix$uniqueName",
                openUri = createdUri.toString(),
            )
        } catch (err: Exception) {
            try {
                DocumentsContract.deleteDocument(resolver, createdUri)
            } catch (_: Exception) {}
            throw err
        }
    }

    private fun extractRelativeSubdirectories(srcFile: File, stagingRoot: String?): List<String> {
        if (stagingRoot.isNullOrBlank()) return emptyList()
        return try {
            val rootCanonical = File(stagingRoot).canonicalPath.trimEnd(File.separatorChar)
            val parentCanonical = srcFile.canonicalFile.parentFile?.canonicalPath?.trimEnd(File.separatorChar)
                ?: return emptyList()
            val prefix = "$rootCanonical${File.separatorChar}"
            if (!parentCanonical.startsWith(prefix)) {
                return emptyList()
            }
            val rel = parentCanonical.substring(prefix.length)
            rel.split(File.separatorChar)
                .filter { it.isNotBlank() && it != "." && it != ".." }
        } catch (_: Exception) {
            emptyList()
        }
    }

    private fun findOrCreateSafDirectory(
        treeUri: Uri,
        parentDocId: String,
        dirName: String,
    ): String? {
        val resolver = applicationContext.contentResolver
        val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentDocId)
        val projection = arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
        )
        resolver.query(childrenUri, projection, null, null, null)?.use { cursor ->
            while (cursor.moveToNext()) {
                val id = cursor.getString(0)
                val name = cursor.getString(1)
                val mime = cursor.getString(2)
                if (name == dirName && mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                    return id
                }
            }
        }
        val parentUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, parentDocId)
        val created = DocumentsContract.createDocument(
            resolver,
            parentUri,
            DocumentsContract.Document.MIME_TYPE_DIR,
            dirName
        ) ?: return null
        return DocumentsContract.getDocumentId(created)
    }

    private fun listSafChildNames(treeUri: Uri, parentDocId: String): Set<String> {
        val names = mutableSetOf<String>()
        val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentDocId)
        val projection = arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME)
        try {
            applicationContext.contentResolver.query(childrenUri, projection, null, null, null)
                ?.use { cursor ->
                    while (cursor.moveToNext()) {
                        cursor.getString(0)?.let { names.add(it) }
                    }
                }
        } catch (_: Exception) {
            // Proceed even if directory listing is restricted.
        }
        return names
    }

    private fun resolveUniqueName(existingNames: Set<String>, filename: String): String {
        if (!existingNames.contains(filename)) return filename
        val dot = filename.lastIndexOf('.')
        val stem = if (dot > 0) filename.substring(0, dot) else filename
        val ext = if (dot > 0) filename.substring(dot) else ""
        var counter = 1
        var candidate = "$stem ($counter)$ext"
        while (existingNames.contains(candidate)) {
            counter++
            candidate = "$stem ($counter)$ext"
        }
        return candidate
    }

    private fun exportViaMediaStore(srcFile: File): ExportedFileRecord? {
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
            return ExportedFileRecord(
                displayPath = "${Environment.DIRECTORY_DOWNLOADS}/Veloce/${srcFile.name}",
                openUri = uri.toString(),
            )
        } catch (err: Exception) {
            resolver.delete(uri, null, null)
            throw err
        }
    }

    @Suppress("DEPRECATION")
    private fun exportViaLegacyDownloads(srcFile: File): ExportedFileRecord? {
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
        return ExportedFileRecord(
            displayPath = destFile.absolutePath,
            openUri = destFile.absolutePath,
        )
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
     * Queries active Wi-Fi, Ethernet, Hotspot, global Cellular IPv6, and mesh VPN (Tailscale/ZeroTier)
     * IP addresses via ConnectivityManager (`allNetworks` so simultaneous cellular or VPN connections
     * do not hide Wi-Fi) plus `NetworkInterface` fallback for Android Hotspot (`ap0`, `wlan*`, `rndis*`).
     *
     * Rust separates private LAN IPs (for mDNS) from global IPv6 and mesh VPN IPs (for global tickets).
     */
    private fun queryActiveWifiAddresses(): List<String> {
        val lanIpv4s = mutableListOf<String>()
        val globalIpv6s = mutableListOf<String>()
        val vpnAddrs = mutableListOf<String>()
        val cm = applicationContext.getSystemService(Context.CONNECTIVITY_SERVICE)
            as? ConnectivityManager

        if (cm != null) {
            for (network in cm.allNetworks) {
                val caps = cm.getNetworkCapabilities(network) ?: continue
                val isVpn = caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)
                val isLanTransport = caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ||
                    caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
                val isCellular = caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR)
                if (!isLanTransport && !isCellular && !isVpn) continue

                val linkProps = cm.getLinkProperties(network) ?: continue
                for (linkAddr in linkProps.linkAddresses) {
                    val addr = linkAddr.address ?: continue
                    if (addr.isLoopbackAddress || addr.isLinkLocalAddress || addr.isMulticastAddress) {
                        continue
                    }
                    val host = addr.hostAddress?.substringBefore('%') ?: continue
                    when {
                        isVpn -> vpnAddrs.add(host)
                        isLanTransport && addr is Inet4Address -> lanIpv4s.add(host)
                        addr is Inet6Address -> {
                            // Include global unicast IPv6 (2000::/3) across Wi-Fi & 4G/5G cellular
                            // so devices bypass IPv4 CGNAT with direct P2P IPv6 at $0 cost.
                            val firstByte = addr.address[0].toInt() and 0xFF
                            val isUniqueLocal = (firstByte and 0xFE) == 0xFC
                            if (!isUniqueLocal || isLanTransport) {
                                globalIpv6s.add(host)
                            }
                        }
                    }
                }
            }
        }

        val results = mutableListOf<String>()
        results.addAll(lanIpv4s)
        results.addAll(globalIpv6s)
        results.addAll(vpnAddrs)

        // Fallback for Android Wi-Fi Hotspot / tethering interfaces (ap0, swlan0, wlan1, rndis0)
        // which may not appear as a client network in ConnectivityManager.allNetworks.
        try {
            val ifaces = java.net.NetworkInterface.getNetworkInterfaces()
            while (ifaces != null && ifaces.hasMoreElements()) {
                val iface = ifaces.nextElement()
                if (!iface.isUp || iface.isLoopback) continue
                val name = iface.name.lowercase()
                val isHotspotOrLan = name.startsWith("wlan") ||
                    name.startsWith("ap") ||
                    name.startsWith("swlan") ||
                    name.startsWith("eth") ||
                    name.startsWith("rndis")
                if (!isHotspotOrLan) continue
                val addrs = iface.inetAddresses
                while (addrs.hasMoreElements()) {
                    val addr = addrs.nextElement()
                    if (addr.isLoopbackAddress || addr.isLinkLocalAddress || addr.isMulticastAddress) {
                        continue
                    }
                    val host = addr.hostAddress?.substringBefore('%') ?: continue
                    if (addr is Inet4Address && addr.isSiteLocalAddress) {
                        if (!results.contains(host)) {
                            results.add(host)
                        }
                    }
                }
            }
        } catch (_: Exception) {
            // Ignore interface enumeration restrictions on locked-down OEM builds.
        }

        return results.distinct()
    }

    override fun onDestroy() {
        releaseMulticastLock()
        ioExecutor.shutdown()
        super.onDestroy()
    }
}
