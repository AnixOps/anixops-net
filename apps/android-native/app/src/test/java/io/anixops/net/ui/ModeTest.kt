package io.anixops.net.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The same cases as the desktop client's mode.test.ts. */
class ModeTest {
    @Test
    fun proxyModeNeverRunsWithoutANode() {
        assertFalse(wantsDirectEgress(AppMode.Proxy, DebugExit.Direct, hasNodes = false))
        assertFalse(wantsDirectEgress(AppMode.Proxy, DebugExit.Direct, hasNodes = true))
    }

    @Test
    fun debugModeWithTheDirectExitRunsWithoutANode() {
        assertTrue(wantsDirectEgress(AppMode.Debug, DebugExit.Direct, hasNodes = true))
    }

    @Test
    fun debugModeThroughNodesFallsBackToDirectWhileThereAreNone() {
        assertTrue(wantsDirectEgress(AppMode.Debug, DebugExit.Node, hasNodes = false))
        assertFalse(wantsDirectEgress(AppMode.Debug, DebugExit.Node, hasNodes = true))
    }

    @Test
    fun storedExitDefaultsToDirect() {
        assertEquals(DebugExit.Direct, DebugExit.of(null))
        assertEquals(DebugExit.Direct, DebugExit.of("other"))
        assertEquals(DebugExit.Node, DebugExit.of("node"))
    }
}
