import Foundation
@testable import NetworkCoreBridge
import XCTest

/// AI tools' access through the MCP program (docs/mcp-desktop.md): the gate the app applies to every engine request.
final class AgentGateTests: XCTestCase {
    private static func policy() throws -> [String: Any] {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let url = root.appendingPathComponent("crates/client-mcp/policy.json")
        let object = try JSONSerialization.jsonObject(with: Data(contentsOf: url))
        let policy = try XCTUnwrap(object as? [String: Any])
        return try XCTUnwrap(policy["engine"] as? [String: Any])
    }

    private func gated(_ text: String) throws -> [String: Any] {
        let data = try AgentGate.engineRequest(text).get()
        return try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    /// The app lets through exactly what the MCP program's tests allow it to send.
    func testTheTableIsTheSharedPolicy() throws {
        let policy = try Self.policy()
        XCTAssertEqual(Set(policy.keys), Set(AgentGate.engine.keys))
        for (command, entry) in policy {
            let entry = try XCTUnwrap(entry as? [String: Any], command)
            let fields = try XCTUnwrap(entry["fields"] as? [String], command)
            let forced = try XCTUnwrap(entry["forced"] as? [String: Bool], command)
            XCTAssertEqual(AgentGate.engine[command], AgentGate.Rule(fields: Set(fields), forced: forced), command)
        }
    }

    func testRedactionAndDryRunsAreForced() throws {
        let requests = try gated(#"{"cmd":"capture_requests","filter":"status:200","limit":5,"redact":false}"#)
        XCTAssertEqual(requests["redact"] as? Bool, true)
        XCTAssertEqual(requests["filter"] as? String, "status:200")
        XCTAssertEqual(requests["limit"] as? Int, 5)

        let exchange = try gated(#"{"cmd":"capture_exchange","id":3,"index":0}"#)
        XCTAssertEqual(exchange["redact"] as? Bool, true)

        let rules = try gated(#"{"cmd":"reload_rules","rules":["FINAL,DIRECT"],"dry_run":false}"#)
        XCTAssertEqual(rules["dry_run"] as? Bool, true)
        XCTAssertEqual(rules["rules"] as? [String], ["FINAL,DIRECT"])

        let traffic = try gated(#"{"cmd":"traffic"}"#)
        XCTAssertEqual(traffic.count, 1)
    }

    func testAnythingElseIsRefused() {
        XCTAssertEqual(AgentGate.engineRequest(#"{"cmd":"set_mode","mode":"direct"}"#), .failure(.notAllowed("set_mode")))
        XCTAssertEqual(AgentGate.engineRequest(#"{"cmd":"set_capture","enabled":true}"#), .failure(.notAllowed("set_capture")))
        XCTAssertEqual(AgentGate.engineRequest(#"{"cmd":"export_capture","format":"har"}"#), .failure(.notAllowed("export_capture")))
        XCTAssertEqual(AgentGate.engineRequest(#"{"cmd":"start_lan_share"}"#), .failure(.notAllowed("start_lan_share")))
        XCTAssertEqual(AgentGate.engineRequest(#"{"cmd":"traffic","extra":1}"#), .failure(.notAllowed("extra")))
        XCTAssertEqual(
            AgentGate.engineRequest(#"{"cmd":"capture_requests","since_rev":1}"#),
            .failure(.notAllowed("since_rev"))
        )
        XCTAssertEqual(AgentGate.engineRequest(#"{"cmd":"test_route","host":{"a":1}}"#), .failure(.malformed))
        XCTAssertEqual(AgentGate.engineRequest(#"{"cmd":"reload_rules","rules":[1]}"#), .failure(.malformed))
        XCTAssertEqual(AgentGate.engineRequest("[]"), .failure(.malformed))
        XCTAssertEqual(AgentGate.engineRequest("{"), .failure(.malformed))
        XCTAssertEqual(AgentGate.engineRequest(#"{"host":"a"}"#), .failure(.malformed))
    }

    func testAccessIsOffUntilTheUserTurnsItOn() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "AgentGateTests-\(UUID().uuidString)"))
        XCTAssertEqual(AgentAccess.load(from: defaults), .off)
        AgentAccess(read: true, content: .headers).save(to: defaults)
        XCTAssertEqual(AgentAccess.load(from: defaults), AgentAccess(read: true, content: .headers))
        defaults.set("everything", forKey: AgentAccess.contentKey)
        XCTAssertEqual(AgentAccess.load(from: defaults).content, .summary)
    }

    #if os(macOS)
    /// The wire format the MCP program (crates/client-mcp/src/control_channel.rs) writes and reads.
    func testAgentRequestsAndAnswersOnTheWire() throws {
        let line = #"{"agent":"engine","command":"agent","payload":"{\"cmd\":\"traffic\"}","version":1}"#
        let request = try JSONDecoder().decode(ControlRequest.self, from: Data(line.utf8))
        XCTAssertEqual(request, ControlRequest(command: .agent, agent: .engine, payload: #"{"cmd":"traffic"}"#))

        let answer = ControlResponse(access: AgentAccess(read: true, content: .bodies))
        let encoded = try XCTUnwrap(
            JSONSerialization.jsonObject(with: JSONEncoder().encode(answer)) as? [String: Any]
        )
        let access = try XCTUnwrap(encoded["access"] as? [String: Any])
        XCTAssertEqual(access["read"] as? Bool, true)
        XCTAssertEqual(access["act"] as? Bool, false)
        XCTAssertEqual(access["content"] as? String, "bodies")

        let failure = try XCTUnwrap(
            JSONSerialization.jsonObject(with: JSONEncoder().encode(ControlResponse.failure(.accessOff, "off"))) as? [String: Any]
        )
        XCTAssertEqual(failure["error"] as? String, "accessOff")
    }
    #endif

    func testLogLevelsUseTheProgramsNames() {
        let at = Date(timeIntervalSince1970: 2)
        XCTAssertEqual(AgentLogLine(TunnelDiagnosticEvent(time: at, message: "m", level: .warning)).level, "warn")
        XCTAssertEqual(AgentLogLine(TunnelDiagnosticEvent(time: at, message: "m", level: .error)).level, "error")
        XCTAssertEqual(AgentLogLine(TunnelDiagnosticEvent(time: at, message: "m", level: .debug)).atMs, 2000)
    }
}
