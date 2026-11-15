package io.anixops.net.core

/**
 * What the Rust client asks of Android (the VPN, Downloads, the Keystore, the current network, notifications),
 * called from Rust threads through JNI. [call] gets the method name and its JSON arguments and answers in the
 * [Envelope]; it may block (the VPN start waits for the interface) but must never throw.
 */
interface PlatformBridge {
    fun call(method: String, args: String): String
}
