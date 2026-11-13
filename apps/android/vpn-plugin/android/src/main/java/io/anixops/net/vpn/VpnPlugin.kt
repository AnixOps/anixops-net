package io.anixops.net.vpn

import android.Manifest
import android.app.Activity
import android.content.ContentValues
import android.content.Intent
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import java.io.File
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/** The interface to bring up; the field names match the Rust `StartRequest` (camelCase). */
@InvokeArg
class StartArgs {
    var addresses: Array<String> = arrayOf()
    var routes: Array<String> = arrayOf()
    var dnsServers: Array<String> = arrayOf()
    var mtu: Int = 1500
    var session: String = "AnixOps Net"
}

/** A text file to put in the shared Downloads folder. */
@InvokeArg
class SaveArgs {
    var name: String = ""
    var contents: String = ""
}

/**
 * Called from Rust (`tauri-plugin-anixops-vpn`). `start` asks for the VPN consent when the system has not
 * given it yet, then has [AnxVpnService] bring up the interface and resolves with its TUN descriptor.
 */
@TauriPlugin
class VpnPlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun start(invoke: Invoke) {
        askForNotifications()
        val consent = VpnService.prepare(activity)
        if (consent != null) {
            startActivityForResult(invoke, consent, "consentResult")
            return
        }
        establish(invoke)
    }

    @ActivityCallback
    private fun consentResult(invoke: Invoke, result: ActivityResult) {
        if (result.resultCode == Activity.RESULT_OK) {
            establish(invoke)
        } else {
            invoke.reject("The VPN was not allowed")
        }
    }

    private fun establish(invoke: Invoke) {
        val args = invoke.parseArgs(StartArgs::class.java)
        AnxVpnService.request(args) { fd, error ->
            if (fd != null) {
                val result = JSObject()
                result.put("fd", fd)
                invoke.resolve(result)
            } else {
                invoke.reject(error ?: "The VPN could not start")
            }
        }
        activity.startService(Intent(activity, AnxVpnService::class.java).setAction(AnxVpnService.ACTION_START))
    }

    /** The "connected" notification needs this on Android 13+; without it the VPN still works, unannounced. */
    private fun askForNotifications() {
        if (Build.VERSION.SDK_INT >= 33 &&
            activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 0)
        }
    }

    /** Saves a file to Downloads (for the HTTPS decryption root, which the user installs in the system settings). */
    @Command
    fun saveToDownloads(invoke: Invoke) {
        val args = invoke.parseArgs(SaveArgs::class.java)
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                val values = ContentValues().apply {
                    put(MediaStore.MediaColumns.DISPLAY_NAME, args.name)
                    put(MediaStore.MediaColumns.MIME_TYPE, "application/x-x509-ca-cert")
                    put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
                }
                val resolver = activity.contentResolver
                val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                    ?: throw IllegalStateException("Downloads is not available")
                resolver.openOutputStream(uri)?.use { it.write(args.contents.toByteArray()) }
                    ?: throw IllegalStateException("Downloads is not writable")
            } else {
                @Suppress("DEPRECATION")
                val folder = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
                File(folder, args.name).writeText(args.contents)
            }
            val result = JSObject()
            result.put("location", "${Environment.DIRECTORY_DOWNLOADS}/${args.name}")
            invoke.resolve(result)
        } catch (error: Exception) {
            invoke.reject(error.message ?: error.toString())
        }
    }

    @Command
    fun stop(invoke: Invoke) {
        AnxVpnService.stopRunning()
        invoke.resolve()
    }
}
