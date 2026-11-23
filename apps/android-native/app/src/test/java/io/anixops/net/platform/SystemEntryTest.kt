package io.anixops.net.platform

import android.net.VpnService
import io.anixops.net.core.ConnectionState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The Quick Settings tile's tap and the always-on VPN's start. */
class SystemEntryTest {
    @Test
    fun aTapOnTheConnectedTileDisconnects() {
        assertEquals(TileTap.Disconnect, tileTap(ConnectionState.Connected, vpnAllowed = true))
        assertEquals(TileTap.Disconnect, tileTap(ConnectionState.Connected, vpnAllowed = false))
    }

    @Test
    fun aTapConnectsOnlyOnceTheVpnIsAllowed() {
        assertEquals(TileTap.Connect, tileTap(ConnectionState.NotConnected, vpnAllowed = true))
        assertEquals(TileTap.Connect, tileTap(ConnectionState.Failed, vpnAllowed = true))
        assertEquals(TileTap.OpenApp, tileTap(ConnectionState.NotConnected, vpnAllowed = false))
    }

    @Test
    fun aTapBeforeTheStateIsReadStillDoesSomething() {
        assertEquals(TileTap.Connect, tileTap(null, vpnAllowed = true))
        assertEquals(TileTap.OpenApp, tileTap(null, vpnAllowed = false))
    }

    @Test
    fun theSystemStartsTheAlwaysOnVpnButNotTheAppsOwnStart() {
        assertTrue(alwaysOnStart(VpnService.SERVICE_INTERFACE))
        assertFalse(alwaysOnStart("io.anixops.net.vpn.START"))
        assertFalse(alwaysOnStart("io.anixops.net.vpn.WAIT"))
        // A restart after the process died carries no intent; what it does depends on what the user wanted
        // (VpnRestartTest), not on the always-on setting.
        assertFalse(alwaysOnStart(null))
    }
}
