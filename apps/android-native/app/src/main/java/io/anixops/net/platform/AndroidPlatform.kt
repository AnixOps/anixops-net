package io.anixops.net.platform

import android.Manifest
import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.MediaScannerConnection
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.Uri
import android.net.wifi.WifiInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.util.Base64
import androidx.annotation.RequiresApi
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import io.anixops.net.R
import io.anixops.net.core.Envelope
import io.anixops.net.core.PlatformBridge
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * What the Rust client asks of Android, answered in the [Envelope]. Called on Rust threads; nothing here touches
 * the UI. Method names and argument shapes match `apps/android-native/rust/src/lib.rs` (`BridgePlatform`).
 */
class AndroidPlatform(context: Context) : PlatformBridge {
    private val context = context.applicationContext
    private val sessionKeys = SessionKeys(this.context)
    private val noticeIds = AtomicInteger(NOTICE_ID_START)
    private val wifiNames = WifiNames()
    private val watchingWifi = AtomicBoolean(false)

    override fun call(method: String, args: String): String = Envelope.answer {
        val arguments = Envelope.json.parseToJsonElement(args).jsonObject
        when (method) {
            "openTunnel" -> openTunnel(arguments)
            "closeTunnel" -> {
                // With the Wi-Fi rules on, the service stays in the foreground to wait for the next network.
                AnxVpnService.close(waitForNetwork = flag(arguments, "waitForNetwork"))
                Envelope.empty
            }
            "saveToDownloads" -> saveToDownloads(arguments)
            "captureSessionKey" -> buildJsonObject { put("keyBase64", JsonPrimitive(sessionKeys.keyBase64())) }
            "currentNetwork" -> currentNetwork()
            "postNotice" -> postNotice(arguments)
            else -> throw IllegalArgumentException("Unknown platform call $method")
        }
    }

    private fun strings(arguments: JsonObject, name: String): List<String> =
        arguments[name]?.jsonArray?.map { it.jsonPrimitive.content }.orEmpty()

    private fun string(arguments: JsonObject, name: String): String =
        arguments[name]?.jsonPrimitive?.content ?: throw IllegalArgumentException("$name is missing")

    /** A string argument; null when missing or JSON null (whose `content` would read "null"). */
    private fun optionalString(arguments: JsonObject, name: String): String? =
        (arguments[name] as? JsonPrimitive)?.takeIf { it.isString }?.contentOrNull

    private fun flag(arguments: JsonObject, name: String): Boolean =
        (arguments[name] as? JsonPrimitive)?.booleanOrNull ?: false

    private fun openTunnel(arguments: JsonObject): JsonElement {
        val request = TunnelRequest(
            addresses = strings(arguments, "addresses"),
            routes = strings(arguments, "routes"),
            dnsServers = strings(arguments, "dnsServers"),
            mtu = arguments["mtu"]?.jsonPrimitive?.int ?: DEFAULT_MTU,
            session = arguments["session"]?.jsonPrimitive?.content ?: context.getString(R.string.app_name),
        )
        val fd = AnxVpnService.open(context, request)
        return buildJsonObject { put("fd", JsonPrimitive(fd)) }
    }

    /**
     * Saves a file to Downloads: the HTTPS decryption root, which the user installs in the system settings,
     * diagnostics reports and packet capture exports. Answers where the file really went: Downloads renames a file
     * whose name is taken ("name (1).crt"), and the user must find the one just saved. With `replace`, a file of the
     * same name this app saved earlier is overwritten instead (the root, saved each time decryption is turned on).
     */
    private fun saveToDownloads(arguments: JsonObject): JsonElement {
        val name = string(arguments, "name")
        val contents = string(arguments, "contents")
        val base64 = flag(arguments, "base64")
        val replace = flag(arguments, "replace")
        val mimeType = optionalString(arguments, "mimeType") ?: "application/octet-stream"
        val bytes = if (base64) Base64.decode(contents, Base64.DEFAULT) else contents.toByteArray()
        val saved = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            saveToMediaStore(name, mimeType, bytes, replace)
        } else {
            saveToFolder(name, bytes, replace)
        }
        return buildJsonObject { put("location", JsonPrimitive("${Environment.DIRECTORY_DOWNLOADS}/$saved")) }
    }

    /** Android 10 and later: through MediaStore, no permission needed. Answers the file's name as stored. */
    @RequiresApi(Build.VERSION_CODES.Q)
    private fun saveToMediaStore(name: String, mimeType: String, bytes: ByteArray, replace: Boolean): String {
        val resolver = context.contentResolver
        val collection = MediaStore.Downloads.EXTERNAL_CONTENT_URI
        // Only this app's own files are visible here, so a replace never touches another app's file (nor one an
        // earlier installation left, which a new root's different name keeps apart anyway).
        val existing = if (replace) {
            resolver.query(
                collection,
                arrayOf(MediaStore.MediaColumns._ID),
                "${MediaStore.MediaColumns.DISPLAY_NAME} = ? AND ${MediaStore.MediaColumns.RELATIVE_PATH} = ?",
                arrayOf(name, "${Environment.DIRECTORY_DOWNLOADS}/"),
                null,
            )?.use { cursor -> if (cursor.moveToFirst()) cursor.getLong(0) else null }
        } else {
            null
        }
        if (existing != null) {
            val uri = ContentUris.withAppendedId(collection, existing)
            // "wt": some Android versions do not truncate on a plain "w".
            resolver.openOutputStream(uri, "wt")?.use { it.write(bytes) }
                ?: throw IllegalStateException("Downloads is not writable")
            return name
        }
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, name)
            put(MediaStore.MediaColumns.MIME_TYPE, mimeType)
            put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val uri = resolver.insert(collection, values) ?: throw IllegalStateException("Downloads is not available")
        try {
            resolver.openOutputStream(uri)?.use { it.write(bytes) }
                ?: throw IllegalStateException("Downloads is not writable")
            resolver.update(uri, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null)
        } catch (error: Exception) {
            // An empty entry would keep the name taken and show as a broken file.
            runCatching { resolver.delete(uri, null, null) }
            throw error
        }
        return resolver.query(uri, arrayOf(MediaStore.MediaColumns.DISPLAY_NAME), null, null, null)
            ?.use { cursor -> if (cursor.moveToFirst()) cursor.getString(0) else null }
            ?: throw IllegalStateException("Downloads did not say where the file went")
    }

    /**
     * Android 8 and 9: straight into the public Downloads folder, which needs the storage permission (asked by the
     * page before it saves). A taken name gets a copy number, as Android 10 does, unless [replace].
     */
    private fun saveToFolder(name: String, bytes: ByteArray, replace: Boolean): String {
        if (!granted(Manifest.permission.WRITE_EXTERNAL_STORAGE)) {
            throw IllegalStateException(context.getString(R.string.downloads_permission_needed))
        }
        @Suppress("DEPRECATION")
        val folder = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
        folder.mkdirs()
        val file = File(folder, if (replace) name else freeName(name) { File(folder, it).exists() })
        file.writeBytes(bytes)
        // So the file manager and the certificate installer see it without waiting for a scan.
        MediaScannerConnection.scanFile(context, arrayOf(file.absolutePath), null, null)
        return file.name
    }

    /**
     * The network the device is on, for the Wi-Fi rules: `kind` is wifi, cellular, other or offline, `ssid` the
     * Wi-Fi name when known, and `ssidWithheld` when the app may read the name but Android hides it for now (the app
     * is in the background without the background location permission, or location is off). A name read on a
     * network is remembered for as long as the device stays on that network. The app is left out of its own VPN,
     * so the active network here is the one underneath. Asking for the permissions is the activity's job.
     */
    private fun currentNetwork(): JsonElement {
        val connectivity = context.getSystemService(ConnectivityManager::class.java)
        val active = connectivity?.activeNetwork
        val capabilities = active?.let { connectivity.getNetworkCapabilities(it) }
        val kind = when {
            capabilities == null -> "offline"
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> "wifi"
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> "cellular"
            else -> "other"
        }
        return buildJsonObject {
            put("kind", JsonPrimitive(kind))
            if (kind == "wifi" && active != null) {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && connectivity != null) watchWifiNames(connectivity)
                val mayRead = granted(Manifest.permission.ACCESS_FINE_LOCATION)
                val reported = if (mayRead) {
                    @Suppress("DEPRECATION")
                    context.getSystemService(WifiManager::class.java)?.connectionInfo?.ssid
                } else {
                    null
                }
                when (val name = wifiNames.read(active.networkHandle, reported, mayRead)) {
                    is WifiName.Known -> put("ssid", JsonPrimitive(name.ssid))
                    WifiName.Withheld -> put("ssidWithheld", JsonPrimitive(true))
                    WifiName.Unknown -> Unit
                }
            }
        }
    }

    /**
     * Android 12 and later: follows the default network with its location info, so the Wi-Fi name is learned as soon
     * as Android lets the app see it (when the network comes up while the app may read it), not only when the
     * rules sample.
     */
    @RequiresApi(Build.VERSION_CODES.S)
    private fun watchWifiNames(connectivity: ConnectivityManager) {
        if (!watchingWifi.compareAndSet(false, true)) return
        val flags = ConnectivityManager.NetworkCallback.FLAG_INCLUDE_LOCATION_INFO
        val callback = object : ConnectivityManager.NetworkCallback(flags) {
            override fun onCapabilitiesChanged(network: Network, capabilities: NetworkCapabilities) {
                val info = capabilities.transportInfo as? WifiInfo ?: return
                wifiNames.learn(network.networkHandle, info.ssid)
            }
        }
        try {
            connectivity.registerDefaultNetworkCallback(callback)
        } catch (error: RuntimeException) {
            // Too many callbacks, or the service is gone: the rules still read the name when they sample.
            watchingWifi.set(false)
        }
    }

    /**
     * Script notifications, an unexpected disconnect, a failed subscription update, the Wi-Fi rules failing to
     * connect. A tap opens the link a script attached (http or https only) or else the app; the whole text shows
     * when expanded; a newer connection or subscription notice replaces the older one.
     */
    // The notification permission is checked at the top; lint does not follow `allowed`.
    @SuppressLint("MissingPermission")
    private fun postNotice(arguments: JsonObject): JsonElement {
        val allowed = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED
        if (!allowed) {
            return Envelope.empty
        }
        Channels.ensure(context)
        val slot = noticeSlot(optionalString(arguments, "kind")) { noticeIds.getAndIncrement() }
        val body = string(arguments, "body")
        val target = noticeLink(optionalString(arguments, "openUrl"))?.let { link ->
            Intent(Intent.ACTION_VIEW, Uri.parse(link)).addCategory(Intent.CATEGORY_BROWSABLE)
        } ?: context.packageManager.getLaunchIntentForPackage(context.packageName)
        val open = target?.let {
            PendingIntent.getActivity(
                context,
                slot.id,
                it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        }
        val notification = NotificationCompat.Builder(context, Channels.NOTICES)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(string(arguments, "title"))
            .setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setContentIntent(open)
            .setAutoCancel(true)
            .build()
        NotificationManagerCompat.from(context).notify(slot.tag, slot.id, notification)
        return Envelope.empty
    }

    private fun granted(permission: String): Boolean =
        ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED

    private companion object {
        const val DEFAULT_MTU = 1500
        const val NOTICE_ID_START = 100
    }
}
