package io.anixops.net.ui

import io.anixops.net.core.CaptureChunk
import io.anixops.net.core.CaptureConnection
import io.anixops.net.core.MappingText
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CaptureBytesTest {
    @Test
    fun aHexDumpShowsOffsetsBytesAndPrintableCharacters() {
        val dump = CaptureBytes.hexDump("GET / HTTP/1.1\r\nHost: a\r\n".toByteArray())
        assertEquals(
            "00000000  47 45 54 20 2f 20 48 54 54 50 2f 31 2e 31 0d 0a  GET / HTTP/1.1..\n" +
                "00000010  48 6f 73 74 3a 20 61 0d 0a                       Host: a..",
            dump,
        )
    }

    @Test
    fun aLongDumpStopsAtTheLimit() {
        val dump = CaptureBytes.hexDump(ByteArray(40), limit = 16)
        assertEquals(2, dump.lines().size)
        assertEquals("…", dump.lines().last())
    }

    @Test
    fun aTlsHandshakeIsCiphertext() {
        fun first(bytes: ByteArray) = CaptureConnection(chunks = listOf(CaptureChunk("upload", 0, MappingText.base64(bytes))))
        assertTrue(CaptureBytes.looksLikeTls(first(byteArrayOf(0x16, 0x03, 0x01, 0x02, 0x00))))
        assertFalse(CaptureBytes.looksLikeTls(first("GET / HTTP/1.1".toByteArray())))
        assertFalse(CaptureBytes.looksLikeTls(CaptureConnection()))
    }
}
