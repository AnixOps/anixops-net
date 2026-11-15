package io.anixops.net.platform

import android.Manifest
import android.annotation.SuppressLint
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.ContentValues
import android.content.Context
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.util.Base64
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import io.anixops.net.R
import io.anixops.net.core.Envelope
import io.anixops.net.core.PlatformBridge
import java.io.File
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.buildJsonObject
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

    override fun call(method: String, args: String): String = Envelope.answer {
        val arguments = Envelope.json.parseToJsonElement(args).jsonObject
        when (method) {
            "openTunnel" -> openTunnel(arguments)
            "closeTunnel" -> {
                AnxVpnService.close()
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
     * diagnostics reports and packet capture exports.
     */
    private fun saveToDownloads(arguments: JsonObject): JsonElement {
        val name = string(arguments, "name")
        val contents = string(arguments, "contents")
        val base64 = arguments["base64"]?.jsonPrimitive?.boolean ?: false
        val mimeType = arguments["mimeType"]?.jsonPrimitive?.content ?: "application/octet-stream"
        val bytes = if (base64) Base64.decode(contents, Base64.DEFAULT) else contents.toByteArray()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val values = ContentValues().apply {
                put(MediaStore.MediaColumns.DISPLAY_NAME, name)
                put(MediaStore.MediaColumns.MIME_TYPE, mimeType)
                put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
            }
            val resolver = context.contentResolver
            val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                ?: throw IllegalStateException("Downloads is not available")
            resolver.openOutputStream(uri)?.use { it.write(bytes) }
                ?: throw IllegalStateException("Downloads is not writable")
        } else {
            @Suppress("DEPRECATION")
            val folder = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
            File(folder, name).writeBytes(bytes)
        }
        return buildJsonObject { put("location", JsonPrimitive("${Environment.DIRECTORY_DOWNLOADS}/$name")) }
    }

    /**
     * The network the device is on, for the Wi-Fi rules: `kind` is wifi, cellular, other or offline, and `ssid`
     * the Wi-Fi name when Android gives it (location permission granted). The app is left out of its own VPN, so
     * the active network here is the one underneath. Asking for the permission is the activity's job.
     */
    private fun currentNetwork(): JsonElement {
        val connectivity = context.getSystemService(ConnectivityManager::class.java)
        val capabilities = connectivity?.activeNetwork?.let { connectivity.getNetworkCapabilities(it) }
        val kind = when {
            capabilities == null -> "offline"
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> "wifi"
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> "cellular"
            else -> "other"
        }
        return buildJsonObject {
            put("kind", JsonPrimitive(kind))
            if (kind == "wifi" && granted(Manifest.permission.ACCESS_FINE_LOCATION)) {
                @Suppress("DEPRECATION")
                val ssid = context.getSystemService(WifiManager::class.java)?.connectionInfo?.ssid
                ssid?.removeSurrounding("\"")?.takeIf { it.isNotEmpty() && it != "<unknown ssid>" }?.let {
                    put("ssid", JsonPrimitive(it))
                }
            }
        }
    }

    /** Script notifications, an unexpected disconnect, a failed subscription update. */
    // The notification permission is checked at the top; lint does not follow `allowed`.
    @SuppressLint("MissingPermission")
    private fun postNotice(arguments: JsonObject): JsonElement {
        val allowed = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED
        if (!allowed) {
            return Envelope.empty
        }
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                NOTICE_CHANNEL,
                context.getString(R.string.notification_channel_notices),
                NotificationManager.IMPORTANCE_DEFAULT,
            ),
        )
        val notification = NotificationCompat.Builder(context, NOTICE_CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(string(arguments, "title"))
            .setContentText(string(arguments, "body"))
            .setAutoCancel(true)
            .build()
        NotificationManagerCompat.from(context).notify(noticeIds.getAndIncrement(), notification)
        return Envelope.empty
    }

    private fun granted(permission: String): Boolean =
        ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED

    private companion object {
        const val DEFAULT_MTU = 1500
        const val NOTICE_CHANNEL = "notices"
        const val NOTICE_ID_START = 100
    }
}
