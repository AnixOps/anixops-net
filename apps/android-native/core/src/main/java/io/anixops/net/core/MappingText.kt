package io.anixops.net.core

import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import java.util.Base64

/** The text forms of a Map Local mapping's headers and body, as the desktop editor writes them (capture-format.ts). */
object MappingText {
    /** One `Name: value` line per header. */
    fun headerLines(headers: List<HeaderField>): String = headers.joinToString("\n") { "${it.name}: ${it.value}" }

    /** Headers from `Name: value` lines; blank lines, lines without a name and HTTP/2 pseudo-headers are left out. */
    fun parseHeaderLines(text: String): List<HeaderField> = text.lines().mapNotNull { line ->
        val colon = line.indexOf(':')
        if (colon <= 0) return@mapNotNull null
        val name = line.substring(0, colon).trim()
        if (name.isEmpty()) null else HeaderField(name, line.substring(colon + 1).trim())
    }

    /** The bytes of a base64 body; a body that does not decode counts as empty. */
    fun bytes(base64: String): ByteArray = try {
        Base64.getDecoder().decode(base64)
    } catch (error: IllegalArgumentException) {
        ByteArray(0)
    }

    /** The body as text when it is UTF-8 without control characters (tabs and line breaks allowed), else null. */
    fun text(bytes: ByteArray): String? {
        val decoded = try {
            Charsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(bytes))
                .toString()
        } catch (error: CharacterCodingException) {
            return null
        }
        return decoded.takeUnless { text -> text.any { it.code in CONTROL_LOW || it.code in CONTROL_HIGH || it.code == DELETE } }
    }

    fun base64(text: String): String = Base64.getEncoder().encodeToString(text.toByteArray(Charsets.UTF_8))

    fun base64(bytes: ByteArray): String = Base64.getEncoder().encodeToString(bytes)

    // The same set as asText in capture-format.ts: \u0000-\u0008 and \u000e-\u001f, and DEL.
    private val CONTROL_LOW = 0x00..0x08
    private val CONTROL_HIGH = 0x0e..0x1f
    private const val DELETE = 0x7f
}
