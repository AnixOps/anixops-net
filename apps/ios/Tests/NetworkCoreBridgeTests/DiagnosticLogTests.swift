import Foundation
import NetworkCoreBridge
import XCTest

final class DiagnosticLogTests: XCTestCase {
    private func classify(_ message: String) -> String {
        let result = DiagnosticModule.classify(message)
        return "\(result.level.rawValue) \(result.module.rawValue)"
    }

    func testExtensionMessagesClassify() {
        XCTAssertEqual(classify("startTunnel"), "info tunnel")
        XCTAssertEqual(classify("startTunnel failed: no network"), "error tunnel")
        XCTAssertEqual(classify("stopTunnel reason 1"), "info tunnel")
        XCTAssertEqual(classify("stopTunnel reason 2"), "warning tunnel")
        XCTAssertEqual(classify("profile loaded, mtu 1500, 1 addresses, 2 routes, 1 dns servers"), "debug tunnel")
        XCTAssertEqual(classify("runtime initialized"), "info runtime")
        XCTAssertEqual(classify("mode rule"), "info rules")
        XCTAssertEqual(classify("subscription rules off: node list is malformed"), "error rules")
        XCTAssertEqual(classify("subscription rules on, 3 nodes, 10 rules, 2 groups"), "info rules")
        XCTAssertEqual(classify("rules reloaded in place, 3 rules, generation 2"), "info rules")
        XCTAssertEqual(classify("rules reload refused (4): restart"), "error rules")
        XCTAssertEqual(classify("local rules changed beyond the rule lines: restart"), "warning rules")
        XCTAssertEqual(classify("dns 2 servers, 1 rules, 0 hosts"), "info dns")
        XCTAssertEqual(classify("https decryption off: no root certificate"), "warning mitm")
        XCTAssertEqual(classify("https decryption off: runtime refused the plugins (code 4)"), "error mitm")
        XCTAssertEqual(classify("scripts off: could not write them"), "error scripts")
        XCTAssertEqual(classify("2 scheduled scripts"), "info scripts")
        XCTAssertEqual(classify("network changed"), "info network")
        XCTAssertEqual(classify("bound interface: none after 2 s, engine unpinned"), "warning network")
        XCTAssertEqual(classify("bound interface en0 (code 0)"), "debug network")
        XCTAssertEqual(classify("memory pressure warn, footprint 30 MB, headroom 10 MB"), "warning memory")
        XCTAssertEqual(classify("device wake"), "debug tunnel")
    }

    func testExplicitLevelAndModuleWin() {
        let event = TunnelDiagnosticEvent(time: Date(), message: "startTunnel", level: .error, module: .dns)
        XCTAssertEqual(event.level, .error)
        XCTAssertEqual(event.module, .dns)
    }

    func testOldAndUnknownJSONStillDecodes() throws {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .millisecondsSince1970
        let old = try decoder.decode(TunnelDiagnosticEvent.self, from: Data(#"{"time":0,"message":"scripts off: could not write them"}"#.utf8))
        XCTAssertEqual(old.level, .error)
        XCTAssertEqual(old.module, .scripts)
        let newer = try decoder.decode(
            TunnelDiagnosticEvent.self, from: Data(#"{"time":0,"message":"device wake","level":"trace","module":"bluetooth"}"#.utf8)
        )
        XCTAssertEqual(newer.level, .debug)
        XCTAssertEqual(newer.module, .tunnel)
    }

    func testFilterByLevelAndModule() {
        let log = TunnelDiagnosticLog()
        log.record("startTunnel")
        log.record("device wake")
        log.record("https decryption off: no root certificate")
        log.record("startTunnel failed: no network")
        let events = log.snapshot()
        XCTAssertEqual(events.filtered(minimum: .debug).count, 4)
        XCTAssertEqual(events.filtered(minimum: .info).count, 3)
        XCTAssertEqual(events.filtered(minimum: .warning).map(\.message), [
            "https decryption off: no root certificate", "startTunnel failed: no network",
        ])
        XCTAssertEqual(events.filtered(minimum: .debug, modules: [.mitm]).count, 1)
        XCTAssertEqual(events.filtered(minimum: .error, modules: [.mitm]).count, 0)
    }
}
