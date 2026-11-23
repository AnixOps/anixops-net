package io.anixops.net.ui

import io.anixops.net.core.OperationResult
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The same cases as the desktop client's plugin-review.test.ts, and which imports open the review. */
class PluginReviewTest {
    @Test
    fun everySiteIsBroad() {
        assertTrue(decryptsBroadly(listOf("*")))
        assertTrue(decryptsBroadly(listOf("api.example.com", "*.*")))
        assertTrue(decryptsBroadly(listOf(" *.com ")))
    }

    @Test
    fun namedDomainsAndExclusionsAreNot() {
        assertFalse(decryptsBroadly(emptyList()))
        assertFalse(decryptsBroadly(listOf("*.example.com", "api.example.org")))
        assertFalse(decryptsBroadly(listOf("-*", "-*.com")))
    }

    @Test
    fun anImportLeftOffOpensTheReview() {
        val off = OperationResult(
            message = "Plugin imported",
            code = "plugins.imported_off",
            params = mapOf("name" to "Demo", "scripts" to "2", "id" to "demo-1"),
        )
        assertEquals("demo-1", pluginToReview(off))
    }

    @Test
    fun anImportKeptOnOrWithoutAnIdDoesNot() {
        val on = OperationResult(message = "Plugin imported", code = "plugins.imported", params = mapOf("id" to "demo-1"))
        assertNull(pluginToReview(on))
        assertNull(pluginToReview(OperationResult(message = "old client", code = "plugins.imported_off")))
        assertNull(pluginToReview(OperationResult(message = "no code")))
    }
}
