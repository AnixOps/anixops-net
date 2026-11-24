package io.anixops.net.ui

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The routing modes every client offers (docs/platform-parity.md, "分流模式"): rule, global and direct, in that
 * order. Android once shipped without global; losing a mode again fails here.
 */
class RoutingModesTest {
    @Test
    fun offersRuleGlobalAndDirect() {
        assertEquals(listOf("rule", "global", "direct"), RoutingModes)
    }
}
