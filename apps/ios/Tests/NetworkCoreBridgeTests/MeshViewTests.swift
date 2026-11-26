@testable import NetworkCoreBridge
import XCTest

/// `MeshView` gives the answers the Rust `view.mesh_*` commands give: every `view.mesh_*` sample in
/// `crates/client-view/fixtures/cases.json` is run here too.
final class MeshViewTests: XCTestCase {
    private static func cases() throws -> [[String: Any]] {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let url = root.appendingPathComponent("crates/client-view/fixtures/cases.json")
        let object = try JSONSerialization.jsonObject(with: Data(contentsOf: url), options: [.fragmentsAllowed])
        let all = try XCTUnwrap(object as? [[String: Any]])
        return all.filter { ($0["command"] as? String)?.hasPrefix("view.mesh_") == true }
    }

    private static func answer(_ command: String, _ args: [String: Any]) -> Any? {
        let status = args["status"]
        let string = { (name: String) in args[name] as? String ?? "" }
        switch command {
        case "view.mesh_entry": return MeshView.entry(status: status)
        case "view.mesh_members":
            return MeshView.members(
                status: status,
                groupBy: string("groupBy"),
                filter: string("filter"),
                sort: string("sort"),
                query: string("query"),
                devices: args["devices"] as? [Any] ?? []
            )
        case "view.mesh_subnets": return MeshView.subnets(status: status, query: string("query"))
        case "view.mesh_paths": return MeshView.paths(status: status)
        case "view.mesh_notices": return MeshView.notices(status: status)
        case "view.mesh_region": return MeshView.region(status: status)
        case "view.mesh_latency_tone": return MeshView.latencyTone(MeshView.unsigned(args["latencyMs"]))
        default: return nil
        }
    }

    /// Both sides through `JSONSerialization`, so numbers, booleans and nulls compare alike.
    private static func normalised(_ value: Any) throws -> Any {
        let data = try JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed, .sortedKeys])
        return try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
    }

    func testEverySharedSampleGetsTheSameAnswer() throws {
        let cases = try Self.cases()
        XCTAssertGreaterThan(cases.count, 15, "the mesh samples are missing")
        for sample in cases {
            let command = try XCTUnwrap(sample["command"] as? String)
            let args = sample["args"] as? [String: Any] ?? [:]
            let answer = try XCTUnwrap(Self.answer(command, args), "\(command) has no Swift side")
            let got = try Self.normalised(answer) as AnyObject
            let expected = try Self.normalised(try XCTUnwrap(sample["answer"])) as AnyObject
            XCTAssertTrue(got.isEqual(expected), "\(command) \(args.keys.sorted()): got \(got), expected \(expected)")
        }
    }

    func testAddressesFallInsideTheirNetworks() throws {
        let net = try XCTUnwrap(MeshView.Cidr("192.168.20.0/24"))
        XCTAssertTrue(net.contains(try XCTUnwrap(MeshView.Address("192.168.20.9"))))
        XCTAssertFalse(net.contains(try XCTUnwrap(MeshView.Address("192.168.21.9"))))
        XCTAssertFalse(net.contains(try XCTUnwrap(MeshView.Address("fd00::1"))))
        let v6 = try XCTUnwrap(MeshView.Cidr("fd00:aa::/64"))
        XCTAssertTrue(v6.contains(try XCTUnwrap(MeshView.Address("fd00:aa::5"))))
        XCTAssertFalse(v6.contains(try XCTUnwrap(MeshView.Address("fd00:ab::5"))))
        XCTAssertNil(MeshView.Cidr("10.0.0.0/33"))
        XCTAssertNil(MeshView.Cidr("example"))
    }

    func testStripV04LeavesTheKernel03Fields() throws {
        let status: [String: Any] = [
            "network": ["state": "joined"],
            "regions": [["code": "eu"]],
            "links": [],
            "self": ["id": "a", "region": "eu", "regionBasis": [:], "regionState": "auto", "hostMode": true, "address": "10.0.0.1/24"],
            "members": [["id": "b", "region": "eu", "vms": [], "bridgesExternal": true, "role": "host"]],
            "subnets": [["cidr": "10.1.0.0/24", "region": "eu", "kind": "lan", "name": "x", "state": "ok"]],
        ]
        let expected: [String: Any] = [
            "network": ["state": "joined"],
            "self": ["id": "a", "address": "10.0.0.1/24"],
            "members": [["id": "b", "role": "host"]],
            "subnets": [["cidr": "10.1.0.0/24", "state": "ok"]],
        ]
        let got = try Self.normalised(MeshView.stripV04(status)) as AnyObject
        XCTAssertTrue(got.isEqual(try Self.normalised(expected) as AnyObject), "\(got)")
    }
}
