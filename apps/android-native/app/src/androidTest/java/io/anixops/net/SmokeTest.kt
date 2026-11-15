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
 * what it answers on the overview, proxies and settings pages. A fresh install has no subscription, so each
 * page shows its empty state, which only appears once the client has answered.
 */
@RunWith(AndroidJUnit4::class)
class SmokeTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private fun text(id: Int) = compose.activity.getString(id)

    private fun waitFor(id: Int) {
        compose.waitUntil(TIMEOUT_MILLIS) {
            compose.onAllNodes(hasText(text(id))).fetchSemanticsNodes().isNotEmpty()
        }
    }

    @Test
    fun theClientAnswersEveryPage() {
        // The overview read through the bridge: with no node the hero card asks for one.
        waitFor(R.string.home_choose_node)
        waitFor(R.string.status_not_connected)

        compose.onNodeWithText(text(R.string.nav_proxies)).performClick()
        waitFor(R.string.proxies_empty)

        compose.onNodeWithText(text(R.string.nav_settings)).performClick()
        waitFor(R.string.settings_version)
        compose.onNodeWithText(BuildConfig.VERSION_NAME).assertExists()
    }

    private companion object {
        const val TIMEOUT_MILLIS = 30_000L
    }
}
