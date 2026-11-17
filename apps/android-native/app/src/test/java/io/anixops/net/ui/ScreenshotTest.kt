package io.anixops.net.ui

import com.github.takahirom.roborazzi.captureRoboImage
import io.anixops.net.core.CaptureBody
import io.anixops.net.core.CaptureChunk
import io.anixops.net.core.CaptureConnection
import io.anixops.net.core.CaptureDirection
import io.anixops.net.core.CaptureExchange
import io.anixops.net.core.CaptureMessage
import io.anixops.net.core.CaptureSession
import io.anixops.net.core.CaptureStatus
import io.anixops.net.core.ConnectionSnapshot
import io.anixops.net.core.DebugMapping
import io.anixops.net.core.DecryptHost
import io.anixops.net.core.Delay
import io.anixops.net.core.DnsText
import io.anixops.net.core.FilterError
import io.anixops.net.core.HeaderField
import io.anixops.net.core.HealthStatus
import io.anixops.net.core.LogEntry
import io.anixops.net.core.LogsView
import io.anixops.net.core.MappingText
import io.anixops.net.core.MitmPlugin
import io.anixops.net.core.MyRules
import io.anixops.net.core.NativeGroup
import io.anixops.net.core.NetworkProbe
import io.anixops.net.core.NodeRow
import io.anixops.net.core.NodeSection
import io.anixops.net.core.NodeSummary
import io.anixops.net.core.NotificationSettings
import io.anixops.net.core.OnDemandSettings
import io.anixops.net.core.Overview
import io.anixops.net.core.ProxiesPage
import io.anixops.net.core.ProxiesView
import io.anixops.net.core.RedirectDraft
import io.anixops.net.core.ReplayResponse
import io.anixops.net.core.RequestRow
import io.anixops.net.core.RuleSetStatus
import io.anixops.net.core.RuntimeSnapshot
import io.anixops.net.core.StatusFact
import io.anixops.net.core.SubscriptionRow
import io.anixops.net.core.Traffic
import io.anixops.net.core.TrafficBar
import io.anixops.net.core.TrafficPage
import io.anixops.net.core.TrafficRedirect
import io.anixops.net.core.TrafficShare
import io.anixops.net.core.TrafficStatsView
import io.anixops.net.core.WifiRule
import io.anixops.net.ui.theme.AnixTheme
import java.io.File
import java.util.TimeZone
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The screenshot baselines (src/test/screenshots): each page in light and dark, Simplified Chinese and English,
 * phone and tablet. CI records them again and fails when they differ from the committed ones; a deliberate UI
 * change commits the new images with it (apps/android-native/README.md).
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35])
class ScreenshotTest {
    @Test
    fun homeConnectedPhoneLightZh() = capture("home-connected-phone-light-zh", PHONE, "zh-rCN", dark = false)

    @Test
    fun homeConnectedPhoneDarkZh() = capture("home-connected-phone-dark-zh", PHONE, "zh-rCN", dark = true)

    @Test
    fun homeConnectedPhoneLightEn() = capture("home-connected-phone-light-en", PHONE, "en", dark = false)

    @Test
    fun homeFailedPhoneLightZh() = capture(
        "home-failed-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        state = UiState(
            snapshot = snapshot.copy(connection = "Connection failed"),
            overview = overview.copy(state = "failed", traffic = null, error = "节点 tokyo-01 握手超时（10 秒）"),
        ),
    )

    @Test
    fun homeEmptyPhoneDarkEn() = capture(
        "home-empty-phone-dark-en",
        PHONE,
        "en",
        dark = true,
        state = UiState(
            snapshot = snapshot.copy(connection = "Not connected", selectedNode = null, subscription = null),
            overview = Overview(state = "disconnected", egress = "node", canConnect = false),
        ),
    )

    @Test
    fun homeConnectedTabletLightZh() = capture("home-connected-tablet-light-zh", TABLET, "zh-rCN", dark = false)

    @Test
    fun homeConnectedTabletDarkEn() = capture("home-connected-tablet-dark-en", TABLET, "en", dark = true)

    @Test
    fun settingsPhoneLightZh() =
        capture("settings-phone-light-zh", PHONE, "zh-rCN", dark = false, destination = Destination.Settings)

    @Test
    fun settingsPhoneDarkEn() =
        capture("settings-phone-dark-en", PHONE, "en", dark = true, destination = Destination.Settings)

    @Test
    fun settingsTabletLightZh() =
        capture("settings-tablet-light-zh", TABLET, "zh-rCN", dark = false, destination = Destination.Settings)

    /** The whole page on one tall screen, with the Wi-Fi rules on so their rows show. */
    @Test
    fun settingsWholePagePhoneLightZh() =
        capture("settings-whole-page-phone-light-zh", TALL_PHONE, "zh-rCN", dark = false, destination = Destination.Settings)

    @Test
    fun settingsWholePagePhoneDarkEn() =
        capture("settings-whole-page-phone-dark-en", TALL_PHONE, "en", dark = true, destination = Destination.Settings)

    @Test
    fun logsPhoneLightZh() = capture(
        "logs-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        destination = Destination.Settings,
        settingsPage = SettingsPage.Logs,
    )

    @Test
    fun logsPhoneDarkEn() = capture(
        "logs-phone-dark-en",
        PHONE,
        "en",
        dark = true,
        destination = Destination.Settings,
        settingsPage = SettingsPage.Logs,
    )

    @Test
    fun trafficPhoneLightZh() = capture(
        "traffic-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        destination = Destination.Settings,
        settingsPage = SettingsPage.Traffic,
    )

    @Test
    fun trafficHostsPhoneDarkEn() = capture(
        "traffic-hosts-phone-dark-en",
        PHONE,
        "en",
        dark = true,
        destination = Destination.Settings,
        settingsPage = SettingsPage.Traffic,
        traffic = trafficState.copy(view = StatsView.Hosts),
    )

    @Test
    fun trafficTabletLightZh() = capture(
        "traffic-tablet-light-zh",
        TABLET,
        "zh-rCN",
        dark = false,
        destination = Destination.Settings,
        settingsPage = SettingsPage.Traffic,
    )

    @Test
    fun termsConsentPhoneLightZh() = captureTerms("terms-consent-phone-light-zh", PHONE, "zh-rCN", dark = false, consent = true)

    @Test
    fun termsConsentPhoneDarkEn() = captureTerms("terms-consent-phone-dark-en", PHONE, "en", dark = true, consent = true)

    @Test
    fun modeChoicePhoneLightZh() = captureModeChoice("mode-choice-phone-light-zh", PHONE, "zh-rCN", dark = false)

    @Test
    fun modeChoicePhoneDarkEn() = captureModeChoice("mode-choice-phone-dark-en", PHONE, "en", dark = true)

    @Test
    fun modeChoiceTabletLightZh() = captureModeChoice("mode-choice-tablet-light-zh", TABLET, "zh-rCN", dark = false)

    @Test
    fun termsViewerTabletLightZh() = captureTerms("terms-viewer-tablet-light-zh", TABLET, "zh-rCN", dark = false, consent = false)

    @Test
    fun proxiesPhoneLightZh() =
        capture("proxies-phone-light-zh", PHONE, "zh-rCN", dark = false, destination = Destination.Proxies)

    @Test
    fun proxiesPhoneDarkEn() =
        capture("proxies-phone-dark-en", PHONE, "en", dark = true, destination = Destination.Proxies)

    @Test
    fun proxiesTabletLightZh() =
        capture("proxies-tablet-light-zh", TABLET, "zh-rCN", dark = false, destination = Destination.Proxies)

    @Test
    fun proxiesEmptyPhoneLightZh() = capture(
        "proxies-empty-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        destination = Destination.Proxies,
        proxies = ProxiesState(loaded = true),
    )

    @Test
    fun rulesPhoneLightZh() = capture("rules-phone-light-zh", PHONE, "zh-rCN", dark = false, destination = Destination.Rules)

    @Test
    fun rulesPhoneDarkEn() = capture("rules-phone-dark-en", PHONE, "en", dark = true, destination = Destination.Rules)

    @Test
    fun rulesTabletLightZh() =
        capture("rules-tablet-light-zh", TABLET, "zh-rCN", dark = false, destination = Destination.Rules)

    /** The whole page on one tall screen, with 自定义 DNS chosen so its editor shows too. */
    @Test
    fun rulesWholePagePhoneLightZh() = capture(
        "rules-whole-page-phone-light-zh",
        TALL_PHONE,
        "zh-rCN",
        dark = false,
        state = connectedState.copy(snapshot = snapshot.copy(dnsPreset = "custom", blockQuic = true)),
        destination = Destination.Rules,
    )

    @Test
    fun rulesWholePagePhoneDarkEn() = capture(
        "rules-whole-page-phone-dark-en",
        TALL_PHONE,
        "en",
        dark = true,
        state = connectedState.copy(snapshot = snapshot.copy(dnsPreset = "custom", blockQuic = true)),
        destination = Destination.Rules,
    )

    @Test
    fun requestsPhoneLightZh() = captureRequests("requests-phone-light-zh", PHONE, "zh-rCN", dark = false)

    @Test
    fun requestsPhoneDarkEn() = captureRequests("requests-phone-dark-en", PHONE, "en", dark = true)

    @Test
    fun requestsTabletLightZh() = captureRequests("requests-tablet-light-zh", TABLET, "zh-rCN", dark = false)

    @Test
    fun requestsFilterErrorPhoneLightEn() = captureRequests(
        "requests-filter-error-phone-light-en",
        PHONE,
        "en",
        dark = false,
        requests = requestsState.copy(typed = "status:>=4x", quick = setOf("json"), rows = emptyList(), error = FilterError("invalid_filter", at = 10)),
    )

    @Test
    fun requestsNotConnectedPhoneLightZh() = captureRequests(
        "requests-not-connected-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        state = UiState(
            snapshot = snapshot.copy(connection = "Not connected", directEgress = true),
            overview = Overview(state = "disconnected", egress = "direct", canConnect = true),
        ),
        requests = RequestsState(loaded = true, setup = listOf("connect", "decryption", "domains", "capture")),
    )

    @Test
    fun requestsCaptureOffPhoneDarkZh() = captureRequests(
        "requests-capture-off-phone-dark-zh",
        PHONE,
        "zh-rCN",
        dark = true,
        requests = RequestsState(capture = CaptureStatus(maxTotalBytes = 268_435_456), loaded = true, setup = listOf("domains", "capture")),
    )

    @Test
    fun requestsSetupPhoneLightEn() = captureRequests(
        "requests-setup-phone-light-en",
        PHONE,
        "en",
        dark = false,
        requests = requestsState.copy(setup = listOf("certificate", "domains")),
    )

    @Test
    fun requestDetailPhoneLightZh() = captureRequests(
        "request-detail-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        requests = requestsState.copy(opened = openedRequest),
    )

    @Test
    fun requestDetailWholePagePhoneDarkEn() = captureRequests(
        "request-detail-whole-page-phone-dark-en",
        TALL_PHONE,
        "en",
        dark = true,
        requests = requestsState.copy(opened = openedRequest),
    )

    @Test
    fun replayPhoneLightZh() = captureRequests(
        "replay-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        requests = requestsState.copy(opened = openedRequest, replay = replayDraft),
    )

    @Test
    fun replayAnswerWholePagePhoneDarkEn() = captureRequests(
        "replay-answer-whole-page-phone-dark-en",
        TALL_PHONE,
        "en",
        dark = true,
        requests = requestsState.copy(opened = openedRequest, replay = replayAnswered),
    )

    @Test
    fun sessionsPhoneLightZh() = captureRequests(
        "sessions-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        requests = requestsState.copy(sessions = sessionsPage),
    )

    @Test
    fun sessionsEmptyPhoneDarkEn() = captureRequests(
        "sessions-empty-phone-dark-en",
        PHONE,
        "en",
        dark = true,
        requests = requestsState.copy(sessions = SessionsPage(loaded = true, protection = "keystore")),
    )

    @Test
    fun requestsConnectionsPhoneLightZh() = captureRequests(
        "requests-connections-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        state = connectedState,
        requests = requestsState.copy(showConnections = true),
    )

    @Test
    fun connectionWholePagePhoneDarkEn() = captureRequests(
        "connection-whole-page-phone-dark-en",
        TALL_PHONE,
        "en",
        dark = true,
        requests = requestsState.copy(connection = connectionView),
    )

    @Test
    fun connectionTlsPhoneLightZh() = captureRequests(
        "connection-tls-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        requests = requestsState.copy(connection = tlsConnectionView),
    )

    @Test
    fun extensionsPhoneLightZh() =
        capture("extensions-phone-light-zh", PHONE, "zh-rCN", dark = false, state = decryptingState, destination = Destination.Extensions)

    @Test
    fun extensionsPhoneDarkEn() =
        capture("extensions-phone-dark-en", PHONE, "en", dark = true, state = decryptingState, destination = Destination.Extensions)

    @Test
    fun extensionsTabletLightZh() =
        capture("extensions-tablet-light-zh", TABLET, "zh-rCN", dark = false, state = decryptingState, destination = Destination.Extensions)

    @Test
    fun extensionsWholePagePhoneDarkZh() = capture(
        "extensions-whole-page-phone-dark-zh",
        TALL_PHONE,
        "zh-rCN",
        dark = true,
        state = decryptingState,
        destination = Destination.Extensions,
    )

    @Test
    fun mappingEditorPhoneLightZh() = capture(
        "mapping-editor-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        mode = AppMode.Debug,
        destination = Destination.Rewrite,
        extensions = extensionsState.copy(editingMapping = extensionsState.mappings.first()),
    )

    @Test
    fun mappingEditorRemotePhoneDarkEn() = capture(
        "mapping-editor-remote-phone-dark-en",
        PHONE,
        "en",
        dark = true,
        mode = AppMode.Debug,
        destination = Destination.Rewrite,
        extensions = extensionsState.copy(editingMapping = extensionsState.mappings.last()),
    )

    @Test
    fun rewriteEmptyPhoneLightEn() = capture(
        "rewrite-empty-phone-light-en",
        PHONE,
        "en",
        dark = false,
        mode = AppMode.Debug,
        destination = Destination.Rewrite,
        extensions = ExtensionsState(loaded = true),
    )

    @Test
    fun egressPhoneLightZh() = capture(
        "egress-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        state = connectedState.copy(nodeCount = 4),
        mode = AppMode.Debug,
        destination = Destination.Egress,
        debugExit = DebugExit.Node,
    )

    @Test
    fun egressDirectPhoneDarkEn() = capture(
        "egress-direct-phone-dark-en",
        PHONE,
        "en",
        dark = true,
        state = connectedState.copy(nodeCount = 4),
        mode = AppMode.Debug,
        destination = Destination.Egress,
    )

    @Test
    fun egressNoNodesPhoneLightZh() = capture(
        "egress-no-nodes-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        mode = AppMode.Debug,
        destination = Destination.Egress,
        proxies = ProxiesState(loaded = true),
        debugExit = DebugExit.Node,
    )

    @Test
    fun egressTabletLightZh() = capture(
        "egress-tablet-light-zh",
        TABLET,
        "zh-rCN",
        dark = false,
        state = connectedState.copy(nodeCount = 4),
        mode = AppMode.Debug,
        destination = Destination.Egress,
        debugExit = DebugExit.Node,
    )

    @Test
    fun egressRulesPhoneDarkZh() = capture(
        "egress-rules-phone-dark-zh",
        PHONE,
        "zh-rCN",
        dark = true,
        mode = AppMode.Debug,
        destination = Destination.Egress,
        egressRules = true,
    )

    private fun capture(
        name: String,
        device: String,
        locale: String,
        dark: Boolean,
        state: UiState = connectedState,
        mode: AppMode = AppMode.Proxy,
        destination: Destination? = null,
        proxies: ProxiesState = proxiesState,
        rules: RulesState = rulesState,
        settingsPage: SettingsPage? = null,
        traffic: TrafficState = trafficState,
        extensions: ExtensionsState = extensionsState,
        debugExit: DebugExit = DebugExit.Direct,
        egressRules: Boolean = false,
        requests: RequestsState = requestsState,
    ) {
        TimeZone.setDefault(TimeZone.getTimeZone("UTC"))
        RuntimeEnvironment.setQualifiers("$locale-$device-${if (dark) "night" else "notnight"}-xhdpi")
        captureRoboImage("src/test/screenshots/$name.png") {
            AnixTheme {
                AnixScaffold(
                    state = state,
                    mode = mode,
                    version = "0.3.0",
                    onModeChange = {},
                    proxiesState = proxies,
                    rulesState = rules,
                    settingsState = settingsState,
                    settings = SettingsActions(openLanguageSettings = {}),
                    termsVersion = "2.1.0",
                    logsState = logsState,
                    trafficState = traffic,
                    extensionsState = extensions,
                    debugExit = debugExit,
                    requestsState = requests,
                    initialSettingsPage = settingsPage,
                    initialEgressRules = egressRules,
                    initial = destination,
                )
            }
        }
    }

    private fun captureRequests(
        name: String,
        device: String,
        locale: String,
        dark: Boolean,
        state: UiState = connectedState.copy(overview = overview.copy(egress = "direct", node = null)),
        requests: RequestsState = requestsState,
    ) = capture(name, device, locale, dark, state = state, mode = AppMode.Debug, destination = Destination.Requests, requests = requests)

    private fun captureTerms(name: String, device: String, locale: String, dark: Boolean, consent: Boolean) {
        RuntimeEnvironment.setQualifiers("$locale-$device-${if (dark) "night" else "notnight"}-xhdpi")
        val markdown = File(TermsTest.TERMS_PATH).readText()
        captureRoboImage("src/test/screenshots/$name.png") {
            AnixTheme {
                TermsScreen(markdown, onAccept = if (consent) ({}) else null)
            }
        }
    }

    private fun captureModeChoice(name: String, device: String, locale: String, dark: Boolean) {
        RuntimeEnvironment.setQualifiers("$locale-$device-${if (dark) "night" else "notnight"}-xhdpi")
        captureRoboImage("src/test/screenshots/$name.png") {
            AnixTheme {
                ModeChoiceScreen(onChoose = {})
            }
        }
    }

    private companion object {
        const val PHONE = "w411dp-h891dp"
        const val TABLET = "w1280dp-h800dp"
        const val TALL_PHONE = "w411dp-h3000dp"

        val snapshot = RuntimeSnapshot(
            platform = "android",
            connection = "Connected",
            core = StatusFact(label = "Running", tone = "success"),
            selectedNode = "tokyo-01",
            subscription = "https://panel.example.com/api/v1/client/subscribe?token=demo",
            routingMode = "rule",
            dnsPreset = "system",
        )

        val tokyo = NodeSummary(id = "tokyo-01", label = "东京 01", protocol = "VLESS", selected = true, group = "Panel")

        val overview = Overview(
            state = "connected",
            egress = "node",
            node = tokyo,
            canConnect = true,
            traffic = Traffic(
                uploadRate = 21_504,
                downloadRate = 1_572_864,
                uploadTotal = 18_874_368,
                downloadTotal = 734_003_200,
                connections = 3,
            ),
        )

        val connectedState = UiState(
            snapshot = snapshot,
            overview = overview,
            connections = listOf(
                ConnectionSnapshot(1, "tcp", "www.example.com", "www.example.com:443", "DOMAIN-SUFFIX", listOf("东京 01"), upload = 4_096, download = 1_048_576),
                ConnectionSnapshot(2, "udp", null, "203.0.113.8:443", "MATCH", listOf("东京 01"), upload = 2_048, download = 65_536),
                ConnectionSnapshot(3, "tcp", "api.example.org", "api.example.org:443", "GEOIP", listOf("DIRECT"), upload = 512, download = 2_048),
            ),
            probe = NetworkProbe(exitIp = "198.51.100.24", exitLocation = "JP"),
        )

        val rulesState = RulesState(
            myRules = MyRules(
                text = "# 公司内网直连\nDOMAIN-SUFFIX,corp.example.com,DIRECT\nDOMAIN-KEYWORD,ads,REJECT\nIP-CIDR,10.0.0.0/8,DIRECT,no-resolve",
                count = 3,
            ),
            groups = listOf(
                NativeGroup(
                    tag = "Proxy",
                    groupType = "selector",
                    selected = "tokyo-01",
                    outbounds = listOf("tokyo-01", "osaka-02", "Auto"),
                    outboundLabels = listOf("东京 01", "大阪 02", "Auto"),
                ),
                NativeGroup(
                    tag = "Auto",
                    groupType = "urltest",
                    selected = "osaka-02",
                    outbounds = listOf("tokyo-01", "osaka-02"),
                    outboundLabels = listOf("东京 01", "大阪 02"),
                ),
            ),
            ruleSets = listOf(
                RuleSetStatus("china", "remote", entries = 8_412, updatedAtMs = 1_763_198_400_000),
                RuleSetStatus("ads", "remote", entries = 0, lastError = "HTTP 404"),
                RuleSetStatus("lan", "inline", entries = 12),
            ),
            dns = DnsText(
                servers = "cloudflare = https://1.1.1.1/dns-query, server-name=cloudflare-dns.com",
                rules = "DOMAIN-SUFFIX,cn,alidns",
                fakeIp = true,
            ),
        )

        /** 2026-11-15 09:20 UTC, and the lines a few seconds apart. */
        private const val LOG_TIME = 1_794_734_400_000

        val decryptingState = connectedState.copy(snapshot = snapshot.copy(httpsDecryptionEnabled = true))

        val extensionsState = ExtensionsState(
            loaded = true,
            consented = true,
            hosts = listOf("api.example.com", "*.example.org"),
            plugins = listOf(
                MitmPlugin(
                    id = "p1",
                    name = "去广告",
                    url = "https://plugins.example.com/ads.plugin",
                    enabled = true,
                    hostnames = listOf("api.example.com", "ads.example.net", "m.example.cn", "img.example.cn"),
                    scriptCount = 2,
                ),
                MitmPlugin(
                    id = "p2",
                    name = "Check-in",
                    url = "https://plugins.example.com/checkin.sgmodule",
                    enabled = false,
                    level = "partial",
                    hostnames = listOf("checkin.example.com"),
                    scriptCount = 1,
                ),
                MitmPlugin(id = "p3", name = "Header tweaks", level = "unsupported"),
            ),
            mappings = listOf(
                DebugMapping(
                    id = "m1",
                    name = "Mock profile",
                    pattern = "https://api.example.com/v1/profile",
                    headers = listOf(HeaderField("Content-Type", "application/json; charset=utf-8")),
                    body = MappingText.base64("{\n  \"name\": \"Ada\",\n  \"plan\": \"pro\"\n}"),
                ),
                DebugMapping(
                    id = "m2",
                    enabled = false,
                    kind = "remote",
                    match = "prefix",
                    pattern = "https://api.example.com/v2/",
                    target = "https://staging.example.com/v2/",
                ),
            ),
            redirects = listOf(
                TrafficRedirect(
                    id = "r1",
                    match = "AND,((IP-CIDR,203.0.113.10/32),(DEST-PORT,7000-7100),(PROTOCOL,UDP))",
                    target = "192.168.1.20:7001",
                    draft = RedirectDraft("203.0.113.10", "7000-7100", "udp", "192.168.1.20:7001"),
                ),
                TrafficRedirect(id = "r2", match = "DOMAIN-SUFFIX,game.example.com", target = ":9000", enabled = false),
            ),
        )

        val logsState = LogsState(
            logs = LogsView(
                modules = listOf("app", "mitm"),
                shown = listOf(
                    LogEntry(LOG_TIME + 9_000, "app", "error", "订阅更新失败：HTTP 503（panel.example.com/api/v1/client/subscribe）"),
                    LogEntry(LOG_TIME + 6_000, "mitm", "warn", "api.example.com 的证书不受信任，已直连"),
                    LogEntry(LOG_TIME + 3_000, "app", "info", "Connected through 东京 01"),
                    LogEntry(LOG_TIME, "app", "info", "Engine started (rule mode)"),
                ),
                text = "",
            ),
        )

        val trafficState = TrafficState(
            page = TrafficPage(
                TrafficStatsView(
                    start = 1_794_700_800,
                    upload = 52_428_800,
                    download = 1_181_116_416,
                    bars = List(24) { hour ->
                        TrafficBar(1_794_700_800L + hour * 3_600, listOf(0L, 0L, 0L, 0L, 0L, 0L, 0L, 12, 48, 96, 160, 40).getOrElse(hour) { 0L } * 4_194_304)
                    },
                    hosts = listOf(
                        TrafficShare("video.example.com", 734_003_200),
                        TrafficShare("api.example.org", 209_715_200),
                        TrafficShare("cdn.example.net", 104_857_600),
                        TrafficShare("*", 31_457_280),
                    ),
                    policies = listOf(TrafficShare("Proxy", 943_718_400), TrafficShare("DIRECT", 136_314_880)),
                ),
                recorded = true,
            ),
        )

        val settingsState = SettingsState(
            notifications = NotificationSettings(scripts = true, connection = true, subscriptions = false, language = "zh"),
            onDemand = OnDemandSettings(
                enabled = true,
                wifi = listOf(WifiRule("Home-5G", "disconnect"), WifiRule("Office", "connect")),
                otherWifi = "connect",
                cellular = "connect",
                other = "ignore",
            ),
            health = HealthStatus(crashReports = true, integrity = "unverified", coldStartMs = 412),
        )

        val proxiesState = ProxiesState(
            loaded = true,
            subscriptions = listOf(
                SubscriptionRow(
                    id = "s1",
                    selected = true,
                    name = "Panel",
                    updated = "2026-11-15 09:30",
                    nodes = 3,
                    used = 46_170_898_432,
                    total = 214_748_364_800,
                    expire = 1_798_675_200,
                    intervalHours = 24,
                    refreshable = true,
                ),
                SubscriptionRow(
                    id = "s2",
                    name = "Backup",
                    nodes = 2,
                    refreshable = true,
                    updated = "2026-11-01 08:00",
                    error = "HTTP 403",
                    expire = 1_762_000_000,
                    expired = true,
                ),
                SubscriptionRow(id = "pasted", name = "粘贴的节点", pasted = true, nodes = 1),
            ),
            page = ProxiesPage(
                view = ProxiesView(
                    total = 4,
                    shown = 4,
                    sections = listOf(
                        NodeSection(
                            "Panel",
                            source = "s1",
                            nodes = listOf(
                                NodeRow("tokyo-01", "东京 01", "VLESS", "Panel", selected = true, delay = Delay(86, "fast")),
                                NodeRow("osaka-02", "大阪 02", "Trojan", "Panel", delay = Delay(642, "slow")),
                                NodeRow("hk-03", "香港 03", "Shadowsocks", "Panel", delay = Delay(null, "failed")),
                            ),
                        ),
                        NodeSection(
                            "粘贴的节点",
                            source = "pasted",
                            pasted = true,
                            nodes = listOf(NodeRow("pasted-sg", "Singapore", "Hysteria2", "粘贴的节点")),
                        ),
                    ),
                ),
                editable = setOf("tokyo-01", "osaka-02", "hk-03"),
            ),
        )

        /** 2026-11-15 09:41 UTC, the samples' clock. */
        private const val NOW = 1_794_735_660_000L

        val requestRows = listOf(
            RequestRow(
                connectionId = 9, index = 0, method = "POST", url = "https://upload.example.com/v1/files",
                host = "upload.example.com", httpVersion = "h2", startedAtMs = NOW - 400, requestSize = 2_097_152,
                state = "in_progress", path = "/v1/files", tone = "pending",
            ),
            RequestRow(
                seq = 41, connectionId = 7, index = 2, method = "GET", url = "https://api.example.com/v1/profile?fields=name",
                host = "api.example.com", status = 200, httpVersion = "h2", startedAtMs = NOW - 2_400, endedAtMs = NOW - 2_262,
                responseSize = 612, responseContentType = "application/json", state = "done",
                path = "/v1/profile?fields=name", tone = "success",
            ),
            RequestRow(
                seq = 40, connectionId = 7, index = 1, method = "GET", url = "https://api.example.com/v1/orders/8812",
                host = "api.example.com", status = 404, httpVersion = "h2", startedAtMs = NOW - 3_100, endedAtMs = NOW - 3_020,
                responseSize = 96, state = "done", path = "/v1/orders/8812", tone = "danger",
            ),
            RequestRow(
                seq = 39, connectionId = 6, index = 0, method = "GET", url = "https://cdn.example.net/img/banner.webp",
                host = "cdn.example.net", status = 304, httpVersion = "HTTP/1.1", startedAtMs = NOW - 5_000, endedAtMs = NOW - 3_650,
                state = "done", path = "/img/banner.webp", tone = "warning",
            ),
            RequestRow(
                seq = 38, connectionId = 5, index = 0, method = "GET", url = "https://api.example.com/v1/feature-flags",
                host = "api.example.com", httpVersion = "HTTP/1.1", startedAtMs = NOW - 7_000, endedAtMs = NOW - 6_998,
                responseSize = 48, note = "answered_locally", state = "done", path = "/v1/feature-flags", tone = "neutral",
            ),
        )

        val requestsState = RequestsState(
            capture = CaptureStatus(enabled = true, maxTotalBytes = 268_435_456, usedBytes = 3_407_872, connections = 5),
            rows = requestRows,
            loaded = true,
        )

        private const val PROFILE_JSON = """{"id":42,"name":"Ada","plan":"pro","tags":["beta","ios"]}"""

        val openedRequest = OpenedRequest(
            row = requestRows[1],
            exchange = CaptureExchange(
                startedAtMs = NOW - 2_400,
                endedAtMs = NOW - 2_262,
                method = "GET",
                url = "https://api.example.com/v1/profile?fields=name",
                status = 200,
                request = CaptureMessage(
                    headers = listOf(
                        listOf(":authority", "api.example.com"),
                        listOf("accept", "application/json"),
                        listOf("authorization", "Bearer •••"),
                        listOf("user-agent", "Example/3.2 (Android 15)"),
                    ),
                ),
                response = CaptureMessage(
                    headers = listOf(
                        listOf("content-type", "application/json; charset=utf-8"),
                        listOf("content-encoding", "br"),
                        listOf("cache-control", "no-store"),
                    ),
                    body = CaptureBody(size = 412),
                ),
                state = "done",
            ),
            requestBody = BodyText("", 0),
            responseBody = BodyText(BodyText.text(PROFILE_JSON.toByteArray(), "application/json"), PROFILE_JSON.length.toLong()),
        )

        val replayDraft = ReplayDraft(
            method = "POST",
            url = "https://api.example.com/v1/profile",
            headers = "accept: application/json\ncontent-type: application/json\nuser-agent: Example/3.2 (Android 15)",
            body = "{\"name\": \"Mika\"}",
        )

        val sessionsPage = SessionsPage(
            loaded = true,
            protection = "keystore",
            name = "登录流程",
            sessions = listOf(
                CaptureSession(
                    session = "1794730000000-a1b2c3d4",
                    name = "登录流程",
                    savedAtMs = LOG_TIME - 3_600_000,
                    expiresAtMs = LOG_TIME + 6 * 86_400_000L,
                    size = 1_843_200,
                    connections = 12,
                    requests = 86,
                ),
                CaptureSession(
                    session = "1794640000000-e5f6a7b8",
                    savedAtMs = LOG_TIME - 90_000_000,
                    expiresAtMs = LOG_TIME + 5 * 86_400_000L,
                    size = 204_800,
                    connections = 3,
                    requests = 1,
                ),
                CaptureSession(session = "1794000000000-c9d0e1f2", name = "重装前", savedAtMs = LOG_TIME - 600_000_000, unreadable = true),
            ),
        )

        val connectionView = ConnectionView(
            id = 7,
            name = "api.example.com",
            loaded = true,
            capture = CaptureConnection(
                id = 7,
                network = "tcp",
                host = "api.example.com",
                destination = "203.0.113.20:443",
                startedAtMs = LOG_TIME,
                decrypted = true,
                upload = CaptureDirection(seen = 1_812, captured = 1_812),
                download = CaptureDirection(seen = 2_400_000, captured = 65_536, truncated = true),
                chunks = listOf(
                    CaptureChunk("upload", LOG_TIME + 12, MappingText.base64("GET /v1/profile HTTP/1.1\r\nHost: api.example.com\r\n\r\n")),
                    CaptureChunk("download", LOG_TIME + 150, MappingText.base64("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{}")),
                ),
                http = listOf(
                    CaptureExchange(
                        startedAtMs = LOG_TIME + 12,
                        endedAtMs = LOG_TIME + 150,
                        method = "GET",
                        url = "https://api.example.com/v1/profile?fields=name",
                        status = 200,
                        response = CaptureMessage(
                            headers = listOf(listOf("content-type", "application/json")),
                            body = CaptureBody(size = 412),
                        ),
                    ),
                    CaptureExchange(startedAtMs = LOG_TIME + 900, method = "POST", url = "https://api.example.com/v1/events"),
                ),
            ),
        )

        val tlsConnectionView = ConnectionView(
            id = 9,
            name = "img.example.net",
            loaded = true,
            decrypt = DecryptHost(decryptable = true, host = "img.example.net"),
            capture = CaptureConnection(
                id = 9,
                network = "tcp",
                host = "img.example.net",
                destination = "198.51.100.7:443",
                upload = CaptureDirection(seen = 517, captured = 517),
                download = CaptureDirection(seen = 4_096, captured = 4_096),
                chunks = listOf(
                    CaptureChunk("upload", LOG_TIME, MappingText.base64(byteArrayOf(0x16, 0x03, 0x01, 0x02, 0x00, 0x01, 0x00, 0x01, 0xfc.toByte()))),
                    CaptureChunk("download", LOG_TIME + 80, MappingText.base64(byteArrayOf(0x16, 0x03, 0x03, 0x00, 0x7a, 0x02))),
                ),
            ),
        )

        val replayAnswered = replayDraft.copy(
            answer = ReplayResponse(
                status = 200,
                headers = listOf(listOf("content-type", "application/json"), listOf("cache-control", "no-store")),
                elapsedMs = 184,
            ),
            answerBody = BodyText(BodyText.text(PROFILE_JSON.toByteArray(), "application/json"), PROFILE_JSON.length.toLong()),
        )
    }
}
