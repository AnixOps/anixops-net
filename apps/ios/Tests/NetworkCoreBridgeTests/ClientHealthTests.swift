import Foundation
import NetworkCoreBridge
import XCTest

final class ClientHealthTests: XCTestCase {
    func testRedactionRemovesUrlsHostsAddressesAndKeys() {
        let text = """
        fetch https://sub.example.com/api/v1/client/subscribe?token=abc123 failed via hk.example.net:443 \
        (203.0.113.7:8443, [2001:db8::1]:443, 2001:db8::2) for a@b.cn \
        key Zm9vYmFyYmF6cXV4MTIzNDU2Nzg5MA== id 3F2504E0-4F89-11D3-9A0C-0305E82C3301
        """
        let redacted = Redaction.redact(text)
        for gone in ["example", "203.0.113.7", "2001:db8", "a@b.cn", "Zm9v", "3F2504E0", "token"] {
            XCTAssertFalse(redacted.contains(gone), "\(gone) in \(redacted)")
        }
        for mark in ["<url>", "<host>", "<ip>", "<email>", "<secret>", "<id>"] {
            XCTAssertTrue(redacted.contains(mark), "\(mark) missing in \(redacted)")
        }
    }

    func testRedactionKeepsSourceLocationsAndPlainText() {
        let text = "Fatal error at PacketTunnelProvider.swift:120 after 1500 ms, version 0.3.0 build 5022"
        XCTAssertEqual(Redaction.redact(text), text)
    }

    func testCrashReportsAreRedactedTrimmedAndDeletedWhenOff() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("crash-\(UUID().uuidString)")
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "crash-tests-\(UUID().uuidString)"))
        let store = CrashReportStore(directory: directory, defaults: defaults)
        XCTAssertTrue(store.isEnabled)
        let report = CrashReportStore.exceptionReport(
            app: "AnixOps Net",
            version: "0.3.0",
            name: "NSInvalidArgumentException",
            reason: "bad URL https://panel.example.com/sub?token=x for 198.51.100.4",
            symbols: ["0 CoreFoundation 0x1 __exceptionPreprocess + 164"]
        )
        for index in 0 ..< CrashReportStore.maxReports + 3 {
            XCTAssertNotNil(store.write(report, at: Date(timeIntervalSince1970: TimeInterval(1_700_000_000 + index))))
        }
        XCTAssertEqual(store.pending().count, CrashReportStore.maxReports)
        let exported = store.export()
        XCTAssertTrue(exported.contains("NSInvalidArgumentException"))
        XCTAssertFalse(exported.contains("panel.example.com"))
        XCTAssertFalse(exported.contains("198.51.100.4"))
        XCTAssertTrue(ClientHealthReport.text(integrity: .verified(extensions: 1), crashReports: store)
            .contains("install integrity: verified (1 extensions match the app)"))

        store.isEnabled = false
        XCTAssertTrue(store.pending().isEmpty)
        XCTAssertNil(store.write(report))
        try? FileManager.default.removeItem(at: directory)
    }

    func testIntegrityNeedsAMatchingPacketTunnel() {
        let info: [String: Any] = ["CFBundleShortVersionString": "0.3.0", "CFBundleVersion": "7"]
        var tunnel = info
        tunnel["NSExtension"] = ["NSExtensionPointIdentifier": "com.apple.networkextension.packet-tunnel"]
        var macTunnel = info
        macTunnel["NetworkExtension"] = ["NEProviderClasses": ["com.apple.networkextension.packet-tunnel": "Provider"]]
        var widget = info
        widget["NSExtension"] = ["NSExtensionPointIdentifier": "com.apple.widgetkit-extension"]

        XCTAssertEqual(InstallIntegrity.check(appInfo: info, extensions: [("Tunnel.appex", tunnel), ("Widget.appex", widget)]), .verified(extensions: 2))
        XCTAssertEqual(InstallIntegrity.check(appInfo: info, extensions: [("tunnel.systemextension", macTunnel)]), .verified(extensions: 1))
        XCTAssertEqual(InstallIntegrity.check(appInfo: info, extensions: [("Widget.appex", widget)]), .damaged("the packet tunnel extension is missing"))
        var old = tunnel
        old["CFBundleVersion"] = "6"
        XCTAssertEqual(InstallIntegrity.check(appInfo: info, extensions: [("Tunnel.appex", old)]), .damaged("Tunnel.appex is from another build"))
        XCTAssertEqual(InstallIntegrity.check(appInfo: info, extensions: [("Tunnel.appex", nil)]), .damaged("Tunnel.appex has no readable Info.plist"))
    }

    func testDiagnosticsCarryPrometheusText() throws {
        XCTAssertEqual(
            String(decoding: RuntimeControl.metrics(anonymous: true).request, as: UTF8.self),
            #"{"anonymous":true,"cmd":"metrics"}"#
        )
        let answer = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.Metrics.self, from: Data(
            #"{"content_type":"text/plain; version=0.0.4; charset=utf-8","text":"networkcore_uptime_seconds 3.000\n"}"#.utf8
        )))
        let diagnostics = TunnelDiagnostics(
            runtimeStatus: nil,
            memoryFootprintBytes: 1024,
            uptimeSeconds: 2.5,
            handshake: .current,
            events: [],
            engineMetrics: answer.text
        )
        let text = diagnostics.prometheusText
        XCTAssertTrue(text.contains("# TYPE anixops_extension_memory_bytes gauge\nanixops_extension_memory_bytes 1024\n"))
        XCTAssertTrue(text.contains("anixops_extension_uptime_seconds 2.500\n"))
        XCTAssertTrue(text.hasSuffix("networkcore_uptime_seconds 3.000\n"))
        XCTAssertFalse(text.contains("anixops_extension_memory_peak_bytes"))
        XCTAssertEqual(try TunnelDiagnostics.decoded(from: diagnostics.encoded()), diagnostics)
    }
}
