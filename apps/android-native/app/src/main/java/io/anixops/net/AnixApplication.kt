package io.anixops.net

import android.app.Application
import io.anixops.net.core.Core
import io.anixops.net.core.CoreTransport
import io.anixops.net.core.NativeCore
import io.anixops.net.platform.AndroidPlatform
import java.io.File

/**
 * Holds the process's [Core]. The Rust client opens on the first command, on the command's background thread,
 * so starting the app loads nothing native (and the JVM screenshot tests never do).
 */
class AnixApplication : Application() {
    private val platform by lazy { AndroidPlatform(this) }

    val core: Core by lazy {
        Core(
            CoreTransport { command, args ->
                // Same data directory as the Tauri app, so the cutover keeps settings, nodes and rules.
                NativeCore.open(File(filesDir, DATA_DIR), platform)
                NativeCore.invoke(command, args)
            },
        )
    }

    private companion object {
        const val DATA_DIR = "anixops-net"
    }
}
