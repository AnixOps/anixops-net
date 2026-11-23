package io.anixops.net.platform

/** What the Wi-Fi rules learn about the current Wi-Fi's name. */
sealed interface WifiName {
    /** The name, read now or earlier on the same network. */
    data class Known(val ssid: String) : WifiName

    /**
     * The app may read the name, but Android hides it for now: Android 10 and later do while the app is in the
     * background without the background location permission, and while location is off. The rules then do
     * nothing rather than treat a named network as "other Wi-Fi".
     */
    data object Withheld : WifiName

    /** The app may not read names (no precise location permission): the rules treat it as "other Wi-Fi". */
    data object Unknown : WifiName
}

/**
 * Remembers the name of the Wi-Fi the device is on, by Android's network handle, so a network whose name was read
 * while the app was in front keeps it after the app went to the background. A reconnect is a new network with a new
 * handle, so an old name is never carried over to another network.
 */
class WifiNames {
    private var handle: Long? = null
    private var name: String? = null

    /**
     * Notes what Android said about network [handle]: [reported] is the SSID as Android gives it (quoted, or the
     * placeholder when hidden), [mayRead] whether the app holds the precise location permission.
     */
    @Synchronized
    fun read(handle: Long, reported: String?, mayRead: Boolean): WifiName {
        val ssid = clean(reported)
        if (ssid != null) {
            this.handle = handle
            name = ssid
            return WifiName.Known(ssid)
        }
        val remembered = name
        if (this.handle == handle && remembered != null) return WifiName.Known(remembered)
        if (this.handle != handle) {
            this.handle = null
            name = null
        }
        return if (mayRead) WifiName.Withheld else WifiName.Unknown
    }

    /** Remembers a name a network callback delivered, without answering anything. */
    @Synchronized
    fun learn(handle: Long, reported: String?) {
        val ssid = clean(reported) ?: return
        this.handle = handle
        name = ssid
    }

    companion object {
        /** What Android answers in place of a hidden name (`WifiManager.UNKNOWN_SSID`). */
        const val HIDDEN = "<unknown ssid>"

        /** The SSID without Android's quotes; null for none or the hidden placeholder. */
        fun clean(reported: String?): String? =
            reported?.removeSurrounding("\"")?.takeIf { it.isNotEmpty() && it != HIDDEN }
    }
}
