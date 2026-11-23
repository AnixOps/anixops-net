package io.anixops.net.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Where a notification tap goes and which notifications replace each other (finding android-12). */
class NoticesTest {
    @Test
    fun onlyWebLinksAreOpened() {
        assertEquals("https://a.example/x", noticeLink("https://a.example/x"))
        assertEquals("HTTP://a.example", noticeLink(" HTTP://a.example "))
        assertNull(noticeLink(null))
        assertNull(noticeLink(""))
        // JSON null must not become the text "null".
        assertNull(noticeLink("null"))
        assertNull(noticeLink("javascript:alert(1)"))
        assertNull(noticeLink("file:///sdcard/a"))
        assertNull(noticeLink("intent://x#Intent;end"))
        assertNull(noticeLink("https://"))
    }

    @Test
    fun connectionAndSubscriptionNoticesReplaceScriptNoticesStack() {
        var next = 100
        val first = noticeSlot("connection") { next++ }
        assertEquals(first, noticeSlot("connection") { next++ })
        assertEquals("subscription", noticeSlot("subscription") { next++ }.tag)
        val script = noticeSlot("script") { next++ }
        val another = noticeSlot(null) { next++ }
        assertNull(script.tag)
        assertNotEquals(script.id, another.id)
    }
}
