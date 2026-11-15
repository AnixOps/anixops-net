package io.anixops.net.ui

import io.anixops.net.core.Envelope
import io.anixops.net.core.MappingText
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement

/**
 * A captured body as the inspector shows it: its text (JSON indented), or null for bytes that are not text.
 * [size] is the bytes shown (decoded when the core could undo the Content-Encoding).
 */
data class BodyText(
    val text: String?,
    val size: Long,
    /** Capture kept only the start of the body. */
    val truncated: Boolean = false,
    /** The Content-Encoding could not be undone, so [text] (if any) is of the bytes as captured. */
    val undecoded: Boolean = false,
) {
    companion object {
        /** Most characters shown; a longer body is cut, as the desktop inspector's preview. */
        const val MAX_CHARS = 200_000

        private val pretty = Json { prettyPrint = true }

        /** The bytes ([base64]) of a body sent with [contentType]. */
        fun of(base64: String, contentType: String?, truncated: Boolean, undecoded: Boolean = false): BodyText {
            val bytes = MappingText.bytes(base64)
            return BodyText(text(bytes, contentType), bytes.size.toLong(), truncated, undecoded)
        }

        /** UTF-8 text ([MappingText.text]), indented when it is JSON; null for bytes that are not text. */
        internal fun text(bytes: ByteArray, contentType: String?): String? {
            val text = MappingText.text(bytes) ?: return null
            val shown = if (contentType.orEmpty().contains("json", ignoreCase = true)) indent(text) ?: text else text
            return if (shown.length > MAX_CHARS) shown.take(MAX_CHARS) else shown
        }

        private fun indent(text: String): String? = runCatching {
            pretty.encodeToString(JsonElement.serializer(), Envelope.json.parseToJsonElement(text))
        }.getOrNull()
    }
}
