package io.anixops.net.platform

import android.Manifest
import android.annotation.SuppressLint
import android.app.Notification
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import androidx.annotation.StringRes
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import io.anixops.net.AnixApplication
import io.anixops.net.R
import io.anixops.net.core.ConnectionState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/** Whether the system started the service as the always-on VPN: its start carries [VpnService.SERVICE_INTERFACE]. */
fun alwaysOnStart(action: String?): Boolean = action == VpnService.SERVICE_INTERFACE

/**
 * What the user last asked of the VPN, kept on disk so that a restart after the process died (the system restarts
 * the service with no intent) does what was going on: [Connected] connects again, [Waiting] waits for the network
 * for the Wi-Fi rules, [None] (disconnected by hand with the rules off, or the VPN taken away) stays off.
 */
enum class VpnWant {
    None,
    Connected,
    Waiting,
    ;

    companion object {
        fun of(name: String?): VpnWant = entries.firstOrNull { it.name == name } ?: None
    }
}

/** What a restart with no intent does. */
enum class Restart { Reconnect, Wait, Stop, GiveUp }

/**
 * A restart reconnects only when the user wanted the VPN on; [recentRestarts] reconnects in a short while mean the
 * engine itself keeps failing, and then the service gives up rather than restart forever.
 */
fun restartAction(want: VpnWant, recentRestarts: Int): Restart = when (want) {
    VpnWant.None -> Restart.Stop
    VpnWant.Waiting -> Restart.Wait
    VpnWant.Connected -> if (recentRestarts >= MAX_RESTARTS) Restart.GiveUp else Restart.Reconnect
}

/** The restart times in [stamps] still within [windowMillis] of [now]. */
fun recentStamps(stamps: List<Long>, now: Long, windowMillis: Long): List<Long> =
    stamps.filter { it in (now - windowMillis)..now }

/** Reconnects after the process died, within [RESTART_WINDOW_MILLIS], before the service gives up. */
const val MAX_RESTARTS = 3
const val RESTART_WINDOW_MILLIS = 10 * 60 * 1000L

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
 *
 * The service is in the foreground from its first start: "connecting", then "connected". While the Wi-Fi rules
 * are on it stays in the foreground after a disconnect, "waiting for the network", so the process is neither
 * frozen nor stopped and the rules can connect again by themselves on the next network (Android lets an app the
 * user allowed as a VPN start its foreground service from the background). It is sticky while connected or
 * waiting: when the system stops the process, it restarts the service, which connects or waits again.
 */
class AnxVpnService : VpnService() {
    companion object {
        private const val ACTION_START = "io.anixops.net.vpn.START"
        private const val ACTION_WAIT = "io.anixops.net.vpn.WAIT"
        private const val NOTIFICATION_ID = 1
        private const val FAILURE_NOTICE_ID = 2
        private const val FAILURE_NOTICE_TAG = "vpn-start"
        private const val START_TIMEOUT_SECONDS = 30L
        private const val STATE = "vpn-service"
        private const val WANT = "want"
        private const val RESTARTS = "restarts"

        private var pending: TunnelHandoff<TunnelRequest>? = null

        @Volatile
        private var running: AnxVpnService? = null

        private val main = Handler(Looper.getMainLooper())

        /**
         * Starts the service and waits until the interface is up; answers its descriptor, which the caller owns.
         * The user must have allowed the VPN already ([VpnService.prepare] answered null): the activity asks
         * before it connects.
         */
        fun open(context: Context, request: TunnelRequest): Int {
            if (VpnService.prepare(context) != null) {
                throw IllegalStateException("The VPN is not allowed yet: allow it when the app asks")
            }
            val wait = TunnelHandoff(request)
            synchronized(this) {
                pending?.fail("Another VPN start replaced this one")
                pending = wait
            }
            try {
                // A foreground start: the service may not be running (the Wi-Fi rules connect from the background),
                // and a plain start from the background is refused.
                try {
                    ContextCompat.startForegroundService(
                        context,
                        Intent(context, AnxVpnService::class.java).setAction(ACTION_START),
                    )
                } catch (error: IllegalStateException) {
                    throw IllegalStateException(
                        context.getString(R.string.vpn_start_refused, error.message ?: error.toString()),
                    )
                }
                return wait.await(START_TIMEOUT_SECONDS)
            } finally {
                synchronized(this) { if (pending === wait) pending = null }
            }
        }

        /**
         * The client closed the tunnel. With [waitForNetwork] (the Wi-Fi rules are on) the service stays in the
         * foreground waiting for the network; otherwise it stops.
         */
        fun close(waitForNetwork: Boolean = false) {
            main.post { running?.closed(waitForNetwork) }
        }

        /**
         * The Wi-Fi rules were turned on, or the app opened with them on: keeps the service waiting for the network
         * while not connected, so the rules can connect from the background. From the app in front only.
         */
        fun standby(context: Context) {
            if (VpnService.prepare(context) != null) return
            try {
                ContextCompat.startForegroundService(
                    context,
                    Intent(context, AnxVpnService::class.java).setAction(ACTION_WAIT),
                )
            } catch (error: IllegalStateException) {
                // Not in front after all: the rules still act while the app runs.
            }
        }

        /** The Wi-Fi rules were turned off: a service that only waited for them stops. */
        fun leaveStandby(context: Context) {
            main.post {
                val service = running
                if (service == null) {
                    store(context).edit { putString(WANT, VpnWant.None.name) }
                } else {
                    service.closedIfWaiting()
                }
            }
        }

        private fun store(context: Context): SharedPreferences =
            context.applicationContext.getSharedPreferences(STATE, Context.MODE_PRIVATE)

        @Synchronized
        private fun take(): TunnelHandoff<TunnelRequest>? = pending.also { pending = null }
    }

    /** Whether the interface is up and its descriptor with the client. Main thread only. */
    private var connected = false

    /** The VPN's name in the system, the connected notification's title. */
    private var session = ""

    /** The user turned the VPN off in the system settings, or another VPN took over: no waiting, no restart. */
    private var revoked = false

    private val state get() = store(this)

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
        if (intent == null) return restarted()
        // Every start is a foreground start and must reach startForeground in time, also one with nothing to do.
        if (alwaysOnStart(intent.action)) {
            if (!foreground(R.string.notification_connecting)) return settle()
            // The client then opens the tunnel through [open].
            connectForSystem()
            return START_STICKY
        }
        return when (intent.action) {
            ACTION_START -> start()
            ACTION_WAIT -> {
                if (!connected) remember(VpnWant.Waiting)
                if (foreground(R.string.notification_waiting)) START_STICKY else settle()
            }
            else -> if (connected) START_STICKY else settle()
        }
    }

    /** The client's start ([open]): builds the interface and hands its descriptor over. */
    private fun start(): Int {
        if (!foreground(R.string.notification_connecting)) {
            take()?.fail(getString(R.string.vpn_foreground_refused))
            return settle()
        }
        val wait = take() ?: return if (connected) START_STICKY else settle()
        // The service owns the descriptor until the caller has it: a start that fails after the interface is up,
        // or whose caller stopped waiting, closes it here.
        val fd = try {
            establish(wait.request)
        } catch (error: Exception) {
            wait.fail(error.message ?: error.toString())
            return if (connected) START_STICKY else settle()
        }
        if (!wait.deliver(fd)) {
            closeDescriptor(fd)
            return if (connected) START_STICKY else settle()
        }
        connected = true
        session = wait.request.session
        remember(VpnWant.Connected)
        foreground(R.string.notification_connected)
        ConnectTile.refresh(this)
        return START_STICKY
    }

    /**
     * Shows the notification for what the service does: connected (under the session's name) while connected, else
     * [idle] (connecting, waiting for the network, reconnecting). False when Android refused the foreground.
     */
    private fun foreground(@StringRes idle: Int): Boolean =
        if (connected) show(R.string.notification_connected, session) else show(idle)

    /** The system restarted the service after the process died: does what the user last wanted. */
    private fun restarted(): Int {
        val now = System.currentTimeMillis()
        val stamps = recentStamps(
            state.getString(RESTARTS, "").orEmpty().split(',').mapNotNull { it.toLongOrNull() },
            now,
            RESTART_WINDOW_MILLIS,
        )
        val want = VpnWant.of(state.getString(WANT, null))
        return when (restartAction(want, stamps.size)) {
            Restart.Stop -> settle()
            Restart.GiveUp -> {
                remember(VpnWant.None)
                state.edit { remove(RESTARTS) }
                postFailure(getString(R.string.vpn_restart_gave_up))
                settle()
            }
            Restart.Reconnect -> {
                state.edit { putString(RESTARTS, (stamps + now).joinToString(",")) }
                if (!foreground(R.string.notification_reconnecting)) return settle()
                connectForSystem()
                START_STICKY
            }
            Restart.Wait -> {
                if (!foreground(R.string.notification_waiting)) return settle()
                // The Wi-Fi rules run in the client, which opens on its first command.
                val app = application as AnixApplication
                app.scope.launch { runCatching { app.core.snapshot() } }
                START_STICKY
            }
        }
    }

    /** The tunnel is closed; waits for the network (the Wi-Fi rules) or stops. */
    private fun closed(waitForNetwork: Boolean) {
        connected = false
        if (waitForNetwork && !revoked) {
            remember(VpnWant.Waiting)
            if (show(R.string.notification_waiting)) {
                ConnectTile.refresh(this)
                return
            }
        }
        remember(VpnWant.None)
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun closedIfWaiting() {
        if (!connected) closed(waitForNetwork = false)
    }

    /**
     * Not connected after a start: waits for the network when the Wi-Fi rules asked for that, else stops.
     * Answers the start's result.
     */
    private fun settle(): Int {
        val want = VpnWant.of(state.getString(WANT, null))
        if (!revoked && want == VpnWant.Waiting && show(R.string.notification_waiting)) {
            return START_STICKY
        }
        if (want == VpnWant.Connected) remember(VpnWant.None)
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
        return START_NOT_STICKY
    }

    private fun remember(want: VpnWant) {
        state.edit {
            putString(WANT, want.name)
            if (want != VpnWant.Connected) remove(RESTARTS)
        }
    }

    private fun closeDescriptor(fd: Int) {
        runCatching { ParcelFileDescriptor.adoptFd(fd).close() }
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

    /**
     * Puts the service in the foreground with [text] under [title] (the app's name by default); tapping it opens
     * the app. False when Android refuses (a start from the background it does not allow).
     */
    private fun show(@StringRes text: Int, title: String = getString(R.string.app_name)): Boolean {
        Channels.ensure(this)
        val open = packageManager.getLaunchIntentForPackage(packageName)?.let {
            PendingIntent.getActivity(this, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        }
        val notification = Notification.Builder(this, Channels.VPN)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(getString(text))
            .setOngoing(true)
            .setContentIntent(open)
            .build()
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
            true
        } catch (error: RuntimeException) {
            false
        }
    }

    /** Says why the VPN did not start by itself (always-on, or a restart); a tap opens the app. */
    // The notification permission is checked first; lint does not follow the check.
    @SuppressLint("MissingPermission")
    private fun postFailure(reason: String) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        Channels.ensure(this)
        val open = packageManager.getLaunchIntentForPackage(packageName)?.let {
            PendingIntent.getActivity(this, 1, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        }
        val title = getString(R.string.vpn_start_failed)
        val notification = NotificationCompat.Builder(this, Channels.NOTICES)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(reason)
            .setStyle(NotificationCompat.BigTextStyle().bigText(reason))
            .setContentIntent(open)
            .setAutoCancel(true)
            .build()
        NotificationManagerCompat.from(this).notify(FAILURE_NOTICE_TAG, FAILURE_NOTICE_ID, notification)
    }

    private fun cidr(value: String): Pair<String, Int> {
        val slash = value.lastIndexOf('/')
        require(slash > 0) { "Not a CIDR range: $value" }
        return Pair(value.substring(0, slash), value.substring(slash + 1).toInt())
    }

    /**
     * The system started the service itself: the user set this app as the always-on VPN (系统设置 › VPN), at boot
     * or after the app was stopped, or the service restarts after the process died while connected. Connects as the
     * connect button would, unless already connected; the client then opens the tunnel through [open], which this
     * running service answers. A failure is notified (the system itself only says the always-on VPN is off) and the
     * service stops, or goes back to waiting for the Wi-Fi rules.
     */
    private fun connectForSystem() {
        val app = application as AnixApplication
        app.scope.launch {
            // The notifications the client makes follow the app's language, also without the app open.
            runCatching { app.applyLanguage() }
            val failure = try {
                if (app.core.snapshot().state != ConnectionState.Connected) app.core.connect()
                null
            } catch (error: Exception) {
                error.message ?: error.toString()
            }
            launch(Dispatchers.Main) {
                if (failure != null) postFailure(failure)
                if (!connected && running === this@AnxVpnService) settle()
            }
        }
    }

    /**
     * Another VPN took over, or the user turned this one off in the system settings. The client is told too, so
     * the app and the tile show it as disconnected instead of a tunnel that no longer carries anything. Nothing
     * restarts or waits after this.
     */
    override fun onRevoke() {
        revoked = true
        connected = false
        remember(VpnWant.None)
        val app = application as AnixApplication
        app.scope.launch { runCatching { app.core.disconnect() } }
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }
}
