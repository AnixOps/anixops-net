import NetworkCoreBridge
import XCTest

final class NetworkRecoveryTests: XCTestCase {
    private let home = NetworkPathKey(interface: "en0", gateways: ["192.168.1.1"])
    private let office = NetworkPathKey(interface: "en0", gateways: ["10.0.0.1"])
    private let cellular = NetworkPathKey(interface: "pdp_ip0", gateways: [])

    private func request(_ control: RuntimeControl?) -> String? {
        control.map { String(decoding: $0.request, as: UTF8.self) }
    }

    func testTheFirstPathAndRepeatsResetNothing() {
        var recovery = NetworkRecovery()
        XCTAssertNil(recovery.pathChanged(to: home))
        XCTAssertNil(recovery.pathChanged(to: home))
        XCTAssertNil(recovery.pathChanged(to: NetworkPathKey(interface: "en0", gateways: ["192.168.1.1"])))
    }

    func testAnotherNetworkResetsEverything() {
        var recovery = NetworkRecovery()
        _ = recovery.pathChanged(to: home)
        XCTAssertEqual(request(recovery.pathChanged(to: cellular)), #"{"close_connections":true,"cmd":"reset_network"}"#)
        XCTAssertNotNil(recovery.pathChanged(to: office))
        XCTAssertNil(recovery.pathChanged(to: office))
    }

    func testBackOnlineResetsOnceTheNetworkIsUp() {
        var recovery = NetworkRecovery()
        _ = recovery.pathChanged(to: home)
        XCTAssertNil(recovery.pathChanged(to: nil))
        XCTAssertNil(recovery.pathChanged(to: nil))
        XCTAssertNotNil(recovery.pathChanged(to: home))
    }

    func testStartingOfflineResetsWhenTheNetworkComes() {
        var recovery = NetworkRecovery()
        XCTAssertNil(recovery.pathChanged(to: nil))
        XCTAssertNotNil(recovery.pathChanged(to: home))
    }

    func testGatewayOrderDoesNotMatter() {
        XCTAssertEqual(
            NetworkPathKey(interface: "en0", gateways: ["a", "b"]),
            NetworkPathKey(interface: "en0", gateways: ["b", "a"])
        )
    }

    func testWakeClosesConnectionsOnlyAfterALongSleep() {
        XCTAssertEqual(
            request(NetworkRecovery.afterWake(sleptFor: 5)), #"{"close_connections":false,"cmd":"reset_network"}"#
        )
        XCTAssertEqual(
            request(NetworkRecovery.afterWake(sleptFor: NetworkRecovery.closeAfterSleep)),
            #"{"close_connections":true,"cmd":"reset_network"}"#
        )
    }

    /// Twenty network changes in a row each ask for one reset.
    func testTwentyCyclesResetEveryTime() {
        var recovery = NetworkRecovery()
        _ = recovery.pathChanged(to: home)
        var resets = 0
        for cycle in 0..<20 {
            if recovery.pathChanged(to: cycle.isMultiple(of: 2) ? cellular : home) != nil {
                resets += 1
            }
        }
        XCTAssertEqual(resets, 20)
    }

    func testDecodesTheAnswer() {
        let answer = RuntimeControl.decode(RuntimeControl.NetworkReset.self, from: Data(#"{"reset":2,"closed":7}"#.utf8))
        XCTAssertEqual(answer, RuntimeControl.NetworkReset(reset: 2, closed: 7))
    }
}
