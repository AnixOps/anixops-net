package io.anixops.net.platform

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import io.anixops.net.AnixApplication
import io.anixops.net.R
import io.anixops.net.core.ConnectionState
import io.anixops.net.core.RuntimeSnapshot
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/** What a tap on the tile does. */
enum class TileTap {
    Connect,
    Disconnect,

    /** The VPN is not allowed yet; only the app can ask, so the tap opens it. */
    OpenApp,
}

/** The tile is on only while connected; a tap turns it off, or on when the VPN is already allowed. */
fun tileTap(state: ConnectionState?, vpnAllowed: Boolean): TileTap = when {
    state == ConnectionState.Connected -> TileTap.Disconnect
    vpnAllowed -> TileTap.Connect
    else -> TileTap.OpenApp
}

/**
 * The Quick Settings tile (连接): on while connected, with the node in use; off with why otherwise. A tap
 * connects or disconnects without opening the app, the same as the connect button; the first time, when the VPN
 * still needs the user's consent, it opens the app instead, which asks.
 */
class ConnectTile : TileService() {
    private val app get() = application as AnixApplication
    private var reading: Job? = null
    private var snapshot: RuntimeSnapshot? = null
    private var working = false
    private var listening = false

    override fun onStartListening() {
        super.onStartListening()
        listening = true
        read()
    }

    override fun onStopListening() {
        listening = false
        reading?.cancel()
        reading = null
        super.onStopListening()
    }

    override fun onClick() {
        super.onClick()
        if (working) return
        when (tileTap(snapshot?.state, VpnService.prepare(this) == null)) {
            TileTap.OpenApp -> openApp()
            TileTap.Connect -> perform {
                // The client's notifications follow the app's language, also when the app was not opened.
                runCatching { app.applyLanguage() }
                app.core.connect()
            }
            TileTap.Disconnect -> perform { app.core.disconnect() }
        }
    }

    private fun perform(action: suspend () -> Unit) {
        working = true
        show()
        // On the main thread like the system's calls; the core runs each command on its own background thread.
        app.scope.launch(Dispatchers.Main) {
            // A failure shows on the tile itself ("连接失败") and in the app, which reads the same state.
            runCatching { action() }
            working = false
            read()
        }
    }

    private fun read() {
        reading?.cancel()
        reading = app.scope.launch(Dispatchers.Main) {
            snapshot = runCatching { app.core.snapshot() }.getOrNull()
            show()
        }
    }

    /** Draws the tile; only while the system listens, when [getQsTile] is there. */
    private fun show() {
        if (!listening) return
        val tile = qsTile ?: return
        val state = snapshot?.state
        tile.state = when {
            working -> Tile.STATE_UNAVAILABLE
            state == ConnectionState.Connected -> Tile.STATE_ACTIVE
            else -> Tile.STATE_INACTIVE
        }
        tile.label = getString(R.string.tile_label)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            tile.subtitle = when {
                working -> getString(R.string.tile_working)
                state == ConnectionState.Connected -> snapshot?.selectedNode ?: getString(R.string.notification_connected)
                state == ConnectionState.Failed || state == ConnectionState.CoreError -> getString(R.string.tile_failed)
                else -> getString(R.string.tile_off)
            }
        }
        tile.updateTile()
    }

    // The Intent form is the only one before Android 14 and runs only there; lint does not read the version check.
    @Suppress("DEPRECATION")
    @SuppressLint("StartActivityAndCollapseDeprecated")
    private fun openApp() {
        val open = packageManager.getLaunchIntentForPackage(packageName) ?: return
        open.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startActivityAndCollapse(PendingIntent.getActivity(this, 0, open, PendingIntent.FLAG_IMMUTABLE))
        } else {
            startActivityAndCollapse(open)
        }
    }

    companion object {
        private fun component(context: Context) = ComponentName(context, ConnectTile::class.java)

        /** Asks the system to redraw the tile after the VPN came up or went down outside it. */
        fun refresh(context: Context) {
            requestListeningState(context, component(context))
        }
    }
}
