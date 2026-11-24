package io.anixops.net.ui

import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasAnyDescendant
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import io.anixops.net.R
import io.anixops.net.core.DecryptDomain
import io.anixops.net.core.MitmPlugin
import io.anixops.net.core.WifiRule
import io.anixops.net.ui.UiFixtures.extensionsState
import io.anixops.net.ui.UiFixtures.settingsState
import io.anixops.net.ui.UiFixtures.snapshot
import io.anixops.net.ui.theme.AnixTheme
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * How much a page asks of the eye (docs/ui-ux.md, N25-N27), measured on the phone reference size 393x852 dp with the
 * richest realistic state, so a page that grows back fails here and not in a reviewer's scroll:
 *
 * - N25: the first screen of 设置 has at most [MAX_SECTIONS] sections and [MAX_ROWS] rows, and its content is no taller
 *   than two viewports. What is rarely used belongs on a sub-page.
 * - N26: a list of things the user adds shows at most [MAX_FOLDED_ITEMS] items, then folds behind "show all".
 * - N27: the master switch of a feature is not an item of the list it governs.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], qualifiers = "zh-rCN-w393dp-h852dp-xhdpi")
class DensityTest {
    @get:Rule
    val compose = createComposeRule()

    private fun text(id: Int, vararg args: Any) = RuntimeEnvironment.getApplication().getString(id, *args)

    @Test
    fun settingsFirstScreenStaysShort() {
        var density = 1f
        compose.setContent {
            density = LocalDensity.current.density
            AnixTheme {
                SettingsScreen(
                    mode = AppMode.Proxy,
                    version = "0.3.0",
                    termsVersion = "2.1.0",
                    state = settingsState,
                    actions = SettingsActions(openLanguageSettings = {}),
                )
            }
        }
        compose.waitForIdle()
        val sections = compose.onAllNodesWithTag(SECTION_TAG, useUnmergedTree = true).fetchSemanticsNodes().size
        val rows = compose.onAllNodesWithTag(ROW_TAG, useUnmergedTree = true).fetchSemanticsNodes().size
        val height = compose.onNodeWithTag(PAGE_CONTENT_TAG, useUnmergedTree = true).fetchSemanticsNode().size.height / density
        assertTrue("$sections sections on the first screen of 设置, at most $MAX_SECTIONS", sections in 1..MAX_SECTIONS)
        assertTrue("$rows rows on the first screen of 设置, at most $MAX_ROWS", rows in 1..MAX_ROWS)
        val limit = MAX_VIEWPORTS * VIEWPORT_HEIGHT
        assertTrue("${height}dp of content on the first screen of 设置, at most ${limit}dp", height <= limit)
    }

    private val plugins = extensionsState.plugins + (4..8).map { MitmPlugin(id = "p$it", name = "Plugin $it", level = "unsupported") }

    private fun showExtensions() {
        check(plugins.size == PLUGINS)
        compose.setContent {
            AnixTheme {
                ExtensionsScreen(
                    title = "扩展",
                    snapshot = snapshot.copy(httpsDecryptionEnabled = true),
                    state = extensionsState.copy(plugins = plugins),
                    actions = ExtensionsActions(),
                )
            }
        }
        compose.waitForIdle()
    }

    private val userItems = hasTestTag(USER_ITEM_TAG) and hasAnyAncestor(hasTestTag(USER_LIST_TAG))

    /** The plugin rows only: the page has other folded lists (the decrypted domains) with items of their own. */
    private val pluginItems = userItems and plugins.map { hasAnyDescendant(hasText(it.name)) }.reduce { a, b -> a or b }

    @Test
    fun longPluginListFoldsAndOpens() {
        showExtensions()
        val shown = compose.onAllNodes(pluginItems, useUnmergedTree = true).fetchSemanticsNodes().size
        assertTrue("$shown of $PLUGINS plugins shown, at most $MAX_FOLDED_ITEMS before \"show all\"", shown in 1..MAX_FOLDED_ITEMS)
        compose.onNodeWithText(RuntimeEnvironment.getApplication().resources.getQuantityString(R.plurals.plugins_show_all, PLUGINS, PLUGINS)).performScrollTo().performClick()
        compose.waitForIdle()
        compose.onAllNodes(pluginItems, useUnmergedTree = true).assertCountEquals(PLUGINS)
    }

    @Test
    fun decryptionSwitchIsNotAnItemOfTheList() {
        showExtensions()
        val label = hasText(text(R.string.decryption_switch))
        // The switch is on the page, and none of it sits inside the list it governs.
        compose.onAllNodes(label, useUnmergedTree = true).assertCountEquals(1)
        compose.onAllNodes(label and hasAnyAncestor(hasTestTag(USER_LIST_TAG)), useUnmergedTree = true).assertCountEquals(0)
    }

    private fun showAllText(count: Int) =
        RuntimeEnvironment.getApplication().resources.getQuantityString(R.plurals.list_show_all, count, count)

    @Test
    fun longWifiRuleListFoldsAndKeepsItsSwitchOut() {
        val rules = (1..PLUGINS).map { WifiRule("Wi-Fi $it", "connect") }
        val state = settingsState.copy(onDemand = settingsState.onDemand!!.copy(enabled = true, wifi = rules))
        compose.setContent {
            AnixTheme {
                WifiRulesScreen(state = state, actions = SettingsActions(), onBack = {})
            }
        }
        compose.waitForIdle()
        val shown = compose.onAllNodes(userItems, useUnmergedTree = true).fetchSemanticsNodes().size
        assertTrue("$shown of $PLUGINS Wi-Fi rules shown, at most $MAX_FOLDED_ITEMS before \"show all\"", shown in 1..MAX_FOLDED_ITEMS)
        compose.onNodeWithText(showAllText(PLUGINS)).assertExists()
        // The on/off switch stays on 设置, not among the rules (N27).
        compose.onAllNodes(hasText(text(R.string.wifi_rules_enabled)), useUnmergedTree = true).assertCountEquals(0)
    }

    @Test
    fun longHostListFolds() {
        val domains = extensionsState.domains + (1..PLUGINS).map { DecryptDomain("host$it.example.com") }
        compose.setContent {
            AnixTheme {
                ExtensionsScreen(
                    title = "扩展",
                    snapshot = snapshot,
                    state = extensionsState.copy(domains = domains),
                    actions = ExtensionsActions(),
                )
            }
        }
        compose.waitForIdle()
        // The hosts list folds behind its own "show all"; the other lists of the fixture are short.
        compose.onNodeWithText(showAllText(domains.size)).assertExists()
    }

    private companion object {
        const val MAX_SECTIONS = 8
        /** N25: the first screen of 设置 has at most 16 rows. */
        const val MAX_ROWS = 16
        const val MAX_FOLDED_ITEMS = 5
        const val MAX_VIEWPORTS = 2
        const val VIEWPORT_HEIGHT = 852
        const val PLUGINS = 8
    }
}
