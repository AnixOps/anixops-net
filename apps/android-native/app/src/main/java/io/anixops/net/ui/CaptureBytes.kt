package io.anixops.net.ui

import io.anixops.net.core.CaptureConnection
import io.anixops.net.core.MappingText
import java.util.Locale

/** Captured bytes as the connection view shows them, as `capture-format.ts` does on the desktop. */
object CaptureBytes {
    /** Most bytes a hex dump shows; the rest is "…". */
    const val HEX_LIMIT = 64 * 1024

    private const val ROW = 16

    /** Offset, sixteen bytes in hex and their printable characters per line, like `xxd`. */
    fun hexDump(bytes: ByteArray, limit: Int = HEX_LIMIT): String {
        val end = minOf(bytes.size, limit)
        val lines = (0 until end step ROW).map { offset ->
            val row = bytes.copyOfRange(offset, minOf(offset + ROW, end))
            val hex = row.joinToString(" ") { String.format(Locale.ROOT, "%02x", it.toInt() and 0xff) }
            val ascii = row.map { byte -> (byte.toInt() and 0xff).let { if (it in 0x20 until 0x7f) it.toChar() else '.' } }.joinToString("")
            String.format(Locale.ROOT, "%08x  %-47s  %s", offset, hex, ascii)
        }
        return (if (bytes.size > end) lines + "…" else lines).joinToString("\n")
    }

    /** Whether the first upload is a TLS record (handshake, version 3.x), so the data is ciphertext. */
    fun looksLikeTls(capture: CaptureConnection): Boolean {
        val first = capture.chunks.firstOrNull { it.dir == "upload" } ?: return false
        val bytes = MappingText.bytes(first.dataBase64)
        return bytes.size >= 3 && bytes[0] == TLS_HANDSHAKE && bytes[1] == TLS_MAJOR
    }

    /** UTF-8 text of [bytes] ([MappingText.text]), or null for bytes that are not text. */
    fun text(bytes: ByteArray): String? = MappingText.text(bytes)?.let { if (it.length > BodyText.MAX_CHARS) it.take(BodyText.MAX_CHARS) + "\n…" else it }

    private const val TLS_HANDSHAKE: Byte = 0x16
    private const val TLS_MAJOR: Byte = 0x03
}
