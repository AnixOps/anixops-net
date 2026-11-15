package io.anixops.net

import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * CI's emulator smoke test (x86_64): the app starts, loads libanixops_net.so, opens the Rust client and shows
 * what it answers on the overview, proxies, rules and settings pages. A fresh install has no subscription, so each
 * page shows its empty state, which only appears once the client has answered.
 */
@RunWith(AndroidJUnit4::class)
class SmokeTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private fun text(id: Int) = compose.activity.getString(id)

    private fun waitFor(id: Int) = waitForText(text(id))

    private fun waitForText(value: String) {
        compose.waitUntil(TIMEOUT_MILLIS) {
            compose.onAllNodes(hasText(value)).fetchSemanticsNodes().isNotEmpty()
        }
    }

    @Test
    fun theClientAnswersEveryPage() {
        // A fresh install starts with the user agreement.
        waitFor(R.string.terms_accept)
        compose.onNodeWithText(text(R.string.terms_accept)).performClick()

        // The overview read through the bridge: with no node the hero card asks for one.
        waitFor(R.string.home_choose_node)
        waitFor(R.string.status_not_connected)

        compose.onNodeWithText(text(R.string.nav_proxies)).performClick()
        waitFor(R.string.proxies_empty)

        // 我的规则 shows once `my_rules` has answered; a fresh install has none.
        compose.onNodeWithText(text(R.string.nav_rules)).performClick()
        waitForText(compose.activity.resources.getQuantityString(R.plurals.rules_count, 0, 0))

        // Extensions: the plugin library and the decrypted domains answer (both empty on a new install).
        compose.onNodeWithText(text(R.string.nav_extensions)).performClick()
        waitFor(R.string.plugins_empty)

        // The notification switches show once `notification_settings` has answered.
        compose.onNodeWithText(text(R.string.nav_settings)).performClick()
        waitFor(R.string.notifications_scripts)
        waitFor(R.string.settings_version)
        compose.onNodeWithText(BuildConfig.VERSION_NAME).assertExists()
    }

    private companion object {
        const val TIMEOUT_MILLIS = 30_000L
    }
}
