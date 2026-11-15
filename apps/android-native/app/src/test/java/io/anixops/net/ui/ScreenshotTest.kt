package io.anixops.net.ui

import com.github.takahirom.roborazzi.captureRoboImage
import io.anixops.net.core.ConnectionSnapshot
import io.anixops.net.core.Delay
import io.anixops.net.core.DnsText
import io.anixops.net.core.HealthStatus
import io.anixops.net.core.LogEntry
import io.anixops.net.core.LogsView
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
import io.anixops.net.core.RuleSetStatus
import io.anixops.net.core.RuntimeSnapshot
import io.anixops.net.core.StatusFact
import io.anixops.net.core.SubscriptionSummary
import io.anixops.net.core.Traffic
import io.anixops.net.core.TrafficBar
import io.anixops.net.core.TrafficPage
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
    fun debugRequestsPhoneLightZh() =
        capture("debug-requests-phone-light-zh", PHONE, "zh-rCN", dark = false, mode = AppMode.Debug)

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
                    initialSettingsPage = settingsPage,
                    initial = destination,
                )
            }
        }
    }

    private fun captureTerms(name: String, device: String, locale: String, dark: Boolean, consent: Boolean) {
        RuntimeEnvironment.setQualifiers("$locale-$device-${if (dark) "night" else "notnight"}-xhdpi")
        val markdown = File(TermsTest.TERMS_PATH).readText()
        captureRoboImage("src/test/screenshots/$name.png") {
            AnixTheme {
                TermsScreen(markdown, onAccept = if (consent) ({}) else null)
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
                    start = 1_794_672_000,
                    upload = 52_428_800,
                    download = 1_181_116_416,
                    bars = List(24) { hour ->
                        TrafficBar(1_794_672_000L + hour * 3_600, listOf(0L, 0L, 0L, 0L, 0L, 0L, 0L, 12, 48, 96, 160, 40).getOrElse(hour) { 0L } * 4_194_304)
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
                SubscriptionSummary(
                    id = "s1",
                    location = "https://panel.example.com/sub",
                    selected = true,
                    name = "Panel",
                    lastSuccessfulUpdate = "2026-11-15 09:30",
                    nodeCount = 3,
                    upload = 3_221_225_472,
                    download = 42_949_672_960,
                    total = 214_748_364_800,
                    expire = 1_798_675_200,
                    updateIntervalHours = 24,
                    refreshable = true,
                ),
                SubscriptionSummary(id = "s2", location = "backup", name = "Backup", nodeCount = 1),
            ),
            page = ProxiesPage(
                view = ProxiesView(
                    total = 4,
                    shown = 4,
                    sections = listOf(
                        NodeSection(
                            "Panel",
                            listOf(
                                NodeRow("tokyo-01", "东京 01", "VLESS", "Panel", selected = true, delay = Delay(86, "fast")),
                                NodeRow("osaka-02", "大阪 02", "Trojan", "Panel", delay = Delay(642, "slow")),
                                NodeRow("hk-03", "香港 03", "Shadowsocks", "Panel", delay = Delay(null, "failed")),
                            ),
                        ),
                        NodeSection(null, listOf(NodeRow("pasted", "Singapore", "Hysteria2"))),
                    ),
                ),
                editable = setOf("tokyo-01", "osaka-02", "hk-03"),
            ),
        )
    }
}
