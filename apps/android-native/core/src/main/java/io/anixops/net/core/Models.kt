package io.anixops.net.core

import kotlinx.serialization.Serializable

/**
 * What `runtime_snapshot` answers, as far as the app reads it. Field names are the shared interface's
 * (`bridge.ts` `RuntimeSnapshot`); the Rust side may add fields, which are ignored here.
 */
@Serializable
data class RuntimeSnapshot(
    val platform: String,
    /** "Connected", "Not connected", "Connection failed" or "Core error"; see [ConnectionState]. */
    val connection: String,
    val core: StatusFact,
    val selectedNode: String? = null,
    val subscription: String? = null,
    val subscriptionLastUpdated: String? = null,
    val lastError: String? = null,
    val configurationError: String? = null,
    val routingMode: String,
    val dnsPreset: String,
    val blockQuic: Boolean = false,
    val directEgress: Boolean = false,
    val httpsDecryptionEnabled: Boolean = false,
) {
    val state: ConnectionState get() = ConnectionState.of(connection)
}

/** A status line: label, optional detail, and tone (success, danger, warning, neutral). */
@Serializable
data class StatusFact(
    val label: String,
    val detail: String? = null,
    val tone: String,
)

/**
 * What most commands answer. [code] and [params] name the result, which the app words in its own language
 * ([CoreTexts]); [follow] is a second code with no parameters said after it (what the change did to the connection),
 * [note] a sentence already in the app's language. [message] is the client's English, for a code the app does not
 * know; [Core] replaces it with the app's words when it knows them.
 */
@Serializable
data class OperationResult(
    val message: String,
    val code: String? = null,
    val params: Map<String, String> = emptyMap(),
    val follow: String? = null,
    val note: String? = null,
)

/**
 * What a command that saves to Downloads answers: the core's sentence, and where the file went ([location]), so the
 * app can say it in its own language; the decryption root adds its SHA-256 [fingerprint], or why it was not saved
 * ([saveError]).
 */
@Serializable
data class SavedFileResult(
    val message: String,
    val code: String? = null,
    val params: Map<String, String> = emptyMap(),
    val location: String? = null,
    val fingerprint: String? = null,
    val saveError: String? = null,
)

enum class ConnectionState {
    Connected,
    NotConnected,
    Failed,
    CoreError,
    ;

    companion object {
        fun of(connection: String): ConnectionState = when (connection) {
            "Connected" -> Connected
            "Connection failed" -> Failed
            "Core error" -> CoreError
            else -> NotConnected
        }
    }
}
