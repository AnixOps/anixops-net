import Foundation
@testable import NetworkCoreBridge
import XCTest

/// AI tools' access through the MCP program (docs/mcp-desktop.md): the gate the app applies to every engine request.
final class AgentGateTests: XCTestCase {
    private static var root: URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }

    private static func policy() throws -> [String: Any] {
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

    /// Actions only together with reading: turning reading off turns actions off, and a saved switch for actions
    /// without reading counts for nothing.
    func testActionsNeedReading() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "AgentGateTests-\(UUID().uuidString)"))
        XCTAssertFalse(AgentAccess(read: false, act: true).act)
        AgentAccess(read: true, act: true).save(to: defaults)
        XCTAssertTrue(AgentAccess.load(from: defaults).act)
        var access = AgentAccess.load(from: defaults)
        access.read = false
        access.save(to: defaults)
        XCTAssertEqual(AgentAccess.load(from: defaults), .off)
        defaults.set(true, forKey: AgentAccess.actKey)
        XCTAssertFalse(AgentAccess.load(from: defaults).act)
    }

    /// Every action the MCP program sends (crates/client-mcp/tests/fixtures/actions.json) is one the app reads.
    func testEveryActionTheProgramSendsIsRead() throws {
        let url = Self.root.appendingPathComponent("crates/client-mcp/tests/fixtures/actions.json")
        let fixture = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [[String: Any]])
        var kinds = Set<String>()
        var actions: [AgentAction] = []
        for object in fixture {
            let text = String(decoding: try JSONSerialization.data(withJSONObject: object), as: UTF8.self)
            let action = try AgentAction.parse(text).get()
            XCTAssertEqual(action.kind, object["action"] as? String, text)
            kinds.insert(action.kind)
            actions.append(action)
        }
        XCTAssertEqual(kinds, AgentAction.kinds)
        XCTAssertEqual(actions, [
            .connect, .disconnect, .setMode(.global), .selectNode(node: "Node A", group: nil),
            .selectNode(node: "Node A", group: "Provider"), .testNodeDelays, .setCapture(true), .clearCapture,
            .closeConnection(7),
        ])
    }

    func testOnlyKnownActionsWithTheirOwnFieldsAreRead() {
        XCTAssertEqual(AgentAction.parse(#"{"action":"import_subscription","url":"x"}"#), .failure(.notAllowed("import_subscription")))
        XCTAssertEqual(AgentAction.parse(#"{"action":"connect","node":"a"}"#), .failure(.notAllowed("node")))
        XCTAssertEqual(AgentAction.parse(#"{"action":"set_mode","mode":"fast"}"#), .failure(.malformed))
        XCTAssertEqual(AgentAction.parse(#"{"action":"set_mode"}"#), .failure(.malformed))
        XCTAssertEqual(AgentAction.parse(#"{"action":"select_node","node":" "}"#), .failure(.malformed))
        XCTAssertEqual(AgentAction.parse(#"{"action":"select_node","node":"a","group":1}"#), .failure(.malformed))
        XCTAssertEqual(AgentAction.parse(#"{"action":"set_capture","enabled":1}"#), .failure(.malformed))
        XCTAssertEqual(AgentAction.parse(#"{"action":"close_connection","id":-1}"#), .failure(.malformed))
        XCTAssertEqual(AgentAction.parse(#"{"action":"close_connection","id":1.5}"#), .failure(.malformed))
        XCTAssertEqual(AgentAction.parse(#"{"action":"close_connection","id":true}"#), .failure(.malformed))
        XCTAssertEqual(AgentAction.parse(#"{"cmd":"set_capture"}"#), .failure(.malformed))
        XCTAssertEqual(AgentAction.parse("{"), .failure(.malformed))
    }

    func testAGrantCoversItsKindForFifteenMinutes() {
        let start = Date(timeIntervalSince1970: 1000)
        var grants = AgentGrants()
        XCTAssertFalse(grants.allows("select_node", at: start))
        grants.grant("select_node", at: start)
        XCTAssertTrue(grants.allows("select_node", at: start.addingTimeInterval(14 * 60)))
        XCTAssertFalse(grants.allows("select_node", at: start.addingTimeInterval(15 * 60)))
        XCTAssertFalse(grants.allows("disconnect", at: start), "only its own kind")
        grants.revokeAll()
        XCTAssertFalse(grants.allows("select_node", at: start))
    }

    func testTheAccessLogKeepsTheLast200() throws {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "AgentGateTests-\(UUID().uuidString)"))
        XCTAssertTrue(AgentAccessLog.load(from: defaults).entries.isEmpty)
        for index in 0 ..< 205 {
            AgentAccessLog.record("call-\(index)", index.isMultiple(of: 2) ? .allowed : .denied, defaults: defaults)
        }
        let log = AgentAccessLog.load(from: defaults)
        XCTAssertEqual(log.entries.count, AgentAccessLog.limit)
        XCTAssertEqual(log.entries.first?.call, "call-5")
        XCTAssertEqual(log.entries.last?.call, "call-204")
        XCTAssertEqual(log.entries.last?.outcome, .allowed)
        AgentAccessLog.clear(in: defaults)
        XCTAssertTrue(AgentAccessLog.load(from: defaults).entries.isEmpty)
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

        // The refusals of an action, as control_channel.rs maps them.
        for (code, wire) in [(ControlErrorCode.actOff, "actOff"), (.denied, "denied"), (.noAnswer, "noAnswer")] {
            let refusal = try XCTUnwrap(
                JSONSerialization.jsonObject(with: JSONEncoder().encode(ControlResponse.failure(code, "x"))) as? [String: Any]
            )
            XCTAssertEqual(refusal["error"] as? String, wire)
        }
        let act = #"{"agent":"act","command":"agent","payload":"{\"action\":\"connect\"}","version":1}"#
        XCTAssertEqual(
            try JSONDecoder().decode(ControlRequest.self, from: Data(act.utf8)),
            ControlRequest(command: .agent, agent: .act, payload: #"{"action":"connect"}"#)
        )
    }
    #endif

    func testLogLevelsUseTheProgramsNames() {
        let at = Date(timeIntervalSince1970: 2)
        XCTAssertEqual(AgentLogLine(TunnelDiagnosticEvent(time: at, message: "m", level: .warning)).level, "warn")
        XCTAssertEqual(AgentLogLine(TunnelDiagnosticEvent(time: at, message: "m", level: .error)).level, "error")
        XCTAssertEqual(AgentLogLine(TunnelDiagnosticEvent(time: at, message: "m", level: .debug)).atMs, 2000)
    }
}
