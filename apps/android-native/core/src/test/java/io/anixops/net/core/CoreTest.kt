package io.anixops.net.core

import java.io.File
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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

    /** Words for a few codes, as the app's resources give them (finding ux-i18n-a11y-02). */
    private object Chinese : CoreTexts {
        private val words = mapOf(
            "connect.connected_via" to "已通过 {node} 连接",
            "decrypt.decrypting" to "将解密 {host}",
            "plugins.applied_next" to "下次连接时生效",
            "error.name_required" to "请输入名称",
        )

        override fun text(code: String, params: Map<String, String>): String? =
            words[code]?.let { CoreTexts.fill(it, params) }

        override fun join(first: String, then: String): String = "$first。$then"

        override fun failed(detail: String): String = "操作没有完成：$detail"

        override fun unreadable(): String = "客户端返回了无法识别的结果"
    }

    @Test
    fun resultsAreWordedByTheirCode() = runTest {
        val core = Core(Recording(fixture("connect_ok.json")), StandardTestDispatcher(testScheduler), Chinese)
        val result = core.connect()
        assertEquals("已通过 tokyo-01 连接", result.message)
        assertEquals("connect.connected_via", result.code)
        // What was changed, then what that did to the connection.
        val followed = Core(
            Recording("""{"ok":{"message":"Decrypting a.example","code":"decrypt.decrypting","params":{"host":"a.example"},"follow":"plugins.applied_next"}}"""),
            StandardTestDispatcher(testScheduler),
            Chinese,
        ).connect()
        assertEquals("将解密 a.example。下次连接时生效", followed.message)
        // A code the app does not know keeps the client's words.
        val unknown = Core(
            Recording("""{"ok":{"message":"Something new","code":"future.thing"}}"""),
            StandardTestDispatcher(testScheduler),
            Chinese,
        ).connect()
        assertEquals("Something new", unknown.message)
    }

    @Test
    fun refusalsAreWordedAndKeepTheEnglishAsDetail() = runTest {
        val coded = runCatching {
            Core(Recording(fixture("rename_subscription_error.json")), StandardTestDispatcher(testScheduler), Chinese).connect()
        }.exceptionOrNull() as CoreException
        assertEquals("请输入名称", coded.message)
        assertEquals("error.name_required", coded.code)
        assertEquals("error.name_required: enter a name", coded.detail)
        // No code: what did not finish, never the client's English alone.
        val plain = runCatching {
            Core(Recording(fixture("connect_error.json")), StandardTestDispatcher(testScheduler), Chinese).connect()
        }.exceptionOrNull() as CoreException
        assertEquals("操作没有完成：the current subscription has no nodes", plain.message)
        assertNull(plain.code)
        // Without words of its own the app shows the English, without the code.
        val english = runCatching {
            Core(Recording(fixture("rename_subscription_error.json")), StandardTestDispatcher(testScheduler)).connect()
        }.exceptionOrNull() as CoreException
        assertEquals("enter a name", english.message)
    }

    @Test
    fun anUnreadableAnswerIsSaidInTheAppsLanguage() = runTest {
        val error = runCatching {
            Core(Recording("""{"ok":{"no":"message"}}"""), StandardTestDispatcher(testScheduler), Chinese).connect()
        }.exceptionOrNull() as CoreException
        assertEquals("客户端返回了无法识别的结果", error.message)
        assertTrue(error.detail.orEmpty().startsWith("Unexpected answer to connect"))
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
    fun aCheckupAsksForTheDownloadOnlyWhenWantedAndReadsItsRows() = runTest {
        val transport = Recording(
            """{"ok":{"name":"Tokyo","report":{"latency":{}},"sections":[{"id":"latency","capped":false,"rows":[""" +
                """{"id":"handshake","kind":"ms","value":42.4,"warn":false},""" +
                """{"id":"loss","kind":"pct","value":20.0,"warn":true},""" +
                """{"id":"failure","kind":"failure","value":"timeout","warn":true},""" +
                """{"id":"request","kind":"ms","value":null,"warn":false}]}]}}""",
        )
        val core = Core(transport, StandardTestDispatcher(testScheduler))
        val checkup = core.nodeCheckup("n1", throughput = false)
        core.nodeCheckup("n1", throughput = true)
        assertEquals(
            listOf(
                "node_checkup" to """{"nodeId":"n1"}""",
                "node_checkup" to """{"nodeId":"n1","options":{"throughput":{"max_bytes":200000000}}}""",
            ),
            transport.calls,
        )
        assertEquals("Tokyo", checkup.name)
        val rows = checkup.sections.single().rows
        assertEquals(listOf("handshake", "loss", "failure", "request"), rows.map { it.id })
        assertTrue(rows[1].warn)
        assertEquals(JsonPrimitive("timeout"), rows[2].value)
        assertEquals(JsonNull, rows[3].value ?: JsonNull)
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

    /** Answers the request list's commands as the client would, recording what each was asked. */
    private class RequestsTransport(private val engine: String) : CoreTransport {
        val calls = mutableListOf<Pair<String, String>>()

        override fun invoke(command: String, args: String): String {
            calls += command to args
            return when (command) {
                "view.next_since" -> """{"ok":0}"""
                "runtime_control" -> engine
                "view.merge_requests" -> """{"ok":[{"seq":0,"connection_id":3,"index":0,"method":"GET","state":"in_progress"}]}"""
                "view.next_rev" -> """{"ok":5}"""
                "view.request_rows" -> """{"ok":[{"seq":0,"connection_id":3,"index":0,"method":"GET","path":"/a","tone":"pending"}]}"""
                else -> error("unexpected $command")
            }
        }
    }

    @Test
    fun theRequestListReadsByRevisionThroughTheFilter() = runTest {
        val transport = RequestsTransport("""{"ok":{"requests":[{"seq":0,"connection_id":3,"index":0,"rev":5}],"last_seq":0,"last_rev":5}}""")
        val page = Core(transport, StandardTestDispatcher(testScheduler)).requests(RequestFeed(), "type:json", "seq", true)
        assertEquals(
            """{"request":{"cmd":"capture_requests","since_seq":0,"since_rev":0,"limit":2000,"filter":"type:json"}}""",
            transport.calls.first { it.first == "runtime_control" }.second,
        )
        assertEquals(5L, page.feed.lastRev)
        assertEquals(1, page.feed.rows.size)
        assertEquals(RequestKey(3, 0), page.shown.single().key)
        assertEquals("pending", page.shown.single().tone)
        assertNull(page.error)
    }

    @Test
    fun aNewEngineStartsTheListOver() = runTest {
        // Read up to rev 120 from the engine before a restart; the new one has counted to 3.
        val transport = RequestsTransport("""{"ok":{"requests":[],"last_seq":2,"last_rev":3}}""")
        val feed = RequestFeed(lastSeq = 40, lastRev = 120)
        val page = Core(transport, StandardTestDispatcher(testScheduler)).requests(feed, "", "seq", true)
        assertEquals(RequestFeed(), page.feed)
        assertTrue(page.shown.isEmpty())
        assertTrue(transport.calls.none { it.first == "view.merge_requests" })
    }

    @Test
    fun engineRestartsAreToldByCountersGoingBack() {
        assertTrue(engineRestarted(RequestFeed(lastSeq = 40, lastRev = 120), 2, 3))
        assertFalse(engineRestarted(RequestFeed(lastSeq = 40, lastRev = 120), 40, 120))
        assertFalse(engineRestarted(RequestFeed(), 0, 0))
        assertTrue(engineRestarted(RequestFeed(lastSeq = 40), 3, null))
        assertFalse(engineRestarted(RequestFeed(lastSeq = 40), null, null))
    }

    @Test
    fun aRefusedFilterKeepsWhatWasRead() = runTest {
        val transport = RequestsTransport("""{"ok":{"error":{"code":"invalid_filter","at":7}}}""")
        val feed = RequestFeed(lastSeq = 9, lastRev = 4)
        val page = Core(transport, StandardTestDispatcher(testScheduler)).requests(feed, "status:", "seq", true)
        assertEquals(feed, page.feed)
        assertEquals(FilterError("invalid_filter", at = 7), page.error)
        assertTrue(transport.calls.none { it.first == "view.merge_requests" })
    }

    @Test
    fun oneRequestExportsWithItsExchangeAndLanguage() = runTest {
        val transport = Recording("""{"ok":{"location":"Download/api.example.com.go"}}""")
        val saved = Core(transport, StandardTestDispatcher(testScheduler)).exportExchange(3, 1, "code", language = "go", redact = false)
        assertEquals(
            listOf("export_capture" to """{"format":"code","id":3,"options":{"exchange":1,"language":"go","redact":false}}"""),
            transport.calls,
        )
        assertEquals(SavedFile("Download/api.example.com.go"), saved)
    }

    @Test
    fun aReplayKeepsTheSharedFieldNames() = runTest {
        val transport = Recording(
            """{"ok":{"status":201,"headers":[["content-type","application/json"]],"bodyBase64":"e30=","truncated":false,"elapsedMs":184}}""",
        )
        val answer = Core(transport, StandardTestDispatcher(testScheduler))
            .replay(ReplayRequest("POST", "https://api.example.com/v1", listOf(listOf("accept", "*/*")), "e30="))
        assertEquals(
            listOf(
                "replay_request" to
                    """{"request":{"method":"POST","url":"https://api.example.com/v1","headers":[["accept","*/*"]],"bodyBase64":"e30="}}""",
            ),
            transport.calls,
        )
        assertEquals(201, answer.status)
        assertEquals(184L, answer.elapsedMs)
        assertEquals("application/json", answer.message.header("content-type"))
    }

    @Test
    fun theWholeCaptureExportsWithoutAnId() = runTest {
        val transport = Recording("""{"ok":{"location":"Download/capture.pcapng"}}""")
        Core(transport, StandardTestDispatcher(testScheduler)).exportCapture("pcapng")
        assertEquals(listOf("export_capture" to """{"format":"pcapng"}"""), transport.calls)
    }

    @Test
    fun sessionCommandsNameTheirAction() = runTest {
        val transport = Recording(
            """{"ok":{"sessions":[{"session":"1700000000000-a1b2c3d4","saved_at_ms":1700000000000,"expires_at_ms":1700604800000,""" +
                """"size":2048,"connections":2,"requests":9}],"protection":"keystore","retention_days":7}}""",
        )
        val core = Core(transport, StandardTestDispatcher(testScheduler))
        val list = core.captureSessions()
        assertEquals("keystore", list.protection)
        assertEquals(9, list.sessions.single().requests)
        core.saveCaptureSession("  login  ")
        core.saveCaptureSession(" ")
        core.loadCaptureSession("1700000000000-a1b2c3d4")
        core.deleteCaptureSession("1700000000000-a1b2c3d4")
        assertEquals(
            listOf(
                """{"action":{"action":"list"}}""",
                """{"action":{"action":"save","name":"login"}}""",
                """{"action":{"action":"save"}}""",
                """{"action":{"action":"load","session":"1700000000000-a1b2c3d4"}}""",
                """{"action":{"action":"delete","session":"1700000000000-a1b2c3d4"}}""",
            ),
            transport.calls.map { it.second },
        )
        assertTrue(transport.calls.all { it.first == "capture_session" })
    }

    @Test
    fun theChecklistAsksTheSharedView() = runTest {
        val transport = Recording("""{"ok":{"steps":["domains","capture"]}}""")
        val steps = Core(transport, StandardTestDispatcher(testScheduler)).decryptSetup(true, true, 0, capturing = false)
        assertEquals(
            listOf("view.decrypt_setup" to """{"connected":true,"decrypting":true,"domains":0,"capturing":false}"""),
            transport.calls,
        )
        assertEquals(listOf("domains", "capture"), steps)
    }

    @Test
    fun aConnectionReadsThroughRuntimeControl() = runTest {
        val transport = Recording(
            """{"ok":{"capture":{"id":7,"network":"tcp","destination":"203.0.113.20:443","decrypted":true,""" +
                """"upload":{"seen":10,"captured":10,"truncated":false},"download":{"seen":20,"captured":8,"truncated":true},""" +
                """"chunks":[{"dir":"upload","at_ms":1,"data_base64":"R0VU"}],"http":[]}}}""",
        )
        val capture = Core(transport, StandardTestDispatcher(testScheduler)).captureConnection(7)
        assertEquals(listOf("runtime_control" to """{"request":{"cmd":"capture_connection","id":7}}"""), transport.calls)
        assertEquals(true, capture?.download?.truncated)
        assertEquals("R0VU", capture?.chunks?.single()?.dataBase64)
    }

    @Test
    fun aConnectionCaptureCanBeGone() = runTest {
        assertNull(Core(Recording("""{"ok":{"capture":null}}"""), StandardTestDispatcher(testScheduler)).captureConnection(7))
    }

    @Test
    fun oneConnectionExportsWithItsIdAndTheHostIsAskedOfTheSharedView() = runTest {
        val transport = Recording("""{"ok":{"location":"Download/x.pcapng","decryptable":true,"host":"a.example.com","covered":false}}""")
        val core = Core(transport, StandardTestDispatcher(testScheduler))
        core.exportConnection(7, "pcapng", redact = true)
        val host = core.decryptHost("a.example.com:443", listOf("*.example.org"))
        assertEquals(
            listOf(
                "export_capture" to """{"format":"pcapng","id":7,"options":{"redact":true}}""",
                "view.decrypt_hosts" to """{"host":"a.example.com:443","hosts":["*.example.org"]}""",
            ),
            transport.calls,
        )
        assertEquals(DecryptHost(true, "a.example.com", false), host)
    }

    @Test
    fun appLinksAreReadByTheSharedView() = runTest {
        val transport = Recording("""{"ok":{"action":"select","node":"东京 01","group":"Panel"}}""")
        val link = Core(transport, StandardTestDispatcher(testScheduler)).parseAppLink("anixops://select?node=东京 01&group=Panel")
        assertEquals(listOf("view.parse_app_link" to """{"link":"anixops://select?node=东京 01&group=Panel"}"""), transport.calls)
        assertEquals(AppLink(action = "select", node = "东京 01", group = "Panel"), link)
    }

    @Test
    fun aRefusedLinkSaysWhy() = runTest {
        val core = Core(Recording("""{"ok":{"error":"unknown_mode","detail":"fast"}}"""), StandardTestDispatcher(testScheduler))
        assertEquals(AppLink(error = "unknown_mode", detail = "fast"), core.parseAppLink("anixops://mode?set=fast"))
    }

    @Test
    fun aLinkNodeIsFoundAmongTheNodesListed() = runTest {
        val transport = Recording("""{"ok":null}""")
        val core = Core(transport, StandardTestDispatcher(testScheduler))
        val nodes = listOf(NodeSummary(id = "n1", label = "东京 01", protocol = "VLESS", group = "Panel"))
        assertNull(core.findLinkNode(nodes, "大阪", null))
        assertNull(core.findLinkMember(listOf(NativeGroup("Proxy", "selector", outbounds = listOf("n1"))), "Proxy", "n2"))
        assertEquals(
            listOf(
                "view.find_link_node" to
                    """{"nodes":[{"id":"n1","label":"东京 01","protocol":"VLESS","selected":false,"group":"Panel","editable":false}],"name":"大阪"}""",
                "view.find_link_member" to
                    """{"groups":[{"tag":"Proxy","groupType":"selector","outbounds":["n1"],"outboundLabels":[]}],"policy":"Proxy","member":"n2"}""",
            ),
            transport.calls,
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
    fun ruleSetsCarryTheLinesTheyCouldNotHold() = runTest {
        val transport = Recording(
            """{"ok":{"rule_sets":[{"name":"telegram","source":"remote","entries":40,"skipped":{"SCRIPT":1,"IP-ASN":12,"GEOIP":1}}]}}""",
        )
        val set = Core(transport, StandardTestDispatcher(testScheduler)).ruleSets().single()
        assertEquals(mapOf("SCRIPT" to 1, "IP-ASN" to 12, "GEOIP" to 1), set.skipped)
        assertEquals(14, set.skippedCount)
        assertEquals("IP-ASN ×12, GEOIP ×1, SCRIPT ×1", set.skippedSummary)
    }

    @Test
    fun routingWarningsListWhatTheImportLeftOut() = runTest {
        val transport = Recording(
            """{"ok":["4 IP-ASN rules were not imported","RULE-SET,SYSTEM is not available"]}""",
        )
        val warnings = Core(transport, StandardTestDispatcher(testScheduler)).routingWarnings()
        assertEquals("routing_warnings", transport.calls.single().first)
        assertEquals(listOf("4 IP-ASN rules were not imported", "RULE-SET,SYSTEM is not available"), warnings)
    }

    @Test
    fun ruleSetsAreEmptyWhileTheEngineIsOff() = runTest {
        val core = Core(Recording("""{"error":"the engine is not running"}"""), StandardTestDispatcher(testScheduler))
        assertTrue(core.ruleSets().isEmpty())
    }

    @Test
    fun ruleSetsNameTheDataTheRulesLack() = runTest {
        val transport = Recording("""{"ok":{"rule_sets":[],"missing":["asn-13335","geoip-us"]}}""")
        val view = Core(transport, StandardTestDispatcher(testScheduler)).ruleSetsView()
        assertEquals(listOf("asn-13335", "geoip-us"), view.missing)
        assertEquals(listOf("IP-ASN,13335", "GEOIP,US"), view.missing.map(::dataRule))
    }

    @Test
    fun dataListNamesComeFromWhatTheUserTyped() {
        for (text in listOf("us", "US", "GEOIP,US", "geoip-us", " GeoIP, us ")) assertEquals("geoip-us", dataName(text))
        for (text in listOf("13335", "AS13335", "IP-ASN,13335", "ip-asn,AS13335", "asn-13335")) {
            assertEquals("asn-13335", dataName(text))
        }
        for (text in listOf("", "usa", "geoip-1a", "0", "AS013", "4294967296", "geosite-cn")) assertNull(text, dataName(text))
        assertEquals("asn-4294967295", dataName("4294967295"))
        assertTrue(hasProcessRules("DOMAIN,a.test,PROXY\n process-name,curl,DIRECT"))
        assertFalse(hasProcessRules("DOMAIN,process-name.test,PROXY"))
        assertTrue(hasRuleKind("DOMAIN,a.test,PROXY\ndevice-name, TV ,DIRECT", "DEVICE-NAME"))
        assertFalse(hasRuleKind("DEVICE-NAMES,TV,DIRECT", "DEVICE-NAME"))
    }

    @Test
    fun dataSourcesKeepTheSharedFieldNames() = runTest {
        val transport = Recording("""{"ok":{"message":"saved"}}""")
        val core = Core(transport, StandardTestDispatcher(testScheduler))
        core.saveDataSource("geoip-us", "https://lists.test/us.txt")
        core.removeDataSource("geoip-us")
        assertEquals(
            listOf(
                "save_data_source" to """{"name":"geoip-us","url":"https://lists.test/us.txt"}""",
                "remove_data_source" to """{"name":"geoip-us"}""",
            ),
            transport.calls,
        )
    }

    @Test
    fun ruleHitsAndRouteTestsGoThroughRuntimeControl() = runTest {
        val hits = Recording(
            """{"ok":{"generation":2,"rules":[{"rule":"DOMAIN,a.test","target":"Proxy","hits":0},""" +
                """{"rule":"GEOIP,CN","target":"DIRECT","hits":7,"last_hit_at_ms":5}],"final":{"rule":"FINAL","target":"Proxy","hits":3}}}""",
        )
        val view = Core(hits, StandardTestDispatcher(testScheduler)).ruleHits()!!
        assertEquals(listOf("runtime_control" to """{"request":{"cmd":"rule_hits"}}"""), hits.calls)
        assertEquals(listOf("GEOIP,CN"), view.busiest().map { it.rule })
        assertEquals(1, view.unused)
        assertEquals(3L, view.finalRule.hits)

        val test = Recording(
            """{"ok":{"mode":"rule","test":{"rule_index":1,"rule":"GEOIP,CN","chain":["Proxy"],"outbound":"n1","needs_address":true}}}""",
        )
        val answer = Core(test, StandardTestDispatcher(testScheduler)).testRoute("a.test", 443, "tcp", " ", null)
        assertEquals(
            listOf("runtime_control" to """{"request":{"cmd":"test_route","host":"a.test","port":443,"network":"tcp"}}"""),
            test.calls,
        )
        assertTrue(answer.test.needsAddress)
        assertEquals("Proxy → n1", answer.test.path)
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
