package io.anixops.net.core

import java.io.File

/** How [Core] reaches the client: one command by name with its JSON arguments, answered in the [Envelope]. */
fun interface CoreTransport {
    fun invoke(command: String, args: String): String
}

/**
 * The Rust client in libanixops_net.so. [open] loads it and opens the process's one client (later calls keep
 * the first); the activity and the VPN service both call it, whichever starts first.
 */
object NativeCore : CoreTransport {
    private const val LIBRARY = "anixops_net"

    @Volatile
    private var opened = false

    /** Opens the client on [dataDir] with [bridge] for what only Android can do. */
    @Synchronized
    fun open(dataDir: File, bridge: PlatformBridge) {
        if (opened) return
        System.loadLibrary(LIBRARY)
        nativeInit(dataDir.absolutePath, bridge)?.let { throw CoreException(it) }
        opened = true
    }

    override fun invoke(command: String, args: String): String {
        check(opened) { "NativeCore.open has not run" }
        return nativeInvoke(command, args)
    }

    fun shutdown() {
        if (opened) nativeShutdown()
    }

    @JvmStatic
    private external fun nativeInit(dataDir: String, bridge: PlatformBridge): String?

    @JvmStatic
    private external fun nativeInvoke(command: String, args: String): String

    @JvmStatic
    private external fun nativeShutdown()
}
