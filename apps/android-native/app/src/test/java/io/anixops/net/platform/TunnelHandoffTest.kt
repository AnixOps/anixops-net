package io.anixops.net.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/** Who owns the VPN descriptor when the start and the wait for it meet (third-party review F06). */
class TunnelHandoffTest {
    @Test
    fun aDescriptorDeliveredInTimeIsTheCallers() {
        val handoff = TunnelHandoff("request")
        assertTrue(handoff.deliver(42))
        assertEquals(42, handoff.await(1))
    }

    @Test
    fun aDescriptorThatComesAfterTheCallerLeftStaysTheServices() {
        val handoff = TunnelHandoff("request")
        try {
            handoff.await(0)
            fail("a wait that ends without a descriptor throws")
        } catch (expected: IllegalStateException) {
            assertEquals("The VPN did not start in time", expected.message)
        }
        // The service must close it: the caller is gone.
        assertFalse(handoff.deliver(42))
    }

    @Test
    fun aReplacedStartEndsOnceWithItsReason() {
        val handoff = TunnelHandoff("request")
        handoff.fail("Another VPN start replaced this one")
        assertFalse(handoff.deliver(42))
        try {
            handoff.await(1)
            fail("a failed start throws")
        } catch (expected: IllegalStateException) {
            assertEquals("Another VPN start replaced this one", expected.message)
        }
    }

    @Test
    fun onlyTheFirstEndCounts() {
        val handoff = TunnelHandoff("request")
        assertTrue(handoff.deliver(7))
        handoff.fail("late failure")
        assertFalse(handoff.deliver(8))
        assertEquals(7, handoff.await(1))
    }
}
