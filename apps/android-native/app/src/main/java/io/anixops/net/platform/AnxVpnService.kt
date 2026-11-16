package io.anixops.net.platform

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import io.anixops.net.AnixApplication
import io.anixops.net.R
import io.anixops.net.core.ConnectionState
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.launch

/**
 * Whether the system, not the app, started the service: the always-on VPN's start carries
 * [VpnService.SERVICE_INTERFACE], and a restart after the process died carries no intent at all.
 */
fun alwaysOnStart(action: String?): Boolean = action == null || action == VpnService.SERVICE_INTERFACE

/** The interface the client asks for (`openTunnel`); addresses and routes in CIDR form. */
data class TunnelRequest(
    val addresses: List<String>,
    val routes: List<String>,
    val dnsServers: List<String>,
    val mtu: Int,
    val session: String,
)

/**
 * Brings up the VPN interface and hands its descriptor to the client (Rust), which reads and writes IP packets
 * on it and closes it on disconnect. The app itself is left out of the VPN, so the engine's own connections to
 * its servers go out directly. Same behaviour as the Tauri app's service it replaces.
 */
class AnxVpnService : VpnService() {
    companion object {
        private const val ACTION_START = "io.anixops.net.vpn.START"
        private const val CHANNEL = "vpn"
        private const val NOTIFICATION_ID = 1
        private const val START_TIMEOUT_SECONDS = 30L

        private var pending: Pending? = null

        @Volatile
        private var running: AnxVpnService? = null

        private class Pending(val request: TunnelRequest) {
            val done = CountDownLatch(1)

            @Volatile
            var fd: Int? = null

            @Volatile
            var error: String? = null
        }

        /**
         * Starts the service and waits until the interface is up; answers its descriptor, which the caller owns.
         * The user must have allowed the VPN already ([VpnService.prepare] answered null): the activity asks
         * before it connects.
         */
        fun open(context: Context, request: TunnelRequest): Int {
            if (VpnService.prepare(context) != null) {
                throw IllegalStateException("The VPN is not allowed yet: allow it when the app asks")
            }
            val wait = Pending(request)
            synchronized(this) {
                pending?.let { replaced ->
                    replaced.error = "Another VPN start replaced this one"
                    replaced.done.countDown()
                }
                pending = wait
            }
            context.startService(Intent(context, AnxVpnService::class.java).setAction(ACTION_START))
            if (!wait.done.await(START_TIMEOUT_SECONDS, TimeUnit.SECONDS)) {
                synchronized(this) { if (pending === wait) pending = null }
                throw IllegalStateException("The VPN did not start in time")
            }
            return wait.fd ?: throw IllegalStateException(wait.error ?: "The VPN could not start")
        }

        fun close() {
            running?.stopSelf()
        }

        @Synchronized
        private fun take(): Pending? = pending.also { pending = null }
    }

    override fun onCreate() {
        super.onCreate()
        running = this
    }

    override fun onDestroy() {
        if (running === this) {
            running = null
        }
        ConnectTile.refresh(this)
        super.onDestroy()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (alwaysOnStart(intent?.action)) {
            connectForSystem()
            return START_NOT_STICKY
        }
        if (intent?.action == ACTION_START) {
            val wait = take() ?: return START_NOT_STICKY
            try {
                wait.fd = establish(wait.request)
                showConnected(wait.request.session)
                ConnectTile.refresh(this)
            } catch (error: Exception) {
                wait.error = error.message ?: error.toString()
            } finally {
                wait.done.countDown()
            }
        }
        return START_NOT_STICKY
    }

    private fun establish(request: TunnelRequest): Int {
        val builder = Builder()
            .setSession(request.session)
            .setMtu(request.mtu)
            .setBlocking(true)
        for (address in request.addresses) {
            val (ip, prefix) = cidr(address)
            builder.addAddress(ip, prefix)
        }
        for (route in request.routes) {
            val (ip, prefix) = cidr(route)
            builder.addRoute(ip, prefix)
        }
        for (server in request.dnsServers) {
            builder.addDnsServer(server)
        }
        builder.addDisallowedApplication(packageName)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            builder.setMetered(false)
        }
        val descriptor = builder.establish() ?: throw IllegalStateException("The VPN is no longer allowed")
        return descriptor.detachFd()
    }

    /** Keeps the service in the foreground while the VPN is up; tapping it opens the app. */
    private fun showConnected(session: String) {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, getString(R.string.notification_channel_vpn), NotificationManager.IMPORTANCE_LOW),
        )
        val open = packageManager.getLaunchIntentForPackage(packageName)?.let {
            PendingIntent.getActivity(this, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        }
        val notification = Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(session)
            .setContentText(getString(R.string.notification_connected))
            .setOngoing(true)
            .setContentIntent(open)
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun cidr(value: String): Pair<String, Int> {
        val slash = value.lastIndexOf('/')
        require(slash > 0) { "Not a CIDR range: $value" }
        return Pair(value.substring(0, slash), value.substring(slash + 1).toInt())
    }

    /**
     * The system started the service itself: the user set this app as the always-on VPN (系统设置 › VPN), at boot
     * or after the app was stopped. Connects as the connect button would, unless already connected; the client then
     * opens the tunnel through [open], which this running service answers.
     */
    private fun connectForSystem() {
        val app = application as AnixApplication
        app.scope.launch {
            runCatching {
                if (app.core.snapshot().state != ConnectionState.Connected) app.core.connect()
            }
        }
    }

    /**
     * Another VPN took over, or the user turned this one off in the system settings. The client is told too, so
     * the app and the tile show it as disconnected instead of a tunnel that no longer carries anything.
     */
    override fun onRevoke() {
        val app = application as AnixApplication
        app.scope.launch { runCatching { app.core.disconnect() } }
        stopSelf()
    }
}
