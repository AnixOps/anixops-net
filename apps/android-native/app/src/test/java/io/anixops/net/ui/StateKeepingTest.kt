package io.anixops.net.ui

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.StateRestorationTester
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextReplacement
import io.anixops.net.R
import io.anixops.net.core.DebugMapping
import io.anixops.net.core.MyRules
import io.anixops.net.core.Overview
import io.anixops.net.ui.theme.AnixTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * What the user typed and has not saved stays when another page shows and when the activity is recreated (a
 * rotation, dark mode, the language): finding android-08.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], qualifiers = "en")
class StateKeepingTest {
    @get:Rule
    val compose = createComposeRule()

    private fun string(id: Int) = RuntimeEnvironment.getApplication().getString(id)

    @Test
    fun myRulesDraftSurvivesAnotherPage() {
        val state = UiState(overview = Overview(state = "disconnected", egress = "direct", canConnect = true), nodeCount = 1)
        compose.setContent {
            AnixTheme {
                AnixScaffold(
                    state = state,
                    mode = AppMode.Proxy,
                    version = "0",
                    onModeChange = {},
                    rulesState = RulesState(myRules = MyRules()),
                    initial = Destination.Rules,
                )
            }
        }
        val draft = "DOMAIN-SUFFIX,unsaved.example,DIRECT"
        compose.onNode(hasSetTextAction() and hasText(string(R.string.rules_mine), substring = true))
            .performTextReplacement(draft)
        compose.onNode(hasText(string(R.string.nav_proxies)) and hasClickAction()).performClick()
        compose.waitForIdle()
        compose.onAllNodesWithText(draft).assertCountEquals(0)
        compose.onNode(hasText(string(R.string.nav_rules)) and hasClickAction()).performClick()
        compose.waitForIdle()
        compose.onAllNodesWithText(draft).assertCountEquals(1)
    }

    @Test
    fun theMappingEditorKeepsEveryFieldWhenRecreated() {
        val saved = DebugMapping(id = "m1", pattern = "https://old.example/api", body = "")
        val draft = MappingDraft(saved)
        val restoration = StateRestorationTester(compose)
        restoration.setContent {
            AnixTheme {
                MappingEditorScreen(saved, busy = false, onSave = {}, onBack = {}, editorDraft = draft)
            }
        }
        compose.onNode(hasSetTextAction() and hasText("https://old.example/api")).performTextReplacement("https://new.example/api")
        restoration.emulateSavedInstanceStateRestore()
        compose.onAllNodesWithText("https://new.example/api").assertCountEquals(1)
        compose.onAllNodesWithText("https://old.example/api").assertCountEquals(0)
    }

    @Test
    fun theUnsavedStateOfAPageOpenedFromAnotherHasItsOwnKey() {
        assertEquals("Settings", pageKey(Destination.Settings, null, egressRules = false))
        assertEquals("Settings/Logs", pageKey(Destination.Settings, SettingsPage.Logs, egressRules = false))
        assertEquals("Settings/Advanced", pageKey(Destination.Settings, SettingsPage.Advanced, egressRules = false))
        assertEquals("Egress/rules", pageKey(Destination.Egress, null, egressRules = true))
        assertEquals("Rules", pageKey(Destination.Rules, SettingsPage.Logs, egressRules = true))
    }
}
