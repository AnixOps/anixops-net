package io.anixops.net.ui

import java.util.Base64
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class BodyTextTest {
    private fun base64(text: String) = Base64.getEncoder().encodeToString(text.toByteArray())

    @Test
    fun jsonIsIndented() {
        val body = BodyText.of(base64("""{"a":1,"b":[true]}"""), "application/json; charset=utf-8", truncated = false)
        assertEquals("{\n    \"a\": 1,\n    \"b\": [\n        true\n    ]\n}", body.text)
        assertFalse(body.truncated)
    }

    @Test
    fun brokenJsonShowsAsItCame() {
        assertEquals("{\"a\":", BodyText.text("{\"a\":".toByteArray(), "application/json"))
    }

    @Test
    fun bytesThatAreNotTextHaveNoText() {
        assertNull(BodyText.text(byteArrayOf(0x89.toByte(), 0x50, 0x4e, 0x47), "image/png"))
        assertNull(BodyText.text("a\u0000b".toByteArray(), "text/plain"))
    }

    @Test
    fun theSizeIsOfTheBytesShown() {
        val body = BodyText.of(base64("hello"), "text/plain", truncated = true)
        assertEquals("hello", body.text)
        assertEquals(5L, body.size)
        assertTrue(body.truncated)
    }
}
