package io.anixops.net.vpn

import android.app.Activity
import android.content.Intent
import android.net.VpnService
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

/**
 * Called from Rust (`tauri-plugin-anixops-vpn`). `start` asks for the VPN consent when the system has not
 * given it yet, then has [AnxVpnService] bring up the interface and resolves with its TUN descriptor.
 */
@TauriPlugin
class VpnPlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun start(invoke: Invoke) {
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

    @Command
    fun stop(invoke: Invoke) {
        AnxVpnService.stopRunning()
        invoke.resolve()
    }
}
