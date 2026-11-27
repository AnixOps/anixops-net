@testable import NetworkCoreBridge
import XCTest

/// The release gate refuses a profile with a mesh outbound while it is closed: the engine has mesh compiled in and
/// its only switch is `allow_mesh` (docs/architecture/mesh-client.md, 发布闸门).
/// What a probe answers, set by the test thread and read by the probe's background thread.
private final class ReplyBox: @unchecked Sendable {
    private let lock = NSLock()
    private var reply: Data?

    var value: Data? {
        get {
            lock.lock()
            defer { lock.unlock() }
            return reply
        }
        set {
            lock.lock()
            defer { lock.unlock() }
            reply = newValue
        }
    }
}

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

    func testAnInlineViaChainDeeperThanTheCapIsTreatedAsMesh() throws {
        func chain(depth: Int, last: String) -> Data {
            var entry = last
            for _ in 0..<depth {
                entry = #"{"type":"ss","via":"# + entry + "}"
            }
            return data(#"{"outbound":"# + entry + "}")
        }
        // Within the cap a chain without mesh is clean, and one with mesh is found.
        XCTAssertFalse(EmbeddedRuntimeConfig.containsMeshOutbound(chain(depth: 10, last: #"{"type":"ss"}"#)))
        XCTAssertTrue(EmbeddedRuntimeConfig.containsMeshOutbound(chain(depth: 10, last: #"{"type":"mesh"}"#)))
        // Past the cap it is refused whatever it ends in: the kernel refuses a chain over 4 anyway.
        let deep = EmbeddedRuntimeConfig.maxViaDepth + 2
        XCTAssertTrue(EmbeddedRuntimeConfig.containsMeshOutbound(chain(depth: deep, last: #"{"type":"ss"}"#)))
        XCTAssertNotNil(EmbeddedRuntimeConfig.meshGateFailure(chain(depth: deep, last: #"{"type":"ss"}"#), enabled: false))
    }

    // MARK: allow_mesh

    private func allowMesh(_ config: EmbeddedRuntimeConfig) throws -> Any? {
        let object = try JSONSerialization.jsonObject(with: config.encoded()) as? [String: Any]
        return object?["allow_mesh"]
    }

    private func base() -> EmbeddedRuntimeConfig {
        EmbeddedRuntimeConfig(
            outbound: .native(.init(type: "direct", server: "", port: 0, settings: [:])), profileID: "p"
        )
    }

    func testTheFinalEnvelopeCarriesTheGateAndNeverMoreThanTheGate() throws {
        XCTAssertNil(try allowMesh(base()), "the stored profile carries none")
        XCTAssertEqual(try allowMesh(base().settingAllowMesh(false)) as? Bool, false)
        XCTAssertEqual(try allowMesh(base().settingAllowMesh(true)) as? Bool, true)
        // A config that said true is overwritten by a closed gate.
        XCTAssertEqual(try allowMesh(base().settingAllowMesh(true).settingAllowMesh(false)) as? Bool, false)
        XCTAssertNil(base().settingAllowMesh(true).withoutSecret().allowMesh)
        // What the engine gets (`EngineBudget.encoded`) carries the build's gate.
        let sent = try JSONSerialization.jsonObject(with: EngineBudget.encoded(base().settingAllowMesh(true))) as? [String: Any]
        XCTAssertEqual(sent?["allow_mesh"] as? Bool, MeshSupport.enabled)
        XCTAssertEqual(base().withAllowMesh().allowMesh, MeshSupport.enabled)
        XCTAssertEqual(EmbeddedRuntimeConfig.withAllowMesh(["allow_mesh": !MeshSupport.enabled])["allow_mesh"] as? Bool, MeshSupport.enabled)
        // An envelope that is not a config object gets the same.
        let object = EmbeddedRuntimeConfig.settingAllowMesh(["allow_mesh": true, "x": 1], false)
        XCTAssertEqual(object["allow_mesh"] as? Bool, false)
        XCTAssertEqual(EmbeddedRuntimeConfig.settingAllowMesh([:], true)["allow_mesh"] as? Bool, true)
    }

    /// Only the extension's own refusal reads as the gated one: the kernel's reaches Swift as a bare code.
    func testOnlyTheExtensionsOwnRefusalReadsAsTheGatedOne() {
        XCTAssertTrue(EmbeddedRuntimeConfig.isMeshGated(EmbeddedRuntimeConfig.meshGatedReason))
        XCTAssertFalse(EmbeddedRuntimeConfig.isMeshGated("config_rejected: bad port"))
        XCTAssertFalse(EmbeddedRuntimeConfig.isMeshGated("config_rejected: mesh outbounds are not allowed by this config"))
    }

    func testTheAnswerToCapabilitiesDecidesAvailability() {
        XCTAssertTrue(MeshSupport.runsMesh(capabilitiesAnswer: data(#"{"capabilities":{"mesh":true}}"#)))
        XCTAssertFalse(MeshSupport.runsMesh(capabilitiesAnswer: data(#"{"capabilities":{"mesh":false}}"#)))
        XCTAssertFalse(MeshSupport.runsMesh(capabilitiesAnswer: data(#"{"ok":false,"error":{"code":"unsupported"}}"#)))
        XCTAssertFalse(MeshSupport.runsMesh(capabilitiesAnswer: data("not json")))
        XCTAssertFalse(MeshSupport.runsMesh(capabilitiesAnswer: nil))
    }

    /// No answer is not an answer: only a reply is kept (the engine's own refusal is one, "unsupported" is a no).
    func testOnlyARealAnswerIsKept() {
        XCTAssertEqual(MeshSupport.meshCapability(capabilitiesAnswer: data(#"{"capabilities":{"mesh":true}}"#)), true)
        XCTAssertEqual(MeshSupport.meshCapability(capabilitiesAnswer: data(#"{"capabilities":{"mesh":false}}"#)), false)
        XCTAssertEqual(MeshSupport.meshCapability(capabilitiesAnswer: data(#"{"ok":false,"error":{"code":"unsupported"}}"#)), false)
        XCTAssertNil(MeshSupport.meshCapability(capabilitiesAnswer: data("not json")))
        XCTAssertNil(MeshSupport.meshCapability(capabilitiesAnswer: data(#"{"other":1}"#)))
        XCTAssertNil(MeshSupport.meshCapability(capabilitiesAnswer: nil))
    }

    /// With an installed probe, availability is the gate and the engine's answer, for both values of the gate; a failed
    /// probe is not kept and the next ask answers; the answer kept stays. (The gate is a parameter here: a test cannot
    /// rebuild the app with the other condition.)
    func testAvailabilityIsTheGateAndTheEnginesAnswerForBothGateValues() {
        let reply = ReplyBox()
        MeshSupport.installCapabilitiesProbe { reply.value }
        defer { MeshSupport.installCapabilitiesProbe { nil } }
        XCTAssertFalse(MeshSupport.available(enabled: false))
        // A failed probe: no, and not kept.
        XCTAssertNil(MeshSupport.askEngine())
        XCTAssertFalse(MeshSupport.available(enabled: true))
        // The engine answers: yes with the gate open, no with it closed; kept.
        reply.value = data(#"{"capabilities":{"mesh":true}}"#)
        XCTAssertEqual(MeshSupport.askEngine(), true)
        XCTAssertTrue(MeshSupport.available(enabled: true))
        XCTAssertFalse(MeshSupport.available(enabled: false))
        reply.value = nil
        XCTAssertTrue(MeshSupport.available(enabled: true), "a kept answer is not asked again")
        // An engine that says no stays no with either gate.
        reply.value = data(#"{"capabilities":{"mesh":false}}"#)
        MeshSupport.installCapabilitiesProbe { reply.value }
        XCTAssertEqual(MeshSupport.askEngine(), false)
        XCTAssertFalse(MeshSupport.available(enabled: true))
        XCTAssertFalse(MeshSupport.available(enabled: false))
        XCTAssertFalse(MeshSupport.available)
    }

    /// A probe asked before a newer install answers into nothing: its answer is handed back but not kept.
    func testAStaleProbeAnswerDoesNotOverwriteANewerInstall() {
        MeshSupport.installCapabilitiesProbe { nil }
        defer { MeshSupport.installCapabilitiesProbe { nil } }
        let stale = MeshSupport.probeGeneration
        MeshSupport.installCapabilitiesProbe { nil }
        XCTAssertNotEqual(MeshSupport.probeGeneration, stale)
        let late = MeshSupport.askEngine(probe: { self.data(#"{"capabilities":{"mesh":true}}"#) }, generation: stale)
        XCTAssertEqual(late, true, "the caller still gets the answer")
        XCTAssertFalse(MeshSupport.available(enabled: true), "but the newer install does not cache it")
    }

    /// The first answer announces itself, so the entry can be read again; the same answer twice does not.
    func testTheFirstAnswerPostsAvailabilityDidChange() {
        let reply = ReplyBox()
        MeshSupport.installCapabilitiesProbe { reply.value }
        defer { MeshSupport.installCapabilitiesProbe { nil } }
        reply.value = data(#"{"capabilities":{"mesh":true}}"#)
        let posted = expectation(forNotification: MeshSupport.availabilityDidChange, object: nil)
        XCTAssertEqual(MeshSupport.askEngine(), true)
        wait(for: [posted], timeout: 1)
        let again = expectation(forNotification: MeshSupport.availabilityDidChange, object: nil)
        again.isInverted = true
        XCTAssertEqual(MeshSupport.askEngine(), true)
        wait(for: [again], timeout: 0.2)
    }

    // MARK: call sites

    private func source(_ path: String) throws -> [String] {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let text = try String(contentsOf: root.appendingPathComponent(path), encoding: .utf8)
        // Comment lines (`//`, `///`) may name a call; only code counts.
        return text.components(separatedBy: "\n").filter { !$0.trimmingCharacters(in: .whitespaces).hasPrefix("//") }
    }

    /// The code lines of `lines` between the `func` line that contains `signature` and the next function declaration.
    private func body(of signature: String, in lines: [String]) -> String {
        guard let start = lines.firstIndex(where: { $0.contains("func ") && $0.contains(signature) }) else {
            return ""
        }
        let rest = lines[(start + 1)...]
        let end = rest.firstIndex { $0.hasPrefix("    ") && !$0.hasPrefix("     ") && $0.contains("func ") }
        return rest[..<(end ?? lines.endIndex)].joined(separator: "\n")
    }

    /// The start paths of the extension call the gate, each inside the function that starts an engine: removing a call
    /// fails here (the extension is not a test target, so its source is read).
    func testTheTunnelStartsAndTheControlPathCallTheGate() throws {
        let lines = try source("Sources/NetworkCorePacketTunnel/PacketTunnelProvider.swift")
        let source = lines.joined(separator: "\n")
        func count(_ needle: String, in text: String) -> Int {
            text.components(separatedBy: needle).count - 1
        }
        let gate = "EmbeddedRuntimeConfig.meshGateFailure(of: "
        XCTAssertEqual(count(gate, in: source), 2, "launchTunnel and beginMeshOnly")
        XCTAssertEqual(count(gate, in: body(of: "beginTunnel(", in: lines)), 1)
        XCTAssertEqual(count(gate, in: body(of: "beginMeshOnly(", in: lines)), 1)
        XCTAssertEqual(count("EmbeddedRuntimeConfig.meshGateControlRefusal(", in: source), 1, "the control path")
        XCTAssertEqual(
            count("EmbeddedRuntimeConfig.meshGateControlRefusal(", in: body(of: "handleAppMessage(", in: lines)), 1
        )
        XCTAssertFalse(source.contains("try? config.encoded()"))
    }

    /// Every engine start encodes through `EngineBudget.encoded`, which sets `allow_mesh` to the gate; no start path
    /// encodes a config itself, and nothing writes `allow_mesh: true` as a literal.
    func testEveryEngineStartCarriesTheGateInTheEnvelope() throws {
        let provider = try source("Sources/NetworkCorePacketTunnel/PacketTunnelProvider.swift").joined(separator: "\n")
        let lines = provider.components(separatedBy: "\n")
        func starts(_ function: String) -> Int {
            body(of: function, in: lines).components(separatedBy: "EngineBudget.encoded(").count - 1
        }
        // The start functions each, not the two length checks (`makeSession`, the reload's) that also encode.
        XCTAssertGreaterThanOrEqual(starts("beginMeshOnly("), 1)
        XCTAssertGreaterThanOrEqual(starts("makeRuntimeSession("), 3)
        XCTAssertFalse(provider.contains("EmbeddedRuntimeSession(config: config.encoded"))
        let budget = try source("Sources/NetworkCoreBridge/EngineBudget.swift").joined(separator: "\n")
        XCTAssertTrue(budget.contains("config.withAllowMesh().encoded()"))
        let probe = try source("Sources/NetworkCoreAppHost/EngineCapabilities.swift").joined(separator: "\n")
        XCTAssertTrue(probe.contains("EmbeddedRuntimeConfig.withAllowMesh("))
        let config = try source("Sources/NetworkCoreBridge/EmbeddedRuntimeConfig.swift").joined(separator: "\n")
        XCTAssertTrue(config.contains("settingAllowMesh(MeshSupport.enabled)"))
        XCTAssertFalse(config.contains("public func settingAllowMesh"))
        XCTAssertFalse(config.contains("public static func settingAllowMesh"))
    }
}
