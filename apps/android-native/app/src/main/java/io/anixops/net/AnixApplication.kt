package io.anixops.net

import android.app.Application
import android.content.res.Configuration
import io.anixops.net.core.Core
import io.anixops.net.core.CoreTransport
import io.anixops.net.core.NativeCore
import io.anixops.net.platform.AndroidPlatform
import io.anixops.net.platform.Channels
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/**
 * Holds the process's [Core]. The Rust client opens on the first command, on the command's background thread,
 * so starting the app loads nothing native (and the JVM screenshot tests never do).
 */
class AnixApplication : Application() {
    private val platform by lazy { AndroidPlatform(this) }

    /** Work that outlives a screen: the Quick Settings tile's taps and the always-on VPN's start. */
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    private val opened = lazy {
        Core(
            CoreTransport { command, args ->
                // Same data directory as the Tauri app, so the cutover keeps settings, nodes and rules.
                NativeCore.open(File(filesDir, DATA_DIR), platform)
                NativeCore.invoke(command, args)
            },
            // Results and refusals in the app's language, never the client's English (CodeTexts).
            texts = CodeTexts(this),
        )
    }

    val core: Core by opened

    override fun onCreate() {
        super.onCreate()
        // The channels show in the system settings before the first notification.
        Channels.ensure(this)
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        // The language changed: the channel names, and the client's notifications if it is open already (opening it
        // here would load the native library for nothing).
        Channels.ensure(this)
        if (opened.isInitialized()) scope.launch { runCatching { applyLanguage() } }
    }

    /**
     * Tells the client the app's language, for the notifications it makes (an unexpected disconnect, a failed
     * update, the Wi-Fi rules). The app calls it when it starts and when its language changes; the system's own
     * starts (the always-on VPN, a restart) call it before connecting.
     */
    suspend fun applyLanguage() {
        core.notificationSettings(clientLanguage(resources.configuration.locales[0]))
    }

    private companion object {
        const val DATA_DIR = "anixops-net"
    }
}
