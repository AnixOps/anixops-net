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
        assertTrue(alwaysOnStart(null))
        assertFalse(alwaysOnStart("io.anixops.net.vpn.START"))
    }
}
