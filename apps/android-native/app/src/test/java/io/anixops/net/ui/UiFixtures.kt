package io.anixops.net.ui

import io.anixops.net.core.ConnectionSnapshot
import io.anixops.net.core.DebugMapping
import io.anixops.net.core.DecryptDomain
import io.anixops.net.core.HeaderField
import io.anixops.net.core.HealthStatus
import io.anixops.net.core.MappingText
import io.anixops.net.core.MitmHostOutcome
import io.anixops.net.core.MitmPlugin
import io.anixops.net.core.NetworkProbe
import io.anixops.net.core.NodeSummary
import io.anixops.net.core.NotificationSettings
import io.anixops.net.core.OnDemandSettings
import io.anixops.net.core.Overview
import io.anixops.net.core.RedirectDraft
import io.anixops.net.core.RuntimeSnapshot
import io.anixops.net.core.StatusFact
import io.anixops.net.core.Traffic
import io.anixops.net.core.TrafficRedirect
import io.anixops.net.core.WifiRule

/** Fixtures the screenshot and density tests share, so the layout checks see the states the baselines show. */
internal object UiFixtures {
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

    val decryptingState = connectedState.copy(snapshot = snapshot.copy(httpsDecryptionEnabled = true))

    val extensionsState = ExtensionsState(
        loaded = true,
        consented = true,
        hosts = listOf("api.example.com", "*.example.org"),
        domains = listOf(
            DecryptDomain("api.example.com"),
            DecryptDomain("*.example.org"),
            DecryptDomain("pay.example.org", exclude = true),
        ),
        outcomes = listOf(
            MitmHostOutcome("api.example.com", decrypted = 128, lastDecryptedAtMs = 2),
            MitmHostOutcome(
                "auth.example.org",
                failed = 4,
                lastFailure = "client_rejected_certificate",
                lastFailureAtMs = 1,
            ),
        ),
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
}
