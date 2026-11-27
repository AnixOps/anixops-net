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

    func testOtherProfilesHaveNone() {
        for text in [
            #"{"routing":{"outbounds":{"node":{"type":"ss"},"Mesh":{"type":"Mesh"}}}}"#,
            #"{"outbound":{"type":"mesh"}}"#,
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
    }

    func testTheClosedGateRefusesSavingAndJoining() throws {
        for command in ["mesh_save_network", "mesh_join"] {
            let request = data(#"{"cmd":"\#(command)","network":"office"}"#)
            let refusal = try XCTUnwrap(EmbeddedRuntimeConfig.meshGateControlRefusal(request, enabled: false))
            XCTAssertEqual(RuntimeControl.Refusal.from(refusal)?.code, "unsupported")
            XCTAssertNil(EmbeddedRuntimeConfig.meshGateControlRefusal(request, enabled: true))
        }
        XCTAssertNil(EmbeddedRuntimeConfig.meshGateControlRefusal(data(#"{"cmd":"mesh_status"}"#), enabled: false))
        XCTAssertNil(EmbeddedRuntimeConfig.meshGateControlRefusal(data("not json"), enabled: false))
    }
}
