package io.anixops.net.ui

/**
 * When the app asks the system for a permission, kept apart from the screens so the rules are tested on their own.
 * [sdk] is `Build.VERSION.SDK_INT`.
 */
object Permissions {
    /** Android 13 (API 33): notifications need a runtime permission. */
    private const val NOTIFICATIONS_SDK = 33

    /** Android 9 (API 28): the last version on which saving to Downloads needs the storage permission. */
    private const val LAST_STORAGE_SDK = 28

    /** Android 10 (API 29): location "all the time" is a permission of its own. */
    private const val BACKGROUND_LOCATION_SDK = 29

    /**
     * Whether touching a notification switch asks for the permission: only when the switch is turned on and the
     * permission is missing, never when it is turned off.
     */
    fun asksOnSwitch(sdk: Int, granted: Boolean, turningOn: Boolean): Boolean =
        sdk >= NOTIFICATIONS_SDK && !granted && turningOn

    /** Whether the first connect asks for the notification permission: once, when it is missing. */
    fun asksAfterConnect(sdk: Int, granted: Boolean, askedBefore: Boolean): Boolean =
        sdk >= NOTIFICATIONS_SDK && !granted && !askedBefore

    /** Whether saving to Downloads needs the storage permission first (Android 8 and 9). */
    fun needsStorage(sdk: Int, granted: Boolean): Boolean = sdk <= LAST_STORAGE_SDK && !granted

    /**
     * Whether the Wi-Fi rules section offers location "all the time": named rules exist, the app has precise location
     * already (the system asks for the background one only after it) and the system has the setting.
     */
    fun offersBackgroundLocation(sdk: Int, namedRules: Boolean, precise: Boolean, background: Boolean): Boolean =
        sdk >= BACKGROUND_LOCATION_SDK && namedRules && precise && !background
}
