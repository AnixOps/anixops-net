package io.anixops.net.platform

import org.junit.Assert.assertEquals
import org.junit.Test

/** What the VPN service does when the system restarts it after the process died (findings android-02, android-06). */
class VpnRestartTest {
    @Test
    fun aRestartDoesWhatTheUserLastWanted() {
        assertEquals(Restart.Reconnect, restartAction(VpnWant.Connected, recentRestarts = 0))
        assertEquals(Restart.Wait, restartAction(VpnWant.Waiting, recentRestarts = 0))
        // Disconnected by hand (rules off) or taken away by another VPN: stays off.
        assertEquals(Restart.Stop, restartAction(VpnWant.None, recentRestarts = 0))
    }

    @Test
    fun anEngineThatKeepsDyingIsNotRestartedForever() {
        assertEquals(Restart.Reconnect, restartAction(VpnWant.Connected, MAX_RESTARTS - 1))
        assertEquals(Restart.GiveUp, restartAction(VpnWant.Connected, MAX_RESTARTS))
        assertEquals(5, MAX_RESTARTS)
        // Waiting for the network starts no engine, so it never gives up.
        assertEquals(Restart.Wait, restartAction(VpnWant.Waiting, MAX_RESTARTS + 5))
    }

    @Test
    fun onlyRecentRestartsCount() {
        val now = 10 * RESTART_WINDOW_MILLIS
        val stamps = listOf(now - RESTART_WINDOW_MILLIS - 1, now - 1000, now)
        assertEquals(listOf(now - 1000, now), recentStamps(stamps, now, RESTART_WINDOW_MILLIS))
    }

    @Test
    fun storedWantsReadBackAndUnknownOnesAreNone() {
        VpnWant.entries.forEach { assertEquals(it, VpnWant.of(it.name)) }
        assertEquals(VpnWant.None, VpnWant.of(null))
        assertEquals(VpnWant.None, VpnWant.of("connected"))
    }
}
