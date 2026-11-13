package io.anixops.net.vpn

import android.content.Intent
import android.net.VpnService
import android.os.Build

/**
 * Brings up the VPN interface and hands its descriptor to the engine (Rust), which reads and writes IP packets
 * on it and closes it on disconnect. The app itself is left out of the VPN, so the engine's own connections
 * to its servers go out directly.
 */
class AnxVpnService : VpnService() {
    companion object {
        const val ACTION_START = "io.anixops.net.vpn.START"

        private var pendingArgs: StartArgs? = null
        private var pendingCallback: ((Int?, String?) -> Unit)? = null
        @Volatile
        private var running: AnxVpnService? = null

        /** Remembers what the next start brings up and who hears the result. */
        @Synchronized
        fun request(args: StartArgs, callback: (Int?, String?) -> Unit) {
            pendingCallback?.invoke(null, "Another VPN start replaced this one")
            pendingArgs = args
            pendingCallback = callback
        }

        @Synchronized
        private fun takeRequest(): Pair<StartArgs, (Int?, String?) -> Unit>? {
            val args = pendingArgs ?: return null
            val callback = pendingCallback ?: return null
            pendingArgs = null
            pendingCallback = null
            return Pair(args, callback)
        }

        fun stopRunning() {
            running?.stopSelf()
        }
    }

    override fun onCreate() {
        super.onCreate()
        running = this
    }

    override fun onDestroy() {
        if (running === this) {
            running = null
        }
        super.onDestroy()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_START) {
            val (args, callback) = takeRequest() ?: return START_NOT_STICKY
            try {
                callback(establish(args), null)
            } catch (error: Exception) {
                callback(null, error.message ?: error.toString())
            }
        }
        return START_NOT_STICKY
    }

    private fun establish(args: StartArgs): Int {
        val builder = Builder()
            .setSession(args.session)
            .setMtu(args.mtu)
            .setBlocking(true)
        for (address in args.addresses) {
            val (ip, prefix) = cidr(address)
            builder.addAddress(ip, prefix)
        }
        for (route in args.routes) {
            val (ip, prefix) = cidr(route)
            builder.addRoute(ip, prefix)
        }
        for (server in args.dnsServers) {
            builder.addDnsServer(server)
        }
        builder.addDisallowedApplication(packageName)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            builder.setMetered(false)
        }
        val descriptor = builder.establish() ?: throw IllegalStateException("The VPN is no longer allowed")
        return descriptor.detachFd()
    }

    private fun cidr(value: String): Pair<String, Int> {
        val slash = value.lastIndexOf('/')
        require(slash > 0) { "Not a CIDR range: $value" }
        return Pair(value.substring(0, slash), value.substring(slash + 1).toInt())
    }

    /** Another VPN took over, or the user turned this one off in the system settings. */
    override fun onRevoke() {
        stopSelf()
    }
}
