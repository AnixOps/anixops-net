import Foundation
import NetworkCoreBridge
import XCTest

final class CaptureCoverageTests: XCTestCase {
    private func connection(
        _ id: UInt64,
        host: String?,
        destination: String,
        network: String = "tcp",
        startedAt: UInt64,
        bytes: (UInt64, UInt64) = (100, 200),
        captured: Bool = true
    ) -> String {
        let hostField = host.map { "\"\($0)\"" } ?? "null"
        return """
        {"id":\(id),"network":"\(network)","host":\(hostField),"destination":"\(destination)","rule":"FINAL",
         "outbound":"DIRECT","started_at_ms":\(startedAt),"ended_at_ms":null,"upload":\(bytes.0),
         "download":\(bytes.1),"error":null,"captured":\(captured)}
        """
    }

    private func connections(_ items: [String]) throws -> [RuntimeControl.Connection] {
        let json = #"{"traffic":{"upload_total":0,"download_total":0,"upload_rate":0,"download_rate":0,"active":0},"active":["#
            + items.joined(separator: ",") + #"],"closed":[]}"#
        return try XCTUnwrap(RuntimeControl.decode(RuntimeControl.Connections.self, from: Data(json.utf8))).active
    }

    func testGroupsCapturedConnectionsWithoutRequestsByHost() throws {
        let list = try connections([
            connection(1, host: "API.Example.com", destination: "api.example.com:443", startedAt: 1_000),
            connection(2, host: "api.example.com", destination: "93.184.216.34:443", network: "udp", startedAt: 3_000),
            connection(3, host: nil, destination: "1.2.3.4:443", startedAt: 2_000, bytes: (1, 1)),
            connection(4, host: nil, destination: "[2001:db8::1]:443", startedAt: 500),
            // Decrypted: a request row came from it.
            connection(5, host: "decrypted.example.com", destination: "decrypted.example.com:443", startedAt: 4_000),
            // Opened before capture started.
            connection(6, host: "old.example.com", destination: "old.example.com:443", startedAt: 5_000, captured: false),
        ])

        let groups = CaptureCoverage.hostsWithoutRequests(list, withRequests: [5])

        XCTAssertEqual(groups.map(\.name), ["api.example.com", "1.2.3.4", "2001:db8::1"])
        let api = groups[0]
        XCTAssertTrue(api.isDomain)
        XCTAssertEqual(api.connections.map(\.id), [2, 1], "newest first")
        XCTAssertEqual(api.bytes, 600)
        XCTAssertEqual(api.networks, ["tcp", "udp"])
        XCTAssertFalse(groups[1].isDomain)
        XCTAssertEqual(groups[1].bytes, 2)
        XCTAssertFalse(groups[2].isDomain)
    }

    func testEveryConnectionWithARequestLeavesNothing() throws {
        let list = try connections([
            connection(1, host: "a.example.com", destination: "a.example.com:443", startedAt: 1),
            connection(2, host: "b.example.com", destination: "b.example.com:80", startedAt: 2),
        ])
        XCTAssertTrue(CaptureCoverage.hostsWithoutRequests(list, withRequests: [1, 2]).isEmpty)
        XCTAssertTrue(CaptureCoverage.hostsWithoutRequests([], withRequests: []).isEmpty)
    }

    func testDecryptionDomainsMatchNamesAndWildcards() {
        let domains = DecryptionDomains(patterns: ["API.example.com", "*.cdn.test", "img-*.photos.test:443", ""])
        XCTAssertTrue(domains.covers("api.example.com"))
        XCTAssertTrue(domains.covers("api.example.com:443"))
        XCTAssertFalse(domains.covers("www.example.com"))
        XCTAssertTrue(domains.covers("a.b.cdn.test"))
        XCTAssertFalse(domains.covers("cdn.test"), "*. needs a label in front")
        XCTAssertTrue(domains.covers("img-3.photos.test"))
        XCTAssertFalse(domains.covers("video.photos.test"))
        XCTAssertFalse(domains.covers("1.2.3.4"), "an address is never decrypted by name")
        XCTAssertTrue(DecryptionDomains(patterns: []).isEmpty)
    }

    func testExclusionsWinOverEveryInclude() throws {
        let domains = DecryptionDomains(patterns: ["*.example.com"], excluded: ["pay.example.com", "*.bank.example.com"])
        XCTAssertTrue(domains.covers("api.example.com"))
        XCTAssertFalse(domains.covers("pay.example.com"))
        XCTAssertFalse(domains.covers("x.bank.example.com"))
        XCTAssertTrue(domains.excludes("x.bank.example.com"))
        XCTAssertFalse(domains.excludes("api.example.com"))
        XCTAssertTrue(DecryptionDomains.matches("img-?.cdn.test", "IMG-1.cdn.test"))

        // A plugin's `-host` excludes as well.
        let summary = MitmPlugin.summarize("[MITM]\nhostname = %APPEND% *.example.com, -pay.example.com, !x.example.com\n")
        XCTAssertEqual(summary.hostnames, ["*.example.com"])
        XCTAssertEqual(summary.excludedHostnames, ["pay.example.com", "x.example.com"])
    }

    func testMitmHostsDecodeAndMarkFailingDomains() throws {
        let json = #"""
        {"enabled":true,"hosts":[
          {"host":"pinned.example.com","decrypted":0,"failed":3,"last_failure":"client_rejected_certificate",
           "last_failure_at_ms":30,"failures":{"client_rejected_certificate":3}},
          {"host":"ok.example.com","decrypted":2,"failed":1,"last_failure":"origin_unreachable",
           "last_failure_at_ms":10,"last_decrypted_at_ms":20,"failures":{"origin_unreachable":1}},
          {"host":"new.example.com","decrypted":0,"failed":1,"last_failure":"something_new","last_failure_at_ms":5}
        ]}
        """#
        XCTAssertEqual(RuntimeControl.mitmHosts.request, Data(#"{"cmd":"mitm_hosts"}"#.utf8))
        XCTAssertEqual(RuntimeControl.clearMitmHosts.request, Data(#"{"cmd":"clear_mitm_hosts"}"#.utf8))
        let answer = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.MitmHosts.self, from: Data(json.utf8)))
        XCTAssertTrue(answer.enabled)
        XCTAssertEqual(answer.failing.map(\.host), ["pinned.example.com", "new.example.com"])
        XCTAssertTrue(answer.hosts[0].looksPinned)
        XCTAssertEqual(answer.hosts[0].failure, .clientRejectedCertificate)
        XCTAssertFalse(answer.hosts[1].failing, "decrypted again after the failure")
        XCTAssertEqual(answer.hosts[2].failure, .other)
        let off = try XCTUnwrap(RuntimeControl.decode(
            RuntimeControl.MitmHosts.self,
            from: Data(#"{"enabled":false,"hosts":[]}"#.utf8)
        ))
        XCTAssertTrue(off.failing.isEmpty)
    }

    func testDecryptionDomainsTakeTheDebugListAndMappings() throws {
        let mappings = try DebugMappings().addingHost("debug.example.com")
        let domains = DecryptionDomains(mappings: mappings, plugins: MitmPluginLibrary())
        XCTAssertEqual(domains.patterns, ["debug.example.com"])
        XCTAssertTrue(domains.covers("debug.example.com"))
        XCTAssertFalse(domains.covers("other.example.com"))
    }

    func testHttpsDecryptionStateRoundTripsAndIgnoresTheRuntimeStatus() throws {
        XCTAssertEqual(TunnelAppMessage.decode(TunnelAppMessage.httpsDecryption.encoded), .httpsDecryption)
        let off = HttpsDecryptionState(status: .off, reason: .rootUnavailable, plugins: 2)
        XCTAssertEqual(HttpsDecryptionState.decoded(from: off.encoded()), off)
        XCTAssertEqual(
            HttpsDecryptionState.decoded(from: Data(#"{"status":"off","reason":"no_root","plugins":1}"#.utf8)),
            HttpsDecryptionState(status: .off, reason: .noRoot, plugins: 1)
        )
        // An older extension answers the runtime status, which is not a decryption state.
        XCTAssertNil(HttpsDecryptionState.decoded(from: Data(#"{"status":"running","outbound":"direct"}"#.utf8)))
        XCTAssertNil(HttpsDecryptionState.decoded(from: nil))
    }
}
