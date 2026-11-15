package io.anixops.net.core

import kotlinx.serialization.Serializable

/** A rewrite / HTTPS decryption plugin in the library (`list_mitm_plugins`, `bridge.ts` `MitmPlugin`). */
@Serializable
data class MitmPlugin(
    val id: String,
    val name: String,
    /** Empty when it was opened from a file or pasted. */
    val url: String = "",
    val enabled: Boolean = false,
    /** How much of it the engine runs: `full`, `partial` or `unsupported`. */
    val level: String = "full",
    val hostnames: List<String> = emptyList(),
    val scriptCount: Int = 0,
)

/** What a saved plugin holds, as the engine reads it (`mitm_plugin_details`). */
@Serializable
data class MitmPluginDetails(
    val excludedHostnames: List<String> = emptyList(),
    val supportedLines: Int = 0,
    val partialLines: Int = 0,
    val unsupportedLines: Int = 0,
    val deferredLines: Int = 0,
    val scripts: List<String> = emptyList(),
    val resources: List<String> = emptyList(),
    val report: String = "",
)
