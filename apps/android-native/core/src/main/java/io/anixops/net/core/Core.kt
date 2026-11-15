package io.anixops.net.core

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

/**
 * Typed commands over the client. Every call runs on [dispatcher] (commands may block for long: downloads,
 * delay tests, the VPN consent) and throws [CoreException] when the client refuses. Names and arguments are the
 * shared interface's (`apps/windows-gui/ui/src/bridge.ts`); page logic is the client's `view.*` commands
 * (crates/client-view), so every client shows the same thing.
 */
class Core(
    private val transport: CoreTransport,
    private val dispatcher: CoroutineDispatcher = Dispatchers.IO,
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
        )
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
        val editable = decode(ListSerializer(NodeSummary.serializer()), nodes, "list_nodes")
            .filter { it.editable }
            .mapTo(mutableSetOf()) { it.id }
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
        return call("subscriptions", SubscriptionsView.serializer()).subscriptions
            .map { it.copy(intervalHours = intervals[it.id] ?: 0) }
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

    suspend fun refreshRoutingRules(): OperationResult = call("refresh_routing_rules", OperationResult.serializer())

    suspend fun myRules(): MyRules = call("my_rules", MyRules.serializer())

    suspend fun saveMyRules(text: String): OperationResult =
        call("save_my_rules", OperationResult.serializer(), args("text" to JsonPrimitive(text)))

    suspend fun nativeGroups(): List<NativeGroup> =
        call("list_native_groups", ListSerializer(NativeGroup.serializer()))

    suspend fun selectGroupOutbound(groupTag: String, outboundTag: String): OperationResult = call(
        "select_native_group_outbound",
        OperationResult.serializer(),
        args("groupTag" to JsonPrimitive(groupTag), "outboundTag" to JsonPrimitive(outboundTag)),
    )

    /** The running engine's rule sets; empty while it does not run. */
    suspend fun ruleSets(): List<RuleSetStatus> =
        orNull { decode(RuleSetsAnswer.serializer(), control("rule_sets"), "rule_sets").ruleSets }.orEmpty()

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
    suspend fun enableHttpsMitm(): OperationResult = call("enable_https_mitm", OperationResult.serializer())

    suspend fun disableHttpsMitm(): OperationResult = call("disable_https_mitm", OperationResult.serializer())

    /** The domains decryption was turned on for one by one. */
    suspend fun decryptHosts(): List<String> = call("list_decrypt_hosts", ListSerializer(String.serializer()))

    suspend fun addDecryptHost(host: String): OperationResult =
        call("add_decrypt_host", OperationResult.serializer(), args("host" to JsonPrimitive(host)))

    suspend fun removeDecryptHost(host: String): OperationResult =
        call("remove_decrypt_host", OperationResult.serializer(), args("host" to JsonPrimitive(host)))

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
    suspend fun createDiagnostics(): OperationResult = call("create_diagnostics", OperationResult.serializer())

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
     */
    suspend fun requests(feed: RequestFeed, filter: String, by: String, descending: Boolean): RequestPage {
        val since = call(
            "view.next_since",
            Long.serializer(),
            args("rows" to feed.rows, "lastSeq" to JsonPrimitive(feed.lastSeq)),
        )
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
            return RequestPage(feed, requestRows(feed.rows, by, descending), decode(FilterError.serializer(), error))
        }
        val merged = raw("view.merge_requests", args("rows" to feed.rows, "answer" to answer)) as? JsonArray
            ?: throw CoreException("Unexpected answer to view.merge_requests")
        val lastSeq = ((answer as? JsonObject)?.get("last_seq") as? JsonPrimitive)?.content?.toLongOrNull() ?: 0
        val nextRev = raw(
            "view.next_rev",
            args(
                "current" to (feed.lastRev?.let(::JsonPrimitive) ?: JsonNull),
                "answer" to answer,
                "page" to JsonPrimitive(REQUEST_PAGE),
            ),
        )
        val lastRev = (nextRev as? JsonPrimitive)?.content?.toLongOrNull()
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

    // Plumbing.

    suspend fun <T> call(
        command: String,
        answer: KSerializer<T>,
        args: JsonObject = Envelope.empty,
    ): T = decode(answer, raw(command, args), command)

    /** The answer to [command] as JSON, for passing on to a `view.*` command unchanged. */
    suspend fun raw(command: String, args: JsonObject = Envelope.empty): JsonElement = withContext(dispatcher) {
        Envelope.read(transport.invoke(command, args.toString()))
    }

    private suspend fun control(cmd: String): JsonElement = controlRequest(buildJsonObject { put("cmd", cmd) })

    private suspend fun controlRequest(request: JsonObject): JsonElement =
        raw("runtime_control", buildJsonObject { put("request", request) })

    private fun <T> decode(answer: KSerializer<T>, element: JsonElement, command: String = "the client"): T = try {
        Envelope.json.decodeFromJsonElement(answer, element)
    } catch (error: IllegalArgumentException) {
        throw CoreException("Unexpected answer to $command: ${error.message}")
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

