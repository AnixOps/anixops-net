package io.anixops.net.ui

import io.anixops.net.core.Core
import io.anixops.net.core.CoreTransport
import io.anixops.net.core.SubscriptionRow
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestCoroutineScheduler
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * The search reads the nodes once typing pauses, and only for the latest query; a saved dialog comes back about the
 * same subscription (finding android-11, android-08).
 */
@OptIn(ExperimentalCoroutinesApi::class)
class ProxiesSearchTest {
    private val scheduler = TestCoroutineScheduler()
    private val main = StandardTestDispatcher(scheduler)

    @Before
    fun setUp() = Dispatchers.setMain(main)

    @After
    fun tearDown() = Dispatchers.resetMain()

    @Test
    fun onlyTheLastQueryIsRead() = runTest(main) {
        val queries = mutableListOf<String>()
        val transport = CoreTransport { command, args ->
            when (command) {
                "list_subscriptions" -> """{"ok":[]}"""
                "subscriptions" -> """{"ok":{"subscriptions":[]}}"""
                "list_nodes" -> """{"ok":[]}"""
                "view.proxies" -> {
                    queries += Json.parseToJsonElement(args).jsonObject.getValue("query").jsonPrimitive.content
                    """{"ok":{"total":0,"shown":0,"sections":[]}}"""
                }
                else -> """{"error":"unexpected $command"}"""
            }
        }
        val proxies = ProxiesViewModel(Core(transport, main))
        proxies.search("a")
        advanceTimeBy(ProxiesViewModel.SEARCH_PAUSE_MILLIS / 2)
        proxies.search("ab")
        advanceUntilIdle()
        assertEquals(listOf("ab"), queries)
        assertEquals("ab", proxies.state.value.query)
        assertTrue(proxies.state.value.loaded)
    }

    @Test
    fun aSavedDialogIsRestoredOnlyWhileWhatItIsAboutExists() {
        val subscription = SubscriptionRow(id = "s1", name = "Home")
        assertEquals("Rename", restoreProxiesDialogKind(listOf("rename", "s1"), listOf(subscription)))
        assertNull(restoreProxiesDialogKind(listOf("rename", "gone"), listOf(subscription)))
        assertEquals("Import", restoreProxiesDialogKind(listOf("import", "https://sub.example"), emptyList()))
        assertEquals(
            "Edit",
            restoreProxiesDialogKind(listOf("edit", "n1", "Tokyo", "Tokyo", "a.example", "443", ""), emptyList()),
        )
        assertNull(restoreProxiesDialogKind(listOf("edit", "n1"), emptyList()))
    }
}
