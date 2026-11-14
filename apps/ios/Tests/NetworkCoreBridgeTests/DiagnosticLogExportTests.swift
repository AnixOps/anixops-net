import Foundation
import NetworkCoreBridge
import XCTest

final class DiagnosticLogExportTests: XCTestCase {
    private func report(lastError: String?) -> DeveloperReport {
        DeveloperReport(
            appVersion: "0.3.0",
            appBuild: "5030",
            systemVersion: "iOS 26.1",
            connection: "已连接",
            outboundKind: "anx",
            lastDisconnectError: lastError,
            diagnostics: TunnelDiagnostics(
                runtimeStatus: #"{"dns_queries":3}"#,
                memoryFootprintBytes: 12_582_912,
                uptimeSeconds: 61,
                handshake: .current,
                events: [TunnelDiagnosticEvent(time: Date(timeIntervalSince1970: 1_700_000_000), message: "device sleep")]
            ),
            generatedAt: Date(timeIntervalSince1970: 1_700_000_100)
        )
    }

    func testFileNameIsALogFileInLocalTime() {
        let name = DiagnosticLogExport.fileName(
            at: Date(timeIntervalSince1970: 1_700_000_000),
            timeZone: TimeZone(identifier: "Asia/Shanghai")!
        )
        XCTAssertEqual(name, "AnixOps-Net-20231115-061320.log")
    }

    func testContentsAreRedactedAndCarryThePowerSection() {
        let text = DiagnosticLogExport.contents(
            report: report(lastError: "dial 203.0.113.7:443 via node.example.com failed, key QUJDREVGR0hJSktMTU5PUA1234"),
            power: ["thermal state: serious", "low power mode: false"],
            environment: [:]
        )
        XCTAssertTrue(text.contains("NetworkCore diagnostics"))
        XCTAssertTrue(text.contains("device sleep"))
        XCTAssertTrue(text.contains("power:\nthermal state: serious\nlow power mode: false"))
        XCTAssertFalse(text.contains("203.0.113.7"))
        XCTAssertFalse(text.contains("node.example.com"))
        XCTAssertFalse(text.contains("QUJDREVGR0hJSktMTU5PUA1234"))
        XCTAssertTrue(text.hasSuffix("\n"))
    }

    func testWriteReplacesEarlierExports() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("log-export-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: directory) }

        let first = try DiagnosticLogExport.write("one\n", at: Date(timeIntervalSince1970: 1), in: directory)
        let second = try DiagnosticLogExport.write("two\n", at: Date(timeIntervalSince1970: 2), in: directory)

        XCTAssertEqual(second.pathExtension, "log")
        XCTAssertFalse(FileManager.default.fileExists(atPath: first.path))
        XCTAssertEqual(try String(contentsOf: second, encoding: .utf8), "two\n")
    }

    func testThermalStatesHaveNames() {
        XCTAssertEqual(DiagnosticLogExport.describe(thermalState: .nominal), "nominal")
        XCTAssertEqual(DiagnosticLogExport.describe(thermalState: .critical), "critical")
    }
}
