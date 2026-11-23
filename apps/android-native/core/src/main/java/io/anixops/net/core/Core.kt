package io.anixops.net.core

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.nullable
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

/**
 * Typed commands over the client. Every call runs on [dispatcher] (commands may block for long: downloads,
 * delay tests, the VPN consent) and throws [CoreException] when the client refuses. Names and arguments are the
 * shared interface's (`apps/windows-gui/ui/src/bridge.ts`); page logic is the client's `view.*` commands
 * (crates/client-view), so every client shows the same thing. Results and refusals come back in the app's language
 * ([texts]): a screen shows [OperationResult.message] and [CoreException.message] as they are.
 */
class Core(
    private val transport: CoreTransport,
    private val dispatcher: CoroutineDispatcher = Dispatchers.IO,
    private val texts: CoreTexts = CoreTexts.English,
) {
    suspend fun snapshot(): RuntimeSnapshot = call("runtime_snapshot", RuntimeSnapshot.serializer())

    suspend fun connect(): OperationResult = call("connect", OperationResult.serializer())

    suspend fun disconnect(): OperationResult = call("disconnect", OperationResult.serializer())

    // The overview page.

    /** The snapshot, the node list and (while connected) the traffic counters, read as the overview page. */
    suspend fun overview(): OverviewData {
        val snapshot = raw("runtime_snapshot")
        val nodes = raw("list_nodes")
        val connected = decode(RuntimeSnapshot.serializer(), snapshot).state == ConnectionState.Connected
        // The engine answers its counters only while it runs; a race with a disconnect reads as no traffic.
        val traffic = if (connected) orNull { control("traffic") } ?: JsonNull else JsonNull
        val overview = call(
            "view.overview",
            Overview.serializer(),
            buildJsonObject {
                put("snapshot", snapshot)
                put("nodes", nodes)
                put("traffic", traffic)
            },
        ).let { it.copy(error = it.error?.let { raw -> texts.refusalText(raw) }) }
        val connections = if (connected) {
            orNull { decode(ConnectionsSnapshot.serializer(), control("connections")) }
        } else {
            null
        }
        return OverviewData(
            decode(RuntimeSnapshot.serializer(), snapshot),
            overview,
            connections?.active.orEmpty(),
            nodeCount = (nodes as? JsonArray)?.size ?: 0,
        )
    }

    /** Debug mode's direct exit: the engine runs with no node. */
    suspend fun setDirectEgress(enabled: Boolean): OperationResult =
        call("set_direct_egress", OperationResult.serializer(), args("enabled" to JsonPrimitive(enabled)))

    suspend fun setRoutingMode(mode: String): OperationResult =
        call("set_routing_mode", OperationResult.serializer(), args("mode" to JsonPrimitive(mode)))

    suspend fun closeConnection(id: Long) {
        controlRequest(buildJsonObject { put("cmd", "close_connection"); put("id", id) })
    }

    suspend fun closeAllConnections() {
        controlRequest(buildJsonObject { put("cmd", "close_all") })
    }

    suspend fun networkProbe(connected: Boolean): NetworkProbe =
        call("network_probe", NetworkProbe.serializer(), args("connected" to JsonPrimitive(connected)))

    // The proxies page.

    suspend fun nodes(): List<NodeSummary> = call("list_nodes", ListSerializer(NodeSummary.serializer()))

    /** The node list as the proxies page shows it for [query], with the last [delays]. */
    suspend fun proxies(query: String, delays: Map<String, NodeDelay>): ProxiesPage {
        val nodes = raw("list_nodes")
        val view = call(
            "view.proxies",
            ProxiesView.serializer(),
            buildJsonObject {
                put("nodes", nodes)
                putJsonObject("delays") {
                    delays.forEach { (id, delay) -> put(id, Envelope.json.encodeToJsonElement(NodeDelay.serializer(), delay)) }
                }
                put("query", query)
            },
        )
        val editable = withContext(dispatcher) {
            decode(ListSerializer(NodeSummary.serializer()), nodes, "list_nodes")
                .filter { it.editable }
                .mapTo(mutableSetOf()) { it.id }
        }
        return ProxiesPage(view, editable)
    }

    suspend fun switchNode(nodeId: String): OperationResult =
        call("switch_node", OperationResult.serializer(), args("nodeId" to JsonPrimitive(nodeId)))

    suspend fun testDelays(nodeIds: List<String>, url: String = DELAY_URL): List<NodeDelay> = call(
        "test_node_delays",
        ListSerializer(NodeDelay.serializer()),
        buildJsonObject {
            putJsonArray("nodeIds") { nodeIds.forEach { add(JsonPrimitive(it)) } }
            put("url", url)
        },
    )

    suspend fun selectFastestNode(): OperationResult = call("select_fastest_node", OperationResult.serializer())

    /**
     * Node checkup (节点体检) of one node: latency rounds and, with [throughput], a download capped at
     * [CHECKUP_MAX_BYTES]. Answers when it is over, which takes seconds. Only the user starts it.
     */
    suspend fun nodeCheckup(nodeId: String, throughput: Boolean): NodeCheckup = call(
        "node_checkup",
        NodeCheckup.serializer(),
        buildJsonObject {
            put("nodeId", nodeId)
            if (throughput) {
                putJsonObject("options") {
                    putJsonObject("throughput") { put("max_bytes", CHECKUP_MAX_BYTES) }
                }
            }
        },
    )

    suspend fun shareLink(nodeId: String): String =
        call("node_share_link", String.serializer(), args("nodeId" to JsonPrimitive(nodeId)))

    suspend fun nodeForm(nodeId: String): NodeForm =
        call("node_form", NodeForm.serializer(), args("nodeId" to JsonPrimitive(nodeId)))

    suspend fun editNode(nodeId: String, name: String, server: String, port: String): OperationResult = call(
        "edit_node",
        OperationResult.serializer(),
        args(
            "nodeId" to JsonPrimitive(nodeId),
            "name" to JsonPrimitive(name),
            "server" to JsonPrimitive(server),
            "port" to JsonPrimitive(port),
        ),
    )

    suspend fun removeNode(nodeId: String): OperationResult =
        call("remove_node", OperationResult.serializer(), args("nodeId" to JsonPrimitive(nodeId)))

    /** The subscriptions as the proxies page lists them (`view.subscriptions`), with their update intervals. */
    suspend fun subscriptions(): List<SubscriptionRow> {
        val intervals = call("list_subscriptions", ListSerializer(SubscriptionInterval.serializer()))
            .associate { it.id to it.updateIntervalHours }
        // A row's error is the client's refusal as it kept it (`<code>: <English>`): shown in the app's words.
        return call("subscriptions", SubscriptionsView.serializer()).subscriptions
            .map { it.copy(intervalHours = intervals[it.id] ?: 0, error = it.error?.let { raw -> texts.refusalText(raw) }) }
    }

    suspend fun importSubscription(location: String): OperationResult =
        call("import_subscription", OperationResult.serializer(), args("location" to JsonPrimitive(location)))

    suspend fun selectSubscription(id: String): OperationResult =
        call("select_subscription", OperationResult.serializer(), args("id" to JsonPrimitive(id)))

    suspend fun updateSubscription(id: String): OperationResult =
        call("update_subscription_source", OperationResult.serializer(), args("id" to JsonPrimitive(id)))

    suspend fun renameSubscription(id: String, name: String): OperationResult = call(
        "rename_subscription",
        OperationResult.serializer(),
        args("id" to JsonPrimitive(id), "name" to JsonPrimitive(name)),
    )

    suspend fun setSubscriptionInterval(id: String, hours: Int): OperationResult = call(
        "set_subscription_interval",
        OperationResult.serializer(),
        args("id" to JsonPrimitive(id), "hours" to JsonPrimitive(hours)),
    )

    suspend fun removeSubscription(id: String): OperationResult =
        call("remove_subscription", OperationResult.serializer(), args("id" to JsonPrimitive(id)))

    // The rules page.

    suspend fun myRules(): MyRules = call("my_rules", MyRules.serializer())

    suspend fun saveMyRules(text: String): OperationResult =
        call("save_my_rules", OperationResult.serializer(), args("text" to JsonPrimitive(text)))

    /** What importing the subscription's rules left out, one line each; empty without a subscription. */
    suspend fun routingWarnings(): List<String> = call("routing_warnings", ListSerializer(String.serializer()))

    suspend fun nativeGroups(): List<NativeGroup> =
        call("list_native_groups", ListSerializer(NativeGroup.serializer()))

    suspend fun selectGroupOutbound(groupTag: String, outboundTag: String): OperationResult = call(
        "select_native_group_outbound",
        OperationResult.serializer(),
        args("groupTag" to JsonPrimitive(groupTag), "outboundTag" to JsonPrimitive(outboundTag)),
    )

    /** The running engine's rule sets; empty while it does not run. */
    suspend fun ruleSets(): List<RuleSetStatus> = ruleSetsView().ruleSets

    /** The running engine's rule sets and the GEOIP / IP-ASN data its rules lack; empty while it does not run. */
    suspend fun ruleSetsView(): RuleSetsView =
        orNull { decode(RuleSetsView.serializer(), control("rule_sets"), "rule_sets") } ?: RuleSetsView()

    /** How often each rule decided a connection since the rules were loaded; null while the engine does not run. */
    suspend fun ruleHits(): RuleHitsView? = orNull { decode(RuleHitsView.serializer(), control("rule_hits"), "rule_hits") }

    suspend fun clearRuleHits(): RuleHitsView =
        decode(RuleHitsView.serializer(), control("clear_rule_hits"), "clear_rule_hits")

    /**
     * Which rule would decide a connection to [host] and where it goes, without opening or counting one. [address]
     * is the IP the domain resolves to, for rules that need it.
     */
    suspend fun testRoute(host: String, port: Int, network: String, address: String?, process: String?): RouteTestAnswer =
        decode(
            RouteTestAnswer.serializer(),
            controlRequest(
                buildJsonObject {
                    put("cmd", "test_route")
                    put("host", host)
                    put("port", port)
                    put("network", network)
                    address?.takeIf { it.isNotBlank() }?.let { put("address", it.trim()) }
                    process?.takeIf { it.isNotBlank() }?.let { put("process", it.trim()) }
                },
            ),
            "test_route",
        )

    /** The GEOIP and IP-ASN data lists added by URL. */
    suspend fun dataSources(): List<DataSource> = call("data_sources", ListSerializer(DataSource.serializer()))

    /** Points [name] (`geoip-us`, `asn-13335`) at an https:// list; a running engine restarts. */
    suspend fun saveDataSource(name: String, url: String): OperationResult = call(
        "save_data_source",
        OperationResult.serializer(),
        args("name" to JsonPrimitive(name), "url" to JsonPrimitive(url)),
    )

    suspend fun removeDataSource(name: String): OperationResult =
        call("remove_data_source", OperationResult.serializer(), args("name" to JsonPrimitive(name)))

    /** Starts downloading the remote rule sets again; answers how many. */
    suspend fun updateRuleSets(): Int = decode(ScheduledAnswer.serializer(), control("update_rule_sets"), "update_rule_sets").scheduled

    suspend fun setDnsPreset(preset: String): OperationResult =
        call("set_dns_preset", OperationResult.serializer(), args("preset" to JsonPrimitive(preset)))

    suspend fun setBlockQuic(enabled: Boolean): OperationResult =
        call("set_block_quic", OperationResult.serializer(), args("enabled" to JsonPrimitive(enabled)))

    suspend fun customDns(): DnsText = call("custom_dns", DnsText.serializer())

    suspend fun saveCustomDns(text: DnsText): OperationResult = call(
        "save_custom_dns",
        OperationResult.serializer(),
        args("text" to Envelope.json.encodeToJsonElement(DnsText.serializer(), text)),
    )

    // Extensions / 改写: HTTPS decryption, the decrypted domains and the plugins.

    /** Turns decryption on; on Android the root certificate is saved to Downloads for the user to install. */
    suspend fun enableHttpsMitm(): SavedFileResult = call("enable_https_mitm", SavedFileResult.serializer())

    suspend fun disableHttpsMitm(): OperationResult = call("disable_https_mitm", OperationResult.serializer())

    /** The decryption list's patterns that are on and not exclusions. */
    suspend fun decryptHosts(): List<String> = call("list_decrypt_hosts", ListSerializer(String.serializer()))

    /** The whole decryption list, one line per pattern, in the order added. */
    suspend fun decryptDomains(): List<DecryptDomain> =
        call("list_decrypt_domains", ListSerializer(DecryptDomain.serializer()))

    /** Adds a line; [exclude] (or a leading `-`) makes it an exclusion, which wins over every include. */
    suspend fun addDecryptHost(host: String, exclude: Boolean = false): OperationResult = call(
        "add_decrypt_host",
        OperationResult.serializer(),
        args("host" to JsonPrimitive(host), "exclude" to JsonPrimitive(exclude)),
    )

    suspend fun editDecryptHost(pattern: String, host: String, exclude: Boolean): OperationResult = call(
        "edit_decrypt_host",
        OperationResult.serializer(),
        args("pattern" to JsonPrimitive(pattern), "host" to JsonPrimitive(host), "exclude" to JsonPrimitive(exclude)),
    )

    suspend fun setDecryptHostEnabled(pattern: String, enabled: Boolean): OperationResult = call(
        "set_decrypt_host_enabled",
        OperationResult.serializer(),
        args("pattern" to JsonPrimitive(pattern), "enabled" to JsonPrimitive(enabled)),
    )

    suspend fun removeDecryptHost(host: String): OperationResult =
        call("remove_decrypt_host", OperationResult.serializer(), args("host" to JsonPrimitive(host)))

    /** What decryption did per domain since the engine started; [clear] forgets it first. */
    suspend fun mitmHosts(clear: Boolean = false): MitmHosts =
        call(if (clear) "clear_mitm_hosts" else "mitm_hosts", MitmHosts.serializer())

    suspend fun mitmPlugins(): List<MitmPlugin> = call("list_mitm_plugins", ListSerializer(MitmPlugin.serializer()))

    suspend fun importMitmPlugin(url: String): OperationResult =
        call("import_mitm_plugin", OperationResult.serializer(), args("url" to JsonPrimitive(url)))

    /** A plugin opened from a file ([source] is its name) or pasted ([source] is empty). */
    suspend fun importMitmPluginText(text: String, source: String): OperationResult = call(
        "import_mitm_plugin_text",
        OperationResult.serializer(),
        args("text" to JsonPrimitive(text), "source" to JsonPrimitive(source)),
    )

    suspend fun mitmPluginDetails(id: String): MitmPluginDetails =
        call("mitm_plugin_details", MitmPluginDetails.serializer(), args("id" to JsonPrimitive(id)))

    suspend fun setMitmPluginEnabled(id: String, enabled: Boolean): OperationResult = call(
        "set_mitm_plugin_enabled",
        OperationResult.serializer(),
        args("id" to JsonPrimitive(id), "enabled" to JsonPrimitive(enabled)),
    )

    suspend fun removeMitmPlugin(id: String): OperationResult =
        call("remove_mitm_plugin", OperationResult.serializer(), args("id" to JsonPrimitive(id)))

    /** The Map Local / Map Remote mappings. */
    suspend fun debugMappings(): List<DebugMapping> =
        call("list_debug_mappings", ListSerializer(DebugMapping.serializer()))

    /** Adds a mapping ([DebugMapping.id] empty) or changes one; its domain joins the decrypted domains. */
    suspend fun saveDebugMapping(entry: DebugMapping): OperationResult = call(
        "save_debug_mapping",
        OperationResult.serializer(),
        args("entry" to Envelope.json.encodeToJsonElement(DebugMapping.serializer(), entry)),
    )

    suspend fun removeDebugMapping(id: String): OperationResult =
        call("remove_debug_mapping", OperationResult.serializer(), args("id" to JsonPrimitive(id)))

    /** The destination rewrites, in order. */
    suspend fun trafficRedirects(): List<TrafficRedirect> =
        call("traffic_redirects", ListSerializer(TrafficRedirect.serializer()))

    /** Adds a rewrite from the editor's fields, or changes the one with id [replacing]; a running engine restarts. */
    suspend fun addTrafficRedirect(draft: RedirectDraft, replacing: String?): OperationResult = call(
        "add_traffic_redirect",
        OperationResult.serializer(),
        args(
            "draft" to Envelope.json.encodeToJsonElement(RedirectDraft.serializer(), draft),
            "replacing" to (replacing?.let(::JsonPrimitive) ?: JsonNull),
        ),
    )

    /** Saves the list as changed on the page (switches, removals). */
    suspend fun saveTrafficRedirects(entries: List<TrafficRedirect>): OperationResult = call(
        "save_traffic_redirects",
        OperationResult.serializer(),
        args(
            "entries" to Envelope.json.encodeToJsonElement(
                ListSerializer(SavedRedirect.serializer()),
                entries.map { SavedRedirect(it.id, it.match, it.target, it.enabled) },
            ),
        ),
    )

    // The settings page.

    /** The notification switches, changed to [update] first when given; [language] sets that of the texts. */
    suspend fun notificationSettings(language: String, update: NotificationSettings? = null): NotificationSettings = call(
        "notification_settings",
        NotificationSettings.serializer(),
        args("update" to encodeOrNull(NotificationSettings.serializer(), update), "language" to JsonPrimitive(language)),
    )

    /** The Wi-Fi rules, changed to [update] first when given. */
    suspend fun onDemandSettings(update: OnDemandSettings? = null): OnDemandSettings = call(
        "on_demand_settings",
        OnDemandSettings.serializer(),
        args("update" to encodeOrNull(OnDemandSettings.serializer(), update)),
    )

    /** The network underneath; the app asks for the location permission itself, so [ask] stays false here. */
    suspend fun currentNetwork(): CurrentNetwork =
        call("current_network", CurrentNetwork.serializer(), args("ask" to JsonPrimitive(false)))

    suspend fun validateConfiguration(): OperationResult = call("validate_configuration", OperationResult.serializer())

    suspend fun restartCore(): OperationResult = call("restart_service", OperationResult.serializer())

    suspend fun health(): HealthStatus = call("health_status", HealthStatus.serializer())

    suspend fun setCrashReports(enabled: Boolean): OperationResult =
        call("set_crash_reports", OperationResult.serializer(), args("enabled" to JsonPrimitive(enabled)))

    /** Writes the diagnostics report to Downloads; the answer names the file. */
    suspend fun createDiagnostics(): SavedFileResult = call("create_diagnostics", SavedFileResult.serializer())

    // Activity: the app's log and the traffic statistics.

    /** The log lines at or above [level] (`all`, `warn`, `error`), of [module] (empty for all), containing [query]. */
    suspend fun logs(level: String, module: String, query: String): LogsView {
        val entries = raw("app_logs", args("clear" to JsonPrimitive(false)))
        return call(
            "view.logs",
            LogsView.serializer(),
            args(
                "entries" to entries,
                "level" to JsonPrimitive(level),
                "module" to JsonPrimitive(module),
                "query" to JsonPrimitive(query),
            ),
        )
    }

    suspend fun clearLogs() {
        raw("app_logs", args("clear" to JsonPrimitive(true)))
    }

    /**
     * The traffic statistics for [period] (`today` or `week`) at [nowSeconds], in the time zone [offsetSeconds]
     * east of UTC.
     */
    suspend fun trafficStats(period: String, nowSeconds: Long, offsetSeconds: Int): TrafficPage {
        val hours = raw("traffic_stats", args("clear" to JsonPrimitive(false))).let { answer ->
            (answer as? JsonObject)?.get("hours") as? JsonArray ?: JsonArray(emptyList())
        }
        val view = call(
            "view.traffic_stats",
            TrafficStatsView.serializer(),
            args(
                "hours" to hours,
                "period" to JsonPrimitive(period),
                "now" to JsonPrimitive(nowSeconds),
                "offset" to JsonPrimitive(offsetSeconds),
            ),
        )
        return TrafficPage(view, recorded = hours.isNotEmpty())
    }

    suspend fun clearTrafficStats() {
        raw("traffic_stats", args("clear" to JsonPrimitive(true)))
    }

    // Debug › Requests: the decrypted requests the engine captured.

    /** Whether capture is on and how much it holds; null while the engine is not running. */
    suspend fun captureStatus(): CaptureStatus? = orNull { decode(CaptureStatus.serializer(), control("capture")) }

    suspend fun setCapture(enabled: Boolean): CaptureStatus = decode(
        CaptureStatus.serializer(),
        controlRequest(
            buildJsonObject {
                put("cmd", "set_capture")
                put("enabled", enabled)
            },
        ),
    )

    suspend fun clearCapture(): CaptureStatus = decode(CaptureStatus.serializer(), control("clear_capture"))

    /** The engine's filter expression for the quick filters [quick] (by id) and what the user [typed]. */
    suspend fun combineFilter(quick: Collection<String>, typed: String): String = call(
        "view.combine_filter",
        String.serializer(),
        args("quick" to JsonArray(quick.map(::JsonPrimitive)), "typed" to JsonPrimitive(typed)),
    )

    /**
     * Reads what changed since [feed] (requests still under way included) through [filter], and lists every row
     * ordered by the column [by] (`seq`, `status`, `host`, `path`, `size` or `duration`). A refused filter leaves
     * [feed] as it was.
     *
     * [shown] is what an earlier read listed from this same [feed] in this same order, when the caller has it: if the
     * engine has nothing new, it is returned as it is, so an idle list sends no rows across JNI. Otherwise the rows go
     * across twice, to merge the answer in and to order the result; the cursor is worked out here ([nextSince],
     * [nextRev]).
     */
    suspend fun requests(
        feed: RequestFeed,
        filter: String,
        by: String,
        descending: Boolean,
        shown: List<RequestRow>? = null,
    ): RequestPage {
        val since = nextSince(feed.rows, feed.lastSeq)
        val answer = controlRequest(
            buildJsonObject {
                put("cmd", "capture_requests")
                put("since_seq", since)
                put("since_rev", feed.lastRev ?: 0)
                put("limit", REQUEST_PAGE)
                if (filter.isNotEmpty()) put("filter", filter)
            },
        )
        (answer as? JsonObject)?.get("error")?.let { error ->
            return RequestPage(feed, shown ?: requestRows(feed.rows, by, descending), decode(FilterError.serializer(), error))
        }
        val answerSeq = ((answer as? JsonObject)?.get("last_seq") as? JsonPrimitive)?.content?.toLongOrNull()
        val answerRev = ((answer as? JsonObject)?.get("last_rev") as? JsonPrimitive)?.content?.toLongOrNull()
        if (engineRestarted(feed, answerSeq, answerRev)) {
            // A new engine: the next read starts from nothing and takes everything it holds.
            return RequestPage(RequestFeed(), emptyList())
        }
        val lastSeq = answerSeq ?: 0L
        val lastRev = nextRev(feed.lastRev, answer, REQUEST_PAGE)
        if (shown != null && mergeChangesNothing(feed.rows, answer)) {
            return RequestPage(RequestFeed(feed.rows, maxOf(feed.lastSeq, lastSeq), lastRev), shown)
        }
        val merged = raw("view.merge_requests", args("rows" to feed.rows, "answer" to answer)) as? JsonArray
            ?: throw CoreException(texts.unreadable(), detail = "Unexpected answer to view.merge_requests")
        return RequestPage(RequestFeed(merged, maxOf(feed.lastSeq, lastSeq), lastRev), requestRows(merged, by, descending))
    }

    /** The rows read so far, ordered by the column [by]; see [requests]. */
    suspend fun requestRows(rows: JsonArray, by: String, descending: Boolean): List<RequestRow> = call(
        "view.request_rows",
        ListSerializer(RequestRow.serializer()),
        args("rows" to rows, "by" to JsonPrimitive(by), "descending" to JsonPrimitive(descending)),
    )

    /** One exchange with its headers and bodies; null once capture no longer holds it. */
    suspend fun captureExchange(key: RequestKey): CaptureExchange? {
        val answer = controlRequest(
            buildJsonObject {
                put("cmd", "capture_exchange")
                put("id", key.connection)
                put("index", key.index)
                put("since_message", 0)
            },
        )
        val exchange = (answer as? JsonObject)?.get("exchange")
        return if (exchange == null || exchange is JsonNull) null else decode(CaptureExchange.serializer(), exchange)
    }

    /** [body] with its [contentEncoding] undone (gzip, deflate, br, zstd); refused when it cannot be. */
    suspend fun decodeBody(body: CaptureBody, contentEncoding: String): DecodedBody = call(
        "decode_body",
        DecodedBody.serializer(),
        buildJsonObject {
            putJsonObject("request") {
                put("dataBase64", body.dataBase64)
                put("contentEncoding", contentEncoding)
                put("complete", !body.truncated)
            }
        },
    )

    /**
     * Saves one exchange ([exchange] of connection [connection]) to Downloads as [format] (`curl`, `code`, `postman`,
     * `insomnia` or `har`; `code` in [language]), with cookies, tokens and passwords masked unless [redact] is false.
     */
    suspend fun exportExchange(
        connection: Long,
        exchange: Int,
        format: String,
        language: String? = null,
        redact: Boolean = true,
    ): SavedFile = call(
        "export_capture",
        SavedFile.serializer(),
        buildJsonObject {
            put("format", format)
            put("id", connection)
            putJsonObject("options") {
                put("exchange", exchange)
                if (language != null) put("language", language)
                put("redact", redact)
            }
        },
    )

    /** Sends [request] through the engine without following redirects; it shows up in capture too. */
    suspend fun replay(request: ReplayRequest): ReplayResponse = call(
        "replay_request",
        ReplayResponse.serializer(),
        args("request" to Envelope.json.encodeToJsonElement(ReplayRequest.serializer(), request)),
    )

    /**
     * The steps still to do before HTTPS requests show: `connect`, `decryption`, `domains`, `capture` (and
     * `certificate` where the app can tell the root certificate is not trusted, which Android cannot).
     */
    suspend fun decryptSetup(connected: Boolean, decrypting: Boolean, domains: Int, capturing: Boolean): List<String> = call(
        "view.decrypt_setup",
        DecryptSetup.serializer(),
        buildJsonObject {
            put("connected", connected)
            put("decrypting", decrypting)
            put("domains", domains)
            put("capturing", capturing)
        },
    ).steps

    /** What capture holds of connection [id]; null once newer connections pushed it out or capture was cleared. */
    suspend fun captureConnection(id: Long): CaptureConnection? = decode(
        CaptureConnectionAnswer.serializer(),
        controlRequest(
            buildJsonObject {
                put("cmd", "capture_connection")
                put("id", id)
            },
        ),
    ).capture

    /** Saves what capture holds of connection [id] (`pcapng` or `har`) to Downloads and says where. */
    suspend fun exportConnection(id: Long, format: String, redact: Boolean): SavedFile = call(
        "export_capture",
        SavedFile.serializer(),
        buildJsonObject {
            put("format", format)
            put("id", id)
            putJsonObject("options") { put("redact", redact) }
        },
    )

    /** [host] (a domain, maybe with a port) against the decrypt list's patterns [hosts] and its [excluded] ones. */
    suspend fun decryptHost(host: String, hosts: List<String>, excluded: List<String> = emptyList()): DecryptHost = call(
        "view.decrypt_hosts",
        DecryptHost.serializer(),
        if (excluded.isEmpty()) {
            args("host" to JsonPrimitive(host), "hosts" to JsonArray(hosts.map(::JsonPrimitive)))
        } else {
            args(
                "host" to JsonPrimitive(host),
                "hosts" to JsonArray(hosts.map(::JsonPrimitive)),
                "excluded" to JsonArray(excluded.map(::JsonPrimitive)),
            )
        },
    )

    /** Saves everything capture holds (`har` or `pcapng`) to Downloads and says where. */
    suspend fun exportCapture(format: String): SavedFile =
        call("export_capture", SavedFile.serializer(), args("format" to JsonPrimitive(format)))

    // Saved capture sessions: the engine seals them, the client keeps the key (the Android Keystore here).

    /** The saved sessions, newest first. */
    suspend fun captureSessions(): CaptureSessions =
        call("capture_session", CaptureSessions.serializer(), sessionAction("list"))

    /** Seals what capture holds into a new session; a blank [name] saves it under the time. */
    suspend fun saveCaptureSession(name: String): CaptureSession = call(
        "capture_session",
        SavedSession.serializer(),
        sessionAction("save") { if (name.isNotBlank()) put("name", name.trim()) },
    ).session

    /** Replaces what capture holds with a saved session; live capture goes off. */
    suspend fun loadCaptureSession(session: String): CaptureStatus =
        call("capture_session", CaptureStatus.serializer(), sessionAction("load") { put("session", session) })

    suspend fun deleteCaptureSession(session: String): Boolean =
        call("capture_session", DeletedSession.serializer(), sessionAction("delete") { put("session", session) }).deleted

    private fun sessionAction(action: String, more: JsonObjectBuilder.() -> Unit = {}) = buildJsonObject {
        putJsonObject("action") {
            put("action", action)
            more()
        }
    }

    // `anixops://` links: parsed and matched by the shared view, so every platform reads them the same way.

    suspend fun parseAppLink(link: String): AppLink =
        call("view.parse_app_link", AppLink.serializer(), args("link" to JsonPrimitive(link)))

    /** The node a `select` link names among [nodes], or null. */
    suspend fun findLinkNode(nodes: List<NodeSummary>, name: String, group: String?): NodeSummary? = call(
        "view.find_link_node",
        NodeSummary.serializer().nullable,
        buildJsonObject {
            put("nodes", Envelope.json.encodeToJsonElement(ListSerializer(NodeSummary.serializer()), nodes))
            put("name", name)
            if (group != null) put("group", group)
        },
    )

    /** The group and member a `select_member` link names among [groups], or null. */
    suspend fun findLinkMember(groups: List<NativeGroup>, policy: String, member: String): LinkMember? = call(
        "view.find_link_member",
        LinkMember.serializer().nullable,
        buildJsonObject {
            put("groups", Envelope.json.encodeToJsonElement(ListSerializer(NativeGroup.serializer()), groups))
            put("policy", policy)
            put("member", member)
        },
    )

    // Plumbing.

    suspend fun <T> call(
        command: String,
        answer: KSerializer<T>,
        args: JsonObject = Envelope.empty,
    ): T {
        val element = raw(command, args)
        // Off the caller's thread too: a long answer (thousands of nodes) is not decoded on the main thread.
        return withContext(dispatcher) { worded(decode(answer, element, command)) }
    }

    /** The answer to [command] as JSON, for passing on to a `view.*` command unchanged. */
    suspend fun raw(command: String, args: JsonObject = Envelope.empty): JsonElement = withContext(dispatcher) {
        try {
            Envelope.read(transport.invoke(command, args.toString()))
        } catch (error: CoreException) {
            // The refusal in the app's words; the client's English stays in `detail`.
            throw texts.refusal(error)
        }
    }

    /** A result in the app's words, by its code. */
    @Suppress("UNCHECKED_CAST")
    private fun <T> worded(value: T): T = when (value) {
        is OperationResult -> texts.word(value) as T
        is SavedFileResult -> texts.word(value) as T
        else -> value
    }

    private suspend fun control(cmd: String): JsonElement = controlRequest(buildJsonObject { put("cmd", cmd) })

    private suspend fun controlRequest(request: JsonObject): JsonElement =
        raw("runtime_control", buildJsonObject { put("request", request) })

    private fun <T> decode(answer: KSerializer<T>, element: JsonElement, command: String = "the client"): T = try {
        Envelope.json.decodeFromJsonElement(answer, element)
    } catch (error: IllegalArgumentException) {
        // What the app cannot read is said in its language; the command and the reason stay in `detail`.
        throw CoreException(texts.unreadable(), detail = "Unexpected answer to $command: ${error.message}")
    }

    private inline fun <T> orNull(read: () -> T): T? = try {
        read()
    } catch (error: CoreException) {
        null
    }

    private fun <T> encodeOrNull(serializer: KSerializer<T>, value: T?): JsonElement =
        value?.let { Envelope.json.encodeToJsonElement(serializer, it) } ?: JsonNull

    private fun args(vararg pairs: Pair<String, JsonElement>) = JsonObject(mapOf(*pairs))

    companion object {
        /** The delay test target every client uses. */
        const val DELAY_URL = "https://www.gstatic.com/generate_204"

        /** A checkup's download stops at this many bytes, so it spends little of a metered node. */
        const val CHECKUP_MAX_BYTES = 200_000_000L

        /** Most rows one read of the request list asks for, as on the desktop client. */
        const val REQUEST_PAGE = 2000
    }
}

/** The proxies page: the view, and which of its nodes the user may edit and remove. */
data class ProxiesPage(
    val view: ProxiesView = ProxiesView(),
    val editable: Set<String> = emptySet(),
)

/** What the overview page reads in one go. */
data class OverviewData(
    val snapshot: RuntimeSnapshot,
    val overview: Overview,
    /** Open connections, oldest first; empty while not connected. */
    val connections: List<ConnectionSnapshot>,
    /** How many nodes the client has, in every subscription. */
    val nodeCount: Int = 0,
)

