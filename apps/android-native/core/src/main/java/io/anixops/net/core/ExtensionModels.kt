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

/** One response header of a Map Local mapping. */
@Serializable
data class HeaderField(val name: String, val value: String)

/**
 * A Map Local / Map Remote mapping (`list_debug_mappings`, `bridge.ts` `DebugMapping`); saved ones become the plugin
 * "AnixOps 调试映射". [id] is empty for a new one, [body] is base64.
 */
@Serializable
data class DebugMapping(
    val id: String = "",
    val name: String = "",
    val enabled: Boolean = true,
    /** `local` answers with [status], [headers] and [body]; `remote` sends the request to [target]. */
    val kind: String = "local",
    /** `url` (that URL with any query), `prefix` or `regex`. */
    val match: String = "url",
    val pattern: String = "",
    val status: Int = 200,
    val headers: List<HeaderField> = emptyList(),
    val body: String = "",
    val target: String = "",
)

/** The destination rewrite editor's fields (`bridge.ts` `RedirectDraft`); [network] is `any`, `tcp` or `udp`. */
@Serializable
data class RedirectDraft(
    val source: String = "",
    val ports: String = "",
    val network: String = "any",
    val target: String = "",
)

/** One destination rewrite (`traffic_redirects`); [draft] is null when the editor cannot show it. */
@Serializable
data class TrafficRedirect(
    val id: String,
    val match: String,
    val target: String,
    val enabled: Boolean = true,
    val draft: RedirectDraft? = null,
)

/** What `save_traffic_redirects` takes back: the list without the editor's fields. */
@Serializable
internal data class SavedRedirect(val id: String, val match: String, val target: String, val enabled: Boolean)
