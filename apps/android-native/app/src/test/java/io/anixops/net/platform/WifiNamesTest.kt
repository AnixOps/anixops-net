package io.anixops.net.platform

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The Wi-Fi name the rules see: known, hidden for now, or unknown without the permission (finding android-01). */
class WifiNamesTest {
    @Test
    fun aNameReadInFrontIsKeptOnTheSameNetworkInTheBackground() {
        val names = WifiNames()
        assertEquals(WifiName.Known("Home"), names.read(7L, "\"Home\"", mayRead = true))
        // The app went to the background: Android hides the name, the network is the same.
        assertEquals(WifiName.Known("Home"), names.read(7L, WifiNames.HIDDEN, mayRead = true))
    }

    @Test
    fun aHiddenNameOnANewNetworkIsWithheldNotOtherWifi() {
        val names = WifiNames()
        names.read(7L, "\"Home\"", mayRead = true)
        assertEquals(WifiName.Withheld, names.read(8L, WifiNames.HIDDEN, mayRead = true))
        // The old name is forgotten, also when the first network comes back with a new handle.
        assertEquals(WifiName.Withheld, names.read(7L, null, mayRead = true))
    }

    @Test
    fun withoutThePermissionTheNameIsUnknown() {
        val names = WifiNames()
        assertEquals(WifiName.Unknown, names.read(7L, null, mayRead = false))
    }

    @Test
    fun aNetworkCallbackTeachesTheName() {
        val names = WifiNames()
        names.learn(9L, "\"Cafe\"")
        names.learn(9L, WifiNames.HIDDEN)
        assertEquals(WifiName.Known("Cafe"), names.read(9L, WifiNames.HIDDEN, mayRead = true))
    }

    @Test
    fun cleansWhatAndroidReports() {
        assertEquals("Home 5G", WifiNames.clean("\"Home 5G\""))
        assertNull(WifiNames.clean(WifiNames.HIDDEN))
        assertNull(WifiNames.clean("\"\""))
        assertNull(WifiNames.clean(null))
    }
}
