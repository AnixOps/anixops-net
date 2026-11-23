package io.anixops.net.ui

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** When the app asks for permissions (findings android-01, android-04, android-05). */
class PermissionsTest {
    @Test
    fun turningASwitchOffNeverAsksForNotifications() {
        assertTrue(Permissions.asksOnSwitch(sdk = 33, granted = false, turningOn = true))
        assertFalse(Permissions.asksOnSwitch(sdk = 33, granted = false, turningOn = false))
        assertFalse(Permissions.asksOnSwitch(sdk = 33, granted = true, turningOn = true))
        assertFalse(Permissions.asksOnSwitch(sdk = 32, granted = false, turningOn = true))
    }

    @Test
    fun theFirstConnectAsksOnce() {
        assertTrue(Permissions.asksAfterConnect(sdk = 34, granted = false, askedBefore = false))
        assertFalse(Permissions.asksAfterConnect(sdk = 34, granted = false, askedBefore = true))
        assertFalse(Permissions.asksAfterConnect(sdk = 34, granted = true, askedBefore = false))
        assertFalse(Permissions.asksAfterConnect(sdk = 32, granted = false, askedBefore = false))
    }

    @Test
    fun onlyAndroid8And9NeedTheStoragePermission() {
        assertTrue(Permissions.needsStorage(sdk = 26, granted = false))
        assertTrue(Permissions.needsStorage(sdk = 28, granted = false))
        assertFalse(Permissions.needsStorage(sdk = 28, granted = true))
        assertFalse(Permissions.needsStorage(sdk = 29, granted = false))
    }

    @Test
    fun backgroundLocationIsOfferedOnlyForNamedRulesAfterPreciseLocation() {
        assertTrue(Permissions.offersBackgroundLocation(sdk = 29, namedRules = true, precise = true, background = false))
        assertFalse(Permissions.offersBackgroundLocation(sdk = 29, namedRules = false, precise = true, background = false))
        assertFalse(Permissions.offersBackgroundLocation(sdk = 29, namedRules = true, precise = false, background = false))
        assertFalse(Permissions.offersBackgroundLocation(sdk = 29, namedRules = true, precise = true, background = true))
        assertFalse(Permissions.offersBackgroundLocation(sdk = 28, namedRules = true, precise = true, background = false))
    }
}
