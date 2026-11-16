package io.anixops.net.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import io.anixops.net.core.Overview
import io.anixops.net.ui.theme.AnixTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * The entries every page must keep, whatever the navigation becomes (docs/ui-ux.md, "入口不变量"). Debug mode once
 * lost its only connection switch on iOS when its overview went away; these checks fail before a change like that
 * ships here:
 *
 * - The connection switch: on 首页 in proxy mode, and on every page of debug mode, which has no 首页. Exactly one.
 * - A failure (a refused command, the VPN consent, the connection's error) shows on the page the user is on.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], qualifiers = "zh-rCN")
class EntryTest {
    @get:Rule
    val compose = createComposeRule()

    private data class Case(val mode: AppMode, val destination: Destination, val state: UiState)

    private val disconnected = UiState(overview = Overview(state = "disconnected", egress = "direct", canConnect = true), nodeCount = 1)

    private fun check(cases: List<Case>, verify: (Case) -> Unit) {
        var case by mutableStateOf(cases.first())
        compose.setContent {
            AnixTheme {
                // A new composition per case, so each opens on its own page.
                key(case) {
                    AnixScaffold(state = case.state, mode = case.mode, version = "0", onModeChange = {}, initial = case.destination)
                }
            }
        }
        for (next in cases) {
            case = next
            compose.waitForIdle()
            verify(next)
        }
    }

    private fun every(mode: AppMode, state: UiState) = Destination.of(mode).map { Case(mode, it, state) }

    @Test
    fun debugModeHasTheConnectionSwitchOnEveryPage() = check(every(AppMode.Debug, disconnected)) {
        compose.onAllNodesWithTag(CONNECT_SWITCH_TAG, useUnmergedTree = true)
            .assertCountEquals(1)
    }

    @Test
    fun proxyModeHasTheConnectionSwitchOnTheHomePage() =
        check(listOf(Case(AppMode.Proxy, Destination.Home, disconnected))) {
            compose.onAllNodesWithTag(CONNECT_SWITCH_TAG, useUnmergedTree = true).assertCountEquals(1)
        }

    @Test
    fun debugModeShowsAFailureOnEveryPage() =
        check(every(AppMode.Debug, disconnected.copy(notice = "VPN refused"))) {
            compose.onAllNodesWithTag(NOTICE_TAG, useUnmergedTree = true).assertCountEquals(1)
        }

    @Test
    fun debugModeShowsTheConnectionErrorOnEveryPage() =
        check(every(AppMode.Debug, disconnected.copy(overview = disconnected.overview!!.copy(state = "failed", error = "handshake failed")))) {
            compose.onAllNodesWithText("handshake failed", useUnmergedTree = true).assertCountEquals(1)
        }

    @Test
    fun proxyModeShowsAFailureOnEveryPage() = check(every(AppMode.Proxy, disconnected.copy(notice = "save failed"))) { case ->
        if (case.destination == Destination.Home) {
            compose.onAllNodesWithTag(NOTICE_TAG, useUnmergedTree = true).assertCountEquals(1)
        } else {
            // Elsewhere it comes up in the snackbar.
            compose.waitUntil(5_000) {
                compose.onAllNodesWithText("save failed", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()
            }
        }
    }

    @Test
    fun homeOffersTheImportWhileThereIsNothingToConnectWith() {
        val empty = UiState(overview = Overview(state = "disconnected", egress = "node", canConnect = false), nodeCount = 0)
        check(listOf(Case(AppMode.Proxy, Destination.Home, empty))) {
            // No disabled switch that cannot say why: the card asks for an import instead.
            compose.onAllNodesWithTag(CONNECT_SWITCH_TAG, useUnmergedTree = true).assertCountEquals(0)
            compose.onAllNodesWithText("导入订阅", useUnmergedTree = true).assertCountEquals(1)
        }
    }
}
