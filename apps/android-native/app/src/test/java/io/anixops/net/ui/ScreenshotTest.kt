package io.anixops.net.ui

import com.github.takahirom.roborazzi.captureRoboImage
import io.anixops.net.core.ConnectionSnapshot
import io.anixops.net.core.Delay
import io.anixops.net.core.NetworkProbe
import io.anixops.net.core.NodeRow
import io.anixops.net.core.NodeSection
import io.anixops.net.core.NodeSummary
import io.anixops.net.core.Overview
import io.anixops.net.core.ProxiesPage
import io.anixops.net.core.ProxiesView
import io.anixops.net.core.RuntimeSnapshot
import io.anixops.net.core.StatusFact
import io.anixops.net.core.SubscriptionSummary
import io.anixops.net.core.Traffic
import io.anixops.net.ui.theme.AnixTheme
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
    ) {
        RuntimeEnvironment.setQualifiers("$locale-$device-${if (dark) "night" else "notnight"}-xhdpi")
        captureRoboImage("src/test/screenshots/$name.png") {
            AnixTheme {
                AnixScaffold(
                    state = state,
                    mode = mode,
                    version = "0.3.0",
                    onModeChange = {},
                    proxiesState = proxies,
                    initial = destination,
                )
            }
        }
    }

    private companion object {
        const val PHONE = "w411dp-h891dp"
        const val TABLET = "w1280dp-h800dp"

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
