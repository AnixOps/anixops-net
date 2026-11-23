package io.anixops.net.platform

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import io.anixops.net.R

/**
 * The app's notification channels: the VPN's ongoing notification and the notices (an unexpected disconnect, a
 * failed subscription update, the Wi-Fi rules, scripts). Made when the app starts, so they show in the system
 * settings before the first notification, and made again when the app's language changes, which renames them.
 */
object Channels {
    const val VPN = "vpn"
    const val NOTICES = "notices"

    fun ensure(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        manager.createNotificationChannels(
            listOf(
                NotificationChannel(
                    VPN,
                    context.getString(R.string.notification_channel_vpn),
                    NotificationManager.IMPORTANCE_LOW,
                ),
                NotificationChannel(
                    NOTICES,
                    context.getString(R.string.notification_channel_notices),
                    NotificationManager.IMPORTANCE_DEFAULT,
                ),
            ),
        )
    }
}
