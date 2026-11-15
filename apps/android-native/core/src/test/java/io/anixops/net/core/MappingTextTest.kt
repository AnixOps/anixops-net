package io.anixops.net.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MappingTextTest {
    @Test
    fun headerLinesRoundTrip() {
        val headers = listOf(HeaderField("Content-Type", "application/json"), HeaderField("X-Debug", "a: b"))
        assertEquals("Content-Type: application/json\nX-Debug: a: b", MappingText.headerLines(headers))
        assertEquals(headers, MappingText.parseHeaderLines(MappingText.headerLines(headers)))
    }

    @Test
    fun linesWithoutANameAreLeftOut() {
        val parsed = MappingText.parseHeaderLines("\n:path: /x\nno colon\r\n  Accept :  */*  \r\n")
        assertEquals(listOf(HeaderField("Accept", "*/*")), parsed)
    }

    @Test
    fun textBodiesAreText() {
        assertEquals("{\"ok\":true}\n\tdone", MappingText.text(MappingText.bytes(MappingText.base64("{\"ok\":true}\n\tdone"))))
        assertEquals("中文", MappingText.text("中文".toByteArray()))
    }

    @Test
    fun binaryBodiesAreNot() {
        assertNull(MappingText.text(byteArrayOf(0x89.toByte(), 0x50, 0x4e, 0x47)))
        assertNull(MappingText.text(byteArrayOf(0x41, 0x00, 0x42)))
        assertNull(MappingText.text(byteArrayOf(0x7f)))
    }

    @Test
    fun aBodyThatIsNotBase64IsEmpty() {
        assertEquals(0, MappingText.bytes("not base64!").size)
    }
}
