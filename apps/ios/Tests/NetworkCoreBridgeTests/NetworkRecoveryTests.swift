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

    func testTheNetworkRequestCarriesWhatIsKnown() {
        let facts = NetworkFacts(kind: "wifi", router: "192.168.1.1", addresses: ["192.168.1.5", "2001:db8::5"])
        XCTAssertEqual(
            request(.setNetwork(facts)),
            #"{"addresses":["192.168.1.5","2001:db8::5"],"cmd":"set_network","router":"192.168.1.1","type":"wifi"}"#
        )
        XCTAssertEqual(
            request(.setNetwork(NetworkFacts(kind: "cellular", router: nil, addresses: []))),
            #"{"cmd":"set_network","type":"cellular"}"#
        )
        XCTAssertEqual(request(.setNetwork(nil)), #"{"cmd":"set_network"}"#)
    }

    func testTheCellularRadioIsReportedOnlyOnCellular() {
        XCTAssertEqual(
            request(.setNetwork(NetworkFacts(kind: "cellular", router: nil, addresses: [], radio: "NR"))),
            #"{"cmd":"set_network","radio":"NR","type":"cellular"}"#
        )
        XCTAssertNil(NetworkFacts(kind: "wifi", router: nil, addresses: [], radio: "NR").radio)
        XCTAssertEqual(NetworkFacts.radio(telephony: "CTRadioAccessTechnologyNRNSA"), "NRNSA")
        XCTAssertEqual(NetworkFacts.radio(telephony: "CTRadioAccessTechnologyLTE"), "LTE")
        XCTAssertEqual(NetworkFacts.radio(telephony: "CTRadioAccessTechnologyeHRPD"), "EHRPD")
        XCTAssertNil(NetworkFacts.radio(telephony: nil))
        XCTAssertNil(NetworkFacts.radio(telephony: "LTE"))
    }

    func testTheNetworkKeepsAtMostSixteenAddresses() {
        let many = (1...20).map { "10.0.0.\($0)" }
        XCTAssertEqual(NetworkFacts(kind: nil, router: nil, addresses: many).addresses.count, 16)
    }

    func testDecodesTheNetworkAnswer() {
        let answer = RuntimeControl.decode(RuntimeControl.NetworkSet.self, from: Data(#"{"changed":true}"#.utf8))
        XCTAssertEqual(answer, RuntimeControl.NetworkSet(changed: true))
    }

    func testDecodesTheAnswer() {
        let answer = RuntimeControl.decode(RuntimeControl.NetworkReset.self, from: Data(#"{"reset":2,"closed":7}"#.utf8))
        XCTAssertEqual(answer, RuntimeControl.NetworkReset(reset: 2, closed: 7))
    }
}
