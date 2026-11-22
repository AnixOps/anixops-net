package io.anixops.net.ui

import java.util.Locale

/**
 * Binary bodies the inspector can still show as something: pictures recognised by their first bytes, and Protobuf
 * (bare, or framed as gRPC) decoded without a schema, the way `protoc --decode_raw` does. Everything here reads bytes
 * from the network, so every length is checked against what is there, work stops at fixed limits, and the answer is
 * null rather than an exception. The rules, limits and printed text are the same as the desktop's `binary-view.ts`
 * and Apple's `CaptureBinary.swift`; docs/body-preview.md describes them.
 */
object CaptureBinary {
    /** Pictures larger than this many pixels are not drawn: a few kilobytes of PNG can claim 65535 × 65535. */
    const val MAX_IMAGE_PIXELS = 67_108_864L

    /** The longest side of a picture as the inspector draws it. */
    const val PREVIEW_PIXELS = 2048

    /** The most bytes of a body decoded as Protobuf; the rest is left out, as with a body capture cut. */
    const val MAX_PROTO_BYTES = 2 * 1024 * 1024

    /** How deep messages inside messages are opened; deeper ones are shown as bytes. */
    const val MAX_DEPTH = 16

    /** The most fields shown in all; decoding stops there and says so. */
    const val MAX_FIELDS = 20_000

    /** How many bytes, in all, are looked at again to tell text from a message inside a length-delimited field. */
    const val MAX_SCAN = 16 * 1024 * 1024

    /** The most gRPC messages read from one body. */
    const val MAX_FRAMES = 1_000

    private const val STRING_SHOWN = 1_000
    private const val BYTES_SHOWN = 32
    private const val MAX_FIELD_NUMBER = 536_870_911

    /** `Application/JSON; charset=utf-8` → `application/json`. */
    fun mediaType(contentType: String?): String = contentType.orEmpty().substringBefore(';').trim().lowercase(Locale.ROOT)

    // Pictures ------------------------------------------------------------------------------------------------------

    /** The picture type of [bytes] from its signature, or null. */
    fun sniffImage(bytes: ByteArray): String? {
        fun at(offset: Int, text: String): Boolean {
            val wanted = text.toByteArray(Charsets.ISO_8859_1)
            return bytes.size >= offset + wanted.size && wanted.indices.all { bytes[offset + it] == wanted[it] }
        }
        fun u(index: Int) = bytes[index].toInt() and 0xff
        return when {
            bytes.size >= 8 && u(0) == 0x89 && at(1, "PNG\r\n\u001a\n") -> "image/png"
            bytes.size >= 3 && u(0) == 0xff && u(1) == 0xd8 && u(2) == 0xff -> "image/jpeg"
            at(0, "GIF87a") || at(0, "GIF89a") -> "image/gif"
            at(0, "RIFF") && at(8, "WEBP") -> "image/webp"
            at(4, "ftyp") && (at(8, "avif") || at(8, "avis")) -> "image/avif"
            // "BM" alone is too common in other data: the reserved words must be zero and the header size a known one.
            bytes.size >= 26 && at(0, "BM") && (6..9).all { u(it) == 0 } &&
                (u(14) or (u(15) shl 8) or (u(16) shl 16) or (u(17) shl 24)) in setOf(12, 40, 52, 56, 108, 124) -> "image/bmp"
            else -> null
        }
    }

    // Saving and opening --------------------------------------------------------------------------------------------

    /**
     * The file extension for a body of [contentType], and whether another app may be asked to open such a file.
     * Types that run code where they open (HTML, SVG, scripts, XML, CSV) are shared but never opened from here;
     * anything unknown is `bin`. The same list as `body_type` in crates/desktop-mitm/src/capture.rs.
     */
    fun fileType(contentType: String?): Pair<String, Boolean> {
        val media = mediaType(contentType)
        return when {
            media == "image/png" -> "png" to true
            media == "image/jpeg" || media == "image/jpg" -> "jpg" to true
            media == "image/gif" -> "gif" to true
            media == "image/webp" -> "webp" to true
            media == "image/avif" -> "avif" to true
            media == "image/bmp" -> "bmp" to true
            media == "image/x-icon" || media == "image/vnd.microsoft.icon" -> "ico" to true
            media == "application/pdf" -> "pdf" to true
            media == "text/plain" -> "txt" to true
            media == "video/mp4" -> "mp4" to true
            media == "video/webm" -> "webm" to true
            media == "audio/mpeg" -> "mp3" to true
            media == "audio/mp4" -> "m4a" to true
            media == "audio/ogg" -> "ogg" to true
            media == "audio/wav" || media == "audio/x-wav" -> "wav" to true
            media == "application/json" || media.endsWith("+json") -> "json" to true
            media == "image/svg+xml" -> "svg" to false
            media == "text/html" -> "html" to false
            media == "text/css" -> "css" to false
            media == "text/csv" -> "csv" to false
            media == "text/javascript" || media == "application/javascript" -> "js" to false
            media == "application/xml" || media == "text/xml" || media.endsWith("+xml") -> "xml" to false
            media == "application/x-www-form-urlencoded" -> "txt" to false
            media == "font/woff2" -> "woff2" to false
            media == "font/woff" -> "woff" to false
            media == "application/zip" -> "zip" to false
            else -> "bin" to false
        }
    }

    // Protobuf ------------------------------------------------------------------------------------------------------

    sealed interface ProtoValue {
        data class Varint(val value: ULong) : ProtoValue
        data class Fixed64(val value: ULong) : ProtoValue
        data class Fixed32(val value: UInt) : ProtoValue

        /** Length-delimited bytes that decode as a message. */
        data class Message(val fields: List<ProtoField>, val size: Int) : ProtoValue

        /** Length-delimited bytes that are text. */
        data class Text(val text: String, val size: Int) : ProtoValue

        /** Other length-delimited bytes: the first few and how many there are. */
        class Bytes(val head: ByteArray, val size: Int) : ProtoValue
    }

    data class ProtoField(val number: Int, val value: ProtoValue)

    /** [cut]: not every byte was decoded (the body was cut, too long, or had more fields than are shown). */
    data class ProtoMessage(val fields: List<ProtoField>, val cut: Boolean)

    /** Media types that carry one bare Protobuf message. */
    fun isProtobufType(media: String): Boolean {
        if (isGrpcType(media)) return false
        return media in setOf(
            "application/x-protobuf",
            "application/protobuf",
            "application/x-google-protobuf",
            "application/vnd.google.protobuf",
            "application/x-protobuffer",
        ) || media.endsWith("+proto")
    }

    /** gRPC and gRPC-Web in their binary form, where each message is framed. `grpc-web-text` is base64 text. */
    fun isGrpcType(media: String): Boolean =
        media in setOf("application/grpc", "application/grpc+proto", "application/grpc-web", "application/grpc-web+proto")

    /** The reader and the limits it shares across one body. */
    private class Reader(val bytes: ByteArray) {
        var fields = MAX_FIELDS
        var scan = MAX_SCAN

        /** Set once the field limit stopped decoding. */
        var spent = false
        private var offset = 0

        private fun u(index: Int) = bytes[index].toInt() and 0xff

        /** A tag or a length: at most five bytes. */
        private fun small(end: Int): Long? {
            var result = 0L
            for (index in 0 until 5) {
                if (offset >= end) return null
                val byte = u(offset++)
                result = result or ((byte and 0x7f).toLong() shl (7 * index))
                if (byte < 0x80) return result
            }
            return null
        }

        /** A value: up to ten bytes, 64 bits. */
        private fun varint(end: Int): ULong? {
            var result = 0UL
            var shift = 0
            while (shift < 70) {
                if (offset >= end) return null
                val byte = u(offset++)
                // The tenth byte may only carry the top bit of a 64-bit value.
                if (shift == 63 && byte > 1) return null
                result = result or ((byte and 0x7f).toULong() shl shift)
                if (byte < 0x80) return result
                shift += 7
            }
            return null
        }

        private fun little(at: Int, count: Int): ULong {
            var value = 0UL
            for (index in 0 until count) value = value or (u(at + index).toULong() shl (8 * index))
            return value
        }

        /**
         * The fields of one message in `bytes[start, end)`; null when the bytes are not a whole message. With
         * [partial], the fields before the first one that does not decode are kept (the body was cut there).
         */
        fun message(start: Int, end: Int, depth: Int, partial: Boolean): ProtoMessage? {
            offset = start
            val found = mutableListOf<ProtoField>()
            fun stop(): ProtoMessage? = if (partial && found.isNotEmpty()) ProtoMessage(found, true) else null
            while (offset < end) {
                if (fields <= 0) {
                    spent = true
                    return ProtoMessage(found, true)
                }
                val tag = small(end) ?: return stop()
                val number = tag ushr 3
                if (number < 1 || number > MAX_FIELD_NUMBER) return stop()
                val value: ProtoValue = when ((tag and 7).toInt()) {
                    0 -> ProtoValue.Varint(varint(end) ?: return stop())
                    1 -> {
                        if (end - offset < 8) return stop()
                        ProtoValue.Fixed64(little(offset, 8)).also { offset += 8 }
                    }
                    5 -> {
                        if (end - offset < 4) return stop()
                        ProtoValue.Fixed32(little(offset, 4).toUInt()).also { offset += 4 }
                    }
                    2 -> {
                        val length = small(end) ?: return stop()
                        if (length > end - offset) return stop()
                        val from = offset
                        val to = from + length.toInt()
                        lengthDelimited(from, to, depth).also { offset = to }
                    }
                    // Groups (3, 4) are long deprecated and 6, 7 do not exist: not a message this reader shows.
                    else -> return stop()
                }
                fields -= 1
                found += ProtoField(number.toInt(), value)
            }
            return ProtoMessage(found, false)
        }

        private fun lengthDelimited(from: Int, to: Int, depth: Int): ProtoValue {
            val size = to - from
            val head = bytes.copyOfRange(from, minOf(to, from + BYTES_SHOWN))
            if (size > scan) return ProtoValue.Bytes(head, size)
            scan -= size
            printable(bytes, from, to)?.let { return ProtoValue.Text(it, size) }
            if (depth + 1 < MAX_DEPTH) {
                // A failed try gives back the fields it counted; one stopped by the field limit is kept. The reader
                // carries on after the field whatever the try did to its position.
                val before = fields
                val inner = message(from, to, depth + 1, partial = false)
                if (inner != null && (inner.fields.isNotEmpty() || inner.cut)) return ProtoValue.Message(inner.fields, size)
                fields = before
            }
            return ProtoValue.Bytes(head, size)
        }
    }

    /** The bytes as UTF-8 text when they are text: no control characters besides tab, line feed and carriage return. */
    private fun printable(bytes: ByteArray, from: Int, to: Int): String? {
        for (index in from until to) {
            val byte = bytes[index].toInt() and 0xff
            if ((byte < 0x20 && byte != 0x09 && byte != 0x0a && byte != 0x0d) || byte == 0x7f) return null
        }
        return try {
            Charsets.UTF_8.newDecoder()
                .onMalformedInput(java.nio.charset.CodingErrorAction.REPORT)
                .onUnmappableCharacter(java.nio.charset.CodingErrorAction.REPORT)
                .decode(java.nio.ByteBuffer.wrap(bytes, from, to - from))
                .toString()
        } catch (error: java.nio.charset.CharacterCodingException) {
            null
        }
    }

    /**
     * [bytes] as one Protobuf message without a schema; null when they are not one. A body that was cut ([cut]) or
     * is longer than [MAX_PROTO_BYTES] keeps the fields before the cut. An empty body is an empty message.
     */
    fun decodeProtobuf(bytes: ByteArray, cut: Boolean = false): ProtoMessage? {
        val shortened = bytes.size > MAX_PROTO_BYTES
        val reader = Reader(if (shortened) bytes.copyOf(MAX_PROTO_BYTES) else bytes)
        val message = reader.message(0, reader.bytes.size, 0, cut || shortened) ?: return null
        return ProtoMessage(message.fields, message.cut || shortened || reader.spent)
    }

    /** A binary body of unknown type that also reads as a Protobuf message, worth offering as a second view. */
    fun looksLikeProtobuf(bytes: ByteArray): ProtoMessage? {
        if (bytes.isEmpty()) return null
        val message = decodeProtobuf(bytes) ?: return null
        return message.takeIf { !it.cut && it.fields.isNotEmpty() }
    }

    // gRPC ----------------------------------------------------------------------------------------------------------

    /**
     * One gRPC frame: [compressed] with the stream's `grpc-encoding` (not decoded), [trailers] for a gRPC-Web
     * trailer frame, [size] as its header gives it, and the [data] that is there (less when the body was cut).
     */
    class GrpcFrame(val compressed: Boolean, val trailers: Boolean, val size: Int, val data: ByteArray)

    class GrpcBody(val frames: List<GrpcFrame>, val cut: Boolean)

    /** The length-prefixed frames of a gRPC body; null when the body does not start with one. */
    fun grpcFrames(bytes: ByteArray): GrpcBody? {
        val frames = mutableListOf<GrpcFrame>()
        var offset = 0
        fun u(index: Int) = bytes[index].toInt() and 0xff
        while (offset < bytes.size) {
            if (frames.size >= MAX_FRAMES) return GrpcBody(frames, true)
            if (bytes.size - offset < 5 || (u(offset) and 0x7e) != 0) return if (frames.isEmpty()) null else GrpcBody(frames, true)
            val flag = u(offset)
            val size = (u(offset + 1).toLong() shl 24) or (u(offset + 2).toLong() shl 16) or (u(offset + 3).toLong() shl 8) or u(offset + 4).toLong()
            val start = offset + 5
            val end = minOf(bytes.size.toLong(), start + size).toInt()
            frames += GrpcFrame(flag and 0x01 != 0, flag and 0x80 != 0, size.coerceAtMost(Int.MAX_VALUE.toLong()).toInt(), bytes.copyOfRange(start, end))
            if (start + size > bytes.size) return GrpcBody(frames, true)
            offset = end
        }
        return if (frames.isEmpty()) null else GrpcBody(frames, false)
    }

    // Text ----------------------------------------------------------------------------------------------------------

    /** The words around decoded Protobuf, in the app's language. */
    class Labels(
        /** The heading of the `index`th (from 1) gRPC message of `size` bytes. */
        val message: (index: Int, size: Int) -> String,
        val compressed: String,
        val trailers: String,
        val undecoded: String,
        /** The end of what is shown when not everything was. */
        val more: String,
    )

    private fun hex(bytes: ByteArray): String = bytes.joinToString(" ") { String.format(Locale.ROOT, "%02x", it.toInt() and 0xff) }

    private fun quoted(text: String): String {
        val shown = if (text.length > STRING_SHOWN) text.take(STRING_SHOWN) + "…" else text
        val out = StringBuilder("\"")
        for (char in shown) {
            when (char) {
                '\\' -> out.append("\\\\")
                '"' -> out.append("\\\"")
                '\n' -> out.append("\\n")
                '\r' -> out.append("\\r")
                '\t' -> out.append("\\t")
                else -> out.append(char)
            }
        }
        return out.append('"').toString()
    }

    private fun lines(fields: List<ProtoField>, indent: String, out: MutableList<String>) {
        for ((number, value) in fields) {
            val name = "$indent$number"
            when (value) {
                is ProtoValue.Varint -> out += "$name: ${value.value}" + if (value.value >= (1UL shl 63)) " (${value.value.toLong()})" else ""
                is ProtoValue.Fixed64 -> out += "$name: 0x${value.value.toString(16).padStart(16, '0')} (fixed64)"
                is ProtoValue.Fixed32 -> out += "$name: 0x${value.value.toString(16).padStart(8, '0')} (fixed32)"
                is ProtoValue.Message -> {
                    out += "$name {"
                    lines(value.fields, "$indent  ", out)
                    out += "$indent}"
                }
                is ProtoValue.Text -> out += "$name: ${quoted(value.text)}"
                is ProtoValue.Bytes -> out += "$name: <${hex(value.head)}${if (value.size > value.head.size) " …" else ""}> (${value.size} B)"
            }
        }
    }

    /** A decoded message as text, one field per line, in `protoc --decode_raw` style. */
    fun protoText(message: ProtoMessage, more: String): String {
        val out = mutableListOf<String>()
        lines(message.fields, "", out)
        if (message.cut) out += more
        return out.joinToString("\n")
    }

    /** A gRPC body as text: each message under a heading, decoded where it can be. */
    fun grpcText(body: GrpcBody, labels: Labels): String {
        val parts = mutableListOf<String>()
        var index = 0
        for (frame in body.frames) {
            if (frame.trailers) {
                val shown = printable(frame.data, 0, frame.data.size) ?: hex(frame.data.copyOf(minOf(frame.data.size, BYTES_SHOWN)))
                parts += "# ${labels.trailers}\n$shown".trimEnd()
                continue
            }
            index += 1
            val heading = "# ${labels.message(index, frame.size)}"
            if (frame.compressed) {
                parts += "$heading\n${labels.compressed}"
                continue
            }
            val message = decodeProtobuf(frame.data, cut = frame.data.size < frame.size)
            parts += if (message != null) "$heading\n${protoText(message, labels.more)}".trimEnd() else "$heading\n${labels.undecoded}"
        }
        if (body.cut) parts += labels.more
        return parts.joinToString("\n\n")
    }
}
