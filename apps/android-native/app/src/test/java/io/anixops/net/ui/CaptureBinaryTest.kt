package io.anixops.net.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Base64
import kotlin.random.Random

/** The same cases as `binary-view.test.ts` and `CaptureBinaryTests.swift`: the three must print the same text. */
class CaptureBinaryTest {
    private fun bytes(vararg parts: Any): ByteArray {
        val out = mutableListOf<Byte>()
        for (part in parts) {
            when (part) {
                is String -> out += part.toByteArray(Charsets.UTF_8).toList()
                is IntArray -> out += part.map { it.toByte() }
                is ByteArray -> out += part.toList()
                is Int -> out += part.toByte()
                else -> error("unexpected part $part")
            }
        }
        return out.toByteArray()
    }

    private fun ints(vararg values: Int) = values

    private val labels = CaptureBinary.Labels(
        message = { index, size -> "message $index ($size)" },
        compressed = "compressed",
        trailers = "trailers",
        undecoded = "not protobuf",
        more = "…",
    )

    private fun decoded(data: ByteArray, cut: Boolean = false) = CaptureBinary.decodeProtobuf(data, cut)?.let { CaptureBinary.protoText(it, "…") }

    private val sample = bytes(
        ints(0x08, 0x96, 0x01),
        ints(0x12, 0x07), "testing",
        ints(0x1a, 0x03, 0x08, 0x96, 0x01),
        ints(0x21, 0x18, 0x2d, 0x44, 0x54, 0xfb, 0x21, 0x09, 0x40),
        ints(0x2d, 0x00, 0x00, 0x80, 0x3f),
        ints(0x32, 0x03, 0x00, 0xff, 0x10),
        ints(0x38, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01),
        ints(0x42, 0x00),
        ints(0x4a, 0x05), "a\"b\n\\",
    )

    @Test
    fun printsEveryWireTypeLikeDecodeRaw() {
        assertEquals(
            listOf(
                "1: 150",
                "2: \"testing\"",
                "3 {",
                "  1: 150",
                "}",
                "4: 0x400921fb54442d18 (fixed64)",
                "5: 0x3f800000 (fixed32)",
                "6: <00 ff 10> (3 B)",
                "7: 18446744073709551615 (-1)",
                "8: \"\"",
                "9: \"a\\\"b\\n\\\\\"",
            ).joinToString("\n"),
            decoded(sample),
        )
    }

    @Test
    fun showsLongBytesByTheirStartAndSize() {
        val blob = IntArray(40) { if (it == 0) 0 else 0x80 + it }
        val head = blob.take(32).joinToString(" ") { "%02x".format(it) }
        assertEquals("1: <$head …> (40 B)", decoded(bytes(ints(0x0a, 40), blob)))
    }

    @Test
    fun refusesBytesThatAreNotAWholeMessage() {
        assertNull(CaptureBinary.decodeProtobuf(bytes(0x08)))
        assertNull(CaptureBinary.decodeProtobuf(bytes(0x0a, 0x05, 0x01)))
        assertNull(CaptureBinary.decodeProtobuf(bytes(0x0f, 0x00)))
        assertNull(CaptureBinary.decodeProtobuf(bytes(0x0b, 0x0c)))
        assertNull(CaptureBinary.decodeProtobuf(bytes(0x00, 0x01)))
        assertNull(CaptureBinary.decodeProtobuf(bytes(0x08, IntArray(11) { 0xff }, 0x01)))
        assertNull(CaptureBinary.decodeProtobuf(bytes(0x08, IntArray(9) { 0xff }, 0x02)))
    }

    @Test
    fun keepsTheFieldsBeforeTheCutOfACutBody() {
        assertNull(decoded(bytes(ints(0x08, 0x96, 0x01, 0x12, 0x07), "tes")))
        assertEquals("1: 150\n…", decoded(bytes(ints(0x08, 0x96, 0x01, 0x12, 0x07), "tes"), cut = true))
    }

    @Test
    fun emptyIsAnEmptyMessageButNeverOfferedForUnknownBytes() {
        assertEquals(CaptureBinary.ProtoMessage(emptyList(), false), CaptureBinary.decodeProtobuf(ByteArray(0)))
        assertNull(CaptureBinary.looksLikeProtobuf(ByteArray(0)))
        assertNotNull(CaptureBinary.looksLikeProtobuf(sample))
        assertNull(CaptureBinary.looksLikeProtobuf(bytes(0xff, 0xfe)))
    }

    @Test
    fun opensMessagesInsideMessagesOnlySoDeep() {
        var inner = bytes(0x08, 0x01)
        repeat(CaptureBinary.MAX_DEPTH + 4) { inner = bytes(0x0a, inner.size, inner) }
        val text = decoded(inner).orEmpty()
        assertEquals(CaptureBinary.MAX_DEPTH - 1, text.lines().count { it.endsWith("{") })
        assertTrue(text.contains("B)"))
    }

    @Test
    fun stopsAtTheFieldLimitAndSaysSo() {
        val many = ByteArray((CaptureBinary.MAX_FIELDS + 500) * 2) { if (it % 2 == 0) 0x08 else 0x01 }
        val message = CaptureBinary.decodeProtobuf(many)
        assertEquals(CaptureBinary.MAX_FIELDS, message?.fields?.size)
        assertEquals(true, message?.cut)
        val nested = bytes(ints(0x0a, 0xa0, 0x9c, 0x01), many.copyOf(20_000), ints(0x10, 0x01))
        assertEquals(false, CaptureBinary.decodeProtobuf(nested)?.cut)
        val over = bytes(ints(0x0a, 0xd0, 0xb8, 0x02), many.copyOf(40_016))
        assertEquals(true, CaptureBinary.decodeProtobuf(over)?.cut)
    }

    @Test
    fun knowsTheMediaTypes() {
        assertTrue(CaptureBinary.isProtobufType("application/x-protobuf"))
        assertTrue(CaptureBinary.isProtobufType("application/vnd.foo+proto"))
        assertFalse(CaptureBinary.isProtobufType("application/grpc+proto"))
        assertTrue(CaptureBinary.isGrpcType("application/grpc"))
        assertTrue(CaptureBinary.isGrpcType("application/grpc-web+proto"))
        assertFalse(CaptureBinary.isGrpcType("application/grpc-web-text"))
    }

    private fun frame(flag: Int, data: ByteArray) = bytes(flag, 0, 0, (data.size shr 8) and 0xff, data.size and 0xff, data)

    @Test
    fun decodesGrpcMessagesAndShowsTrailers() {
        val body = bytes(frame(0, bytes(0x08, 0x96, 0x01)), frame(1, bytes(1, 2, 3)), frame(0x80, bytes("grpc-status:0\r\n")))
        val frames = CaptureBinary.grpcFrames(body)!!
        assertEquals(3, frames.frames.size)
        assertEquals(
            "# message 1 (3)\n1: 150\n\n# message 2 (3)\ncompressed\n\n# trailers\ngrpc-status:0",
            CaptureBinary.grpcText(frames, labels),
        )
    }

    @Test
    fun keepsACutGrpcMessageAndRefusesWhatIsNotFramed() {
        val cut = frame(0, bytes(0x08, 0x96, 0x01, 0x10, 0x02)).copyOf(9)
        assertEquals("# message 1 (5)\n1: 150\n…\n\n…", CaptureBinary.grpcText(CaptureBinary.grpcFrames(cut)!!, labels))
        assertNull(CaptureBinary.grpcFrames(bytes(0x02, 0, 0, 0, 0)))
        assertNull(CaptureBinary.grpcFrames(bytes(0, 0, 0)))
        assertEquals("# message 1 (1)\nnot protobuf", CaptureBinary.grpcText(CaptureBinary.grpcFrames(frame(0, bytes(0xff)))!!, labels))
    }

    @Test
    fun recognisesPicturesByTheirFirstBytes() {
        assertEquals("image/png", CaptureBinary.sniffImage(bytes(0x89, "PNG\r\n\u001a\n", ints(0, 0, 0, 13), "IHDR")))
        assertEquals("image/jpeg", CaptureBinary.sniffImage(bytes(0xff, 0xd8, 0xff, 0xe0)))
        assertEquals("image/gif", CaptureBinary.sniffImage(bytes("GIF89a")))
        assertEquals("image/webp", CaptureBinary.sniffImage(bytes("RIFF", ints(0, 0, 0, 0), "WEBPVP8 ")))
        assertEquals("image/avif", CaptureBinary.sniffImage(bytes(ints(0, 0, 0, 0x1c), "ftypavif")))
        assertEquals("image/bmp", CaptureBinary.sniffImage(bytes("BM", IntArray(12), ints(40, 0, 0, 0), IntArray(8))))
        assertNull(CaptureBinary.sniffImage(bytes("BMx")))
        assertNull(CaptureBinary.sniffImage(bytes("hello")))
    }

    private fun base64(data: ByteArray) = Base64.getEncoder().encodeToString(data)

    @Test
    fun bodyKindsForBinaryData() {
        assertEquals(listOf(BodyMode.Protobuf, BodyMode.Hex), BodyText.of(base64(sample), "application/x-protobuf", truncated = false).modes)
        val grpc = BodyText.of(base64(bytes(0, 0, 0, 0, 3, 0x08, 0x96, 0x01)), "application/grpc", truncated = false).kind
        assertNotNull((grpc as BodyKind.Protobuf).grpc)
        assertEquals(listOf(BodyMode.Hex, BodyMode.Protobuf), BodyText.of(base64(sample), "application/octet-stream", truncated = false).modes)
        assertEquals(BodyKind.Binary, BodyText.of(base64(bytes(0xff, 0xff)), "application/x-protobuf", truncated = false).kind)
        val jpeg = BodyText.of(base64(bytes(0xff, 0xd8, 0xff, 0xe0)), "application/octet-stream", truncated = false)
        assertEquals(BodyKind.Image, jpeg.kind)
        assertEquals("image/jpeg", jpeg.contentType)
        val form = BodyText.of(base64(bytes("a=1&b=two+words&c=%zz")), "application/x-www-form-urlencoded", truncated = false)
        assertEquals(BodyKind.Form(listOf("a" to "1", "b" to "two words", "c" to "%zz")), form.kind)
        assertEquals(listOf(BodyMode.Table, BodyMode.Raw, BodyMode.Hex), form.modes)
    }

    @Test
    fun opensOnlyTypesThatRunNoCode() {
        assertEquals("png", CaptureBinary.fileType("image/png").first)
        assertEquals("json", CaptureBinary.fileType("application/problem+json; charset=utf-8").first)
        assertEquals("bin", CaptureBinary.fileType("application/x-protobuf").first)
        for (opens in listOf("image/png", "image/jpeg", "application/pdf", "text/plain; charset=utf-8", "application/json", "audio/mpeg")) {
            assertTrue(opens, CaptureBinary.fileType(opens).second)
        }
        for (saved in listOf("image/svg+xml", "text/html", "text/javascript", "application/xml", "text/csv", "application/octet-stream", "")) {
            assertFalse(saved, CaptureBinary.fileType(saved).second)
        }
        assertFalse(CaptureBinary.fileType(null).second)
    }

    /** Random and damaged input, from a fixed seed so a failure can be run again. */
    @Test
    fun neverFailsOnUntrustedBytes() {
        val random = Random(20261122)
        val seeds = listOf(
            sample,
            bytes(0, 0, 0, 0, 3, 0x08, 0x96, 0x01, 0x80, 0, 0, 0, 2, 0x61, 0x62),
            bytes(0x89, "PNG\r\n\u001a\n", ints(0, 0, 0, 13), "IHDR"),
            bytes("RIFF", ints(0, 0, 0, 0), "WEBPVP8L", IntArray(12) { 0xff }),
        )
        repeat(4000) { round ->
            var data = if (round % 2 == 0) {
                random.nextBytes(random.nextInt(600))
            } else {
                val changed = seeds[round % seeds.size].copyOf()
                repeat(1 + random.nextInt(4)) { if (changed.isNotEmpty()) changed[random.nextInt(changed.size)] = random.nextInt(256).toByte() }
                changed
            }
            if (round % 2 == 1 && random.nextInt(10) < 3) data = data.copyOf(random.nextInt(data.size + 1))
            if (round % 2 == 1 && random.nextInt(10) < 2) data += data
            val message = CaptureBinary.decodeProtobuf(data, random.nextBoolean())
            if (message != null) assertTrue("round $round", CaptureBinary.protoText(message, "…").length < 200 * (data.size + 1))
            CaptureBinary.looksLikeProtobuf(data)
            CaptureBinary.grpcFrames(data)?.let { CaptureBinary.grpcText(it, labels) }
            CaptureBinary.sniffImage(data)
            BodyText.kind(data, "application/octet-stream", null, cut = true)
            BodyText.kind(data, "application/grpc", null, cut = true)
        }
    }
}
