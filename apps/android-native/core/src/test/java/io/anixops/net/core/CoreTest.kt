package io.anixops.net.core

import java.io.File
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class CoreTest {
    /** A sample from crates/client-core/tests/fixtures, which the Rust contract test checks against the client. */
    private fun fixture(name: String): String {
        val dir = System.getProperty("fixtures") ?: error("the fixtures system property is not set")
        return File(dir, name).readText()
    }

    private class Recording(private val answer: String) : CoreTransport {
        val calls = mutableListOf<Pair<String, String>>()

        override fun invoke(command: String, args: String): String {
            calls += command to args
            return answer
        }
    }

    @Test
    fun theSnapshotSampleDecodes() = runTest {
        val transport = Recording(fixture("runtime_snapshot.json"))
        val core = Core(transport, StandardTestDispatcher(testScheduler))
        val snapshot = core.snapshot()
        assertEquals(listOf("runtime_snapshot" to "{}"), transport.calls)
        assertEquals(ConnectionState.Connected, snapshot.state)
        assertEquals("tokyo-01", snapshot.selectedNode)
        assertEquals("rule", snapshot.routingMode)
        assertEquals("success", snapshot.core.tone)
        assertNull(snapshot.lastError)
    }

    @Test
    fun aRefusalBecomesCoreException() = runTest {
        val core = Core(Recording(fixture("connect_error.json")), StandardTestDispatcher(testScheduler))
        val error = runCatching { core.connect() }.exceptionOrNull()
        assertTrue(error is CoreException)
        assertEquals("the current subscription has no nodes", error?.message)
    }

    @Test
    fun operationAnswersDecode() = runTest {
        val core = Core(Recording(fixture("connect_ok.json")), StandardTestDispatcher(testScheduler))
        assertEquals("Connected through tokyo-01", core.connect().message)
    }

    @Test
    fun subscriptionRowsGetTheirUpdateInterval() = runTest {
        val transport = object : CoreTransport {
            override fun invoke(command: String, args: String): String = when (command) {
                "list_subscriptions" -> """{"ok":[{"id":"s1","location":"https://example.com/sub","name":"One","updateIntervalHours":24}]}"""
                "subscriptions" -> """{"ok":{"subscriptions":[
                    {"id":"s1","name":"One","pasted":false,"selected":true,"refreshable":true,"nodes":3,"updated":null,
                     "error":null,"used":null,"total":null,"expire":null,"expireDate":null,"expired":false},
                    {"id":"p","name":"粘贴的节点","pasted":true,"selected":false,"refreshable":false,"nodes":1,"updated":null,
                     "error":"timed out","used":7,"total":null,"expire":100,"expireDate":"1970-01-01","expired":true}]}}"""
                else -> error("unexpected $command")
            }
        }
        val rows = Core(transport, StandardTestDispatcher(testScheduler)).subscriptions()
        assertEquals(listOf(24, 0), rows.map { it.intervalHours })
        assertTrue(rows[1].pasted && rows[1].expired)
        assertEquals("timed out", rows[1].error)
    }

    @Test
    fun pluginsDecodeWithTheSharedFieldNames() = runTest {
        val transport = Recording(
            """{"ok":[{"id":"p1","name":"Ads","url":"https://example.com/ads.plugin","enabled":true,"level":"partial",
                "hostnames":["api.example.com"],"scriptCount":2}]}""",
        )
        val plugin = Core(transport, StandardTestDispatcher(testScheduler)).mitmPlugins().single()
        assertEquals(listOf("list_mitm_plugins" to "{}"), transport.calls)
        assertEquals("partial", plugin.level)
        assertEquals(listOf("api.example.com"), plugin.hostnames)
        assertEquals(2, plugin.scriptCount)
    }

    @Test
    fun aPastedPluginSaysWhereItCameFrom() = runTest {
        val transport = Recording("""{"ok":{"message":"imported"}}""")
        Core(transport, StandardTestDispatcher(testScheduler)).importMitmPluginText("#!name = x", "x.plugin")
        assertEquals("import_mitm_plugin_text" to """{"text":"#!name = x","source":"x.plugin"}""", transport.calls.single())
    }

    @Test
    fun aRedirectEditSaysWhichOneItReplaces() = runTest {
        val transport = Recording("""{"ok":{"message":"saved"}}""")
        val core = Core(transport, StandardTestDispatcher(testScheduler))
        core.addTrafficRedirect(RedirectDraft(source = "203.0.113.10", target = ":7001"), null)
        core.addTrafficRedirect(RedirectDraft(source = "game.example.com", network = "udp", target = "192.0.2.1"), "r1")
        assertEquals(
            listOf(
                "add_traffic_redirect" to
                    """{"draft":{"source":"203.0.113.10","ports":"","network":"any","target":":7001"},"replacing":null}""",
                "add_traffic_redirect" to
                    """{"draft":{"source":"game.example.com","ports":"","network":"udp","target":"192.0.2.1"},"replacing":"r1"}""",
            ),
            transport.calls,
        )
    }

    @Test
    fun savedRedirectsLeaveTheEditorFieldsOut() = runTest {
        val transport = Recording("""{"ok":{"message":"saved"}}""")
        val entry = TrafficRedirect("r1", "IP-CIDR,203.0.113.10/32", "192.0.2.1:7001", enabled = false, draft = RedirectDraft())
        Core(transport, StandardTestDispatcher(testScheduler)).saveTrafficRedirects(listOf(entry))
        assertEquals(
            "save_traffic_redirects" to
                """{"entries":[{"id":"r1","match":"IP-CIDR,203.0.113.10/32","target":"192.0.2.1:7001","enabled":false}]}""",
            transport.calls.single(),
        )
    }

    @Test
    fun ruleSetsGoThroughRuntimeControl() = runTest {
        val transport = Recording(
            """{"ok":{"rule_sets":[{"name":"china","source":"remote","entries":8412,"updated_at_ms":1763198400000}]}}""",
        )
        val core = Core(transport, StandardTestDispatcher(testScheduler))
        val sets = core.ruleSets()
        assertEquals(listOf("runtime_control" to """{"request":{"cmd":"rule_sets"}}"""), transport.calls)
        assertEquals(RuleSetStatus("china", "remote", 8412, updatedAtMs = 1_763_198_400_000), sets.single())
    }

    @Test
    fun ruleSetsAreEmptyWhileTheEngineIsOff() = runTest {
        val core = Core(Recording("""{"error":"the engine is not running"}"""), StandardTestDispatcher(testScheduler))
        assertTrue(core.ruleSets().isEmpty())
    }

    @Test
    fun customDnsKeepsTheSharedFieldNames() = runTest {
        val transport = Recording("""{"ok":{"message":"saved"}}""")
        val core = Core(transport, StandardTestDispatcher(testScheduler))
        core.saveCustomDns(DnsText(servers = "a = 1.1.1.1", fakeIp = true))
        assertEquals(
            "save_custom_dns" to """{"text":{"servers":"a = 1.1.1.1","rules":"","hosts":"","fake_ip":true}}""",
            transport.calls.single(),
        )
    }

    @Test
    fun envelopesRoundTrip() {
        val answer = buildJsonObject { put("fd", JsonPrimitive(42)) }
        assertEquals("""{"ok":{"fd":42}}""", Envelope.ok(answer))
        assertEquals(answer, Envelope.read(Envelope.ok(answer)))
        assertEquals("""{"error":"no"}""", Envelope.error("no"))
        assertEquals("""{"error":"boom"}""", Envelope.answer { throw IllegalStateException("boom") })
        assertThrows(CoreException::class.java) { Envelope.read("{}") }
        assertThrows(CoreException::class.java) { Envelope.read("not json") }
        assertThrows(CoreException::class.java) { Envelope.read("""{"error":"denied"}""") }
    }

    @Test
    fun connectionStatesMapFromTheClientsWords() {
        assertEquals(ConnectionState.Connected, ConnectionState.of("Connected"))
        assertEquals(ConnectionState.Failed, ConnectionState.of("Connection failed"))
        assertEquals(ConnectionState.CoreError, ConnectionState.of("Core error"))
        assertEquals(ConnectionState.NotConnected, ConnectionState.of("Not connected"))
    }
}
