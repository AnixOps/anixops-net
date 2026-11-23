package io.anixops.net.ui

import io.anixops.net.core.Envelope
import io.anixops.net.core.MappingText
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import java.net.URLDecoder

/**
 * A captured body as the inspector shows it: its text (JSON indented), or null for bytes that are not text, and what
 * it is ([kind]). [size] is the bytes shown (decoded when the core could undo the Content-Encoding), [bytes] those
 * bytes, kept for the hex view, the picture and sharing (empty in screenshots that only show text).
 */
class BodyText(
    val text: String?,
    val size: Long,
    /** Capture kept only the start of the body. */
    val truncated: Boolean = false,
    /** The Content-Encoding could not be undone, so [text] (if any) is of the bytes as captured. */
    val undecoded: Boolean = false,
    val bytes: ByteArray = ByteArray(0),
    /** The body's Content-Type, or the picture's type found from its bytes; names the shared file. */
    val contentType: String? = null,
    val kind: BodyKind = if (text != null) BodyKind.Text else BodyKind.Binary,
) {
    /** The ways to show this body, the one it opens in first. */
    val modes: List<BodyMode>
        get() = when (val shown = kind) {
            BodyKind.Text -> listOf(BodyMode.Raw, BodyMode.Hex)
            is BodyKind.Form -> listOf(BodyMode.Table, BodyMode.Raw, BodyMode.Hex)
            BodyKind.Image -> listOf(BodyMode.Preview, BodyMode.Hex)
            is BodyKind.Protobuf -> if (shown.labelled) listOf(BodyMode.Protobuf, BodyMode.Hex) else listOf(BodyMode.Hex, BodyMode.Protobuf)
            BodyKind.Binary -> listOf(BodyMode.Hex)
        }

    companion object {
        /** Most characters shown; a longer body is cut, as the desktop inspector's preview. */
        const val MAX_CHARS = 200_000

        private val pretty = Json { prettyPrint = true }

        /** The bytes ([base64]) of a body sent with [contentType]; [truncated]: capture kept only their start. */
        fun of(base64: String, contentType: String?, truncated: Boolean, undecoded: Boolean = false): BodyText {
            val bytes = MappingText.bytes(base64)
            val text = text(bytes, contentType)
            val kind = kind(bytes, contentType, text, truncated)
            val fileType = if (kind == BodyKind.Image && !isImageType(CaptureBinary.mediaType(contentType))) CaptureBinary.sniffImage(bytes) else contentType
            return BodyText(text, bytes.size.toLong(), truncated, undecoded, bytes, fileType, kind)
        }

        /** UTF-8 text ([MappingText.text]), indented when it is JSON; null for bytes that are not text. */
        internal fun text(bytes: ByteArray, contentType: String?): String? {
            val text = MappingText.text(bytes) ?: return null
            val shown = if (contentType.orEmpty().contains("json", ignoreCase = true)) indent(text) ?: text else text
            return if (shown.length > MAX_CHARS) shown.take(MAX_CHARS) else shown
        }

        /** A picture the system draws. SVG is text, and is shown as such. */
        private fun isImageType(media: String) = media.startsWith("image/") && media != "image/svg+xml"

        /**
         * What a body is, from its type, its text and its bytes. A picture or Protobuf sent with a generic type is
         * recognised by its bytes; Protobuf found that way opens as hex, since random bytes can read as a message.
         */
        internal fun kind(bytes: ByteArray, contentType: String?, text: String?, cut: Boolean): BodyKind {
            val media = CaptureBinary.mediaType(contentType)
            if (isImageType(media)) return BodyKind.Image
            if (CaptureBinary.isGrpcType(media)) CaptureBinary.grpcFrames(bytes)?.let { return BodyKind.Protobuf(grpc = it, labelled = true) }
            if (CaptureBinary.isProtobufType(media)) CaptureBinary.decodeProtobuf(bytes, cut)?.let { return BodyKind.Protobuf(message = it, labelled = true) }
            if (text == null) {
                if (!media.startsWith("text/") && CaptureBinary.sniffImage(bytes) != null) return BodyKind.Image
                return CaptureBinary.looksLikeProtobuf(bytes)?.let { BodyKind.Protobuf(message = it, labelled = false) } ?: BodyKind.Binary
            }
            if (media == "application/x-www-form-urlencoded") {
                val pairs = formPairs(text)
                if (pairs.isNotEmpty()) return BodyKind.Form(pairs)
            }
            return BodyKind.Text
        }

        /** `a=1&b=two+words` as names and values, decoded. A part without `=` is a name with an empty value. */
        internal fun formPairs(text: String): List<Pair<String, String>> = text.split('&').filter { it.isNotEmpty() }.map { part ->
            val equals = part.indexOf('=')
            if (equals < 0) decoded(part) to "" else decoded(part.substring(0, equals)) to decoded(part.substring(equals + 1))
        }

        private fun decoded(text: String): String = try {
            URLDecoder.decode(text, "UTF-8")
        } catch (error: IllegalArgumentException) {
            // Not valid percent-encoding: shown as written.
            text.replace('+', ' ')
        }

        private fun indent(text: String): String? = runCatching {
            pretty.encodeToString(JsonElement.serializer(), Envelope.json.parseToJsonElement(text))
        }.getOrNull()
    }
}

/** What a body is, which decides how it can be shown. */
sealed interface BodyKind {
    data object Text : BodyKind

    data class Form(val pairs: List<Pair<String, String>>) : BodyKind

    data object Image : BodyKind

    /**
     * Protobuf decoded without a schema: one [message], or gRPC's framed messages ([grpc]). [labelled]: the
     * Content-Type says so, and it opens decoded.
     */
    class Protobuf(
        val message: CaptureBinary.ProtoMessage? = null,
        val grpc: CaptureBinary.GrpcBody? = null,
        val labelled: Boolean,
    ) : BodyKind

    data object Binary : BodyKind
}

/** One way of looking at a body. Hex is always offered, so nothing is hidden behind a nicer view. */
enum class BodyMode {
    Raw,
    Table,
    Preview,
    Protobuf,
    Hex,
}
