package io.anixops.net.core

import kotlinx.serialization.Serializable

/** The system notification switches (`notification_settings`); [language] is that of the texts. */
@Serializable
data class NotificationSettings(
    val scripts: Boolean = true,
    val connection: Boolean = true,
    val subscriptions: Boolean = true,
    val language: String = "",
)

/** One named Wi-Fi network's rule. [action] is `connect`, `disconnect` or `ignore`. */
@Serializable
data class WifiRule(val ssid: String, val action: String)

/** The Wi-Fi rules (`on_demand_settings`), as the iOS On Demand settings: named networks first, first match wins. */
@Serializable
data class OnDemandSettings(
    val enabled: Boolean = false,
    val wifi: List<WifiRule> = emptyList(),
    val otherWifi: String = "connect",
    val cellular: String = "connect",
    /** Ethernet and other networks. */
    val other: String = "connect",
)

/** The network underneath the VPN (`current_network`); [ssid] when Android gives the Wi-Fi name. */
@Serializable
data class CurrentNetwork(val kind: String, val ssid: String? = null)

/** Crash reports, the local metrics endpoint, the install check and the cold start (`health_status`). */
@Serializable
data class HealthStatus(
    val crashReports: Boolean = false,
    val crashReportsKept: Int = 0,
    /** The build sends reports to an address; otherwise they stay on the device. */
    val crashUploadConfigured: Boolean = false,
    val metricsAddress: String? = null,
    val metricsError: String? = null,
    val integrity: String = "checking",
    val integrityDetail: String? = null,
    val coldStartMs: Long? = null,
)
