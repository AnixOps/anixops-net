@testable import NetworkCoreBridge
import XCTest

/// The release gate refuses a profile with a mesh outbound while it is closed: the engine compiles mesh in and has
/// no runtime switch (docs/architecture/mesh-client.md, 发布闸门).
final class MeshGateTests: XCTestCase {
    private func data(_ text: String) -> Data {
        Data(text.utf8)
    }

    func testAMeshOutboundInTheObjectFormIsFound() {
        let profile = data(#"{"routing":{"outbounds":{"node":{"type":"ss"},"mesh-x":{"type":"mesh"}},"rules":["FINAL,mesh-x"]}}"#)
        XCTAssertTrue(EmbeddedRuntimeConfig.containsMeshOutbound(profile))
    }

    func testAMeshOutboundInTheArrayFormIsFound() {
        let profile = data(#"{"routing":{"outbounds":[{"type":"ss"},{"type":"mesh"}]}}"#)
        XCTAssertTrue(EmbeddedRuntimeConfig.containsMeshOutbound(profile))
    }

    func testATopLevelOutboundAndInlineViaObjectsAreFound() {
        for text in [
            #"{"outbound":{"type":"mesh"}}"#,
            #"{"outbound":{"type":"ss","via":{"type":"mesh"}}}"#,
            #"{"outbound":{"type":"ss","via":{"type":"ss","via":{"type":"mesh"}}}}"#,
            #"{"routing":{"outbounds":{"a":{"type":"ss","via":{"type":"mesh"}}}}}"#,
            #"{"routing":{"outbounds":[{"type":"ss","via":{"type":"ss","via":{"type":"mesh"}}}]}}"#,
        ] {
            XCTAssertTrue(EmbeddedRuntimeConfig.containsMeshOutbound(data(text)), text)
        }
    }

    func testOtherProfilesHaveNone() {
        for text in [
            #"{"routing":{"outbounds":{"node":{"type":"ss"},"Mesh":{"type":"Mesh"}}}}"#,
            #"{"outbound":{"type":"ss","via":"mesh"}}"#,
            #"{"outbound":{"type":"ss","via":{"type":"ss"}}}"#,
            #"{"routing":{"outbounds":1}}"#,
            #"{}"#,
            "not json",
        ] {
            XCTAssertFalse(EmbeddedRuntimeConfig.containsMeshOutbound(data(text)), text)
        }
    }

    func testTheClosedGateRefusesAndTheOpenGateDoesNot() {
        let profile = data(#"{"routing":{"outbounds":{"m":{"type":"mesh"}}}}"#)
        XCTAssertEqual(EmbeddedRuntimeConfig.meshGateFailure(profile, enabled: false), EmbeddedRuntimeConfig.meshGatedReason)
        XCTAssertNil(EmbeddedRuntimeConfig.meshGateFailure(profile, enabled: true))
        XCTAssertNil(EmbeddedRuntimeConfig.meshGateFailure(data(#"{"routing":{"outbounds":{}}}"#), enabled: false))
        XCTAssertTrue(EmbeddedRuntimeConfig.isMeshGated(EmbeddedRuntimeConfig.meshGatedReason))
        XCTAssertTrue(EmbeddedRuntimeConfig.meshGatedReason.hasPrefix("error.mesh_gated: "))
    }

    func testTheClosedGateRefusesEveryMeshCommandButStopping() throws {
        for command in ["mesh_save_network", "mesh_join", "mesh_join_credential", "mesh_status", "mesh_unheard_of"] {
            let request = data(#"{"cmd":"\#(command)","network":"office"}"#)
            let refusal = try XCTUnwrap(EmbeddedRuntimeConfig.meshGateControlRefusal(request, enabled: false), command)
            XCTAssertEqual(RuntimeControl.Refusal.from(refusal)?.code, "error.mesh_gated")
            XCTAssertNil(EmbeddedRuntimeConfig.meshGateControlRefusal(request, enabled: true))
        }
        XCTAssertNil(EmbeddedRuntimeConfig.meshGateControlRefusal(data(#"{"cmd":"mesh_engine_stop"}"#), enabled: false))
        XCTAssertNil(EmbeddedRuntimeConfig.meshGateControlRefusal(data(#"{"cmd":"status"}"#), enabled: false))
        XCTAssertNil(EmbeddedRuntimeConfig.meshGateControlRefusal(data(#"{"cmd":"set_rule","name":"mesh_x"}"#), enabled: false))
    }

    func testARequestThatNamesMeshButCannotBeReadIsRefused() {
        for text in ["mesh_join", #"{"cmd":"mesh_join""#, #"[{"cmd":"mesh_join"}]"#, #"{"cmd":1,"x":"mesh_join"}"#] {
            XCTAssertNotNil(EmbeddedRuntimeConfig.meshGateControlRefusal(data(text), enabled: false), text)
        }
        XCTAssertNil(EmbeddedRuntimeConfig.meshGateControlRefusal(data("not json"), enabled: false))
    }

    /// The start paths of the extension call the gate: removing a call fails here (the extension is not a test
    /// target, so its source is read).
    func testTheTunnelStartsAndTheControlPathCallTheGate() throws {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let source = try String(
            contentsOf: root.appendingPathComponent("Sources/NetworkCorePacketTunnel/PacketTunnelProvider.swift"),
            encoding: .utf8
        )
        func count(_ needle: String) -> Int {
            source.components(separatedBy: needle).count - 1
        }
        XCTAssertEqual(count("EmbeddedRuntimeConfig.meshGateFailure(of: "), 2, "launchTunnel and beginMeshOnly")
        XCTAssertEqual(count("EmbeddedRuntimeConfig.meshGateControlRefusal("), 1, "the control path")
        XCTAssertFalse(source.contains("try? config.encoded()"))
    }
}
