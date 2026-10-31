import Foundation
import NetworkCoreBridge
import XCTest

final class LatencyGradeTests: XCTestCase {
    func testThresholds() {
        XCTAssertEqual(LatencyGrade(milliseconds: nil), .unknown)
        XCTAssertEqual(LatencyGrade(milliseconds: 0), .good)
        XCTAssertEqual(LatencyGrade(milliseconds: 119), .good)
        XCTAssertEqual(LatencyGrade(milliseconds: 120), .fair)
        XCTAssertEqual(LatencyGrade(milliseconds: 249), .fair)
        XCTAssertEqual(LatencyGrade(milliseconds: 250), .poor)
    }

    func testStripFollowsTheGroupsMemberOrder() throws {
        let json = """
        {"groups":[{"id":"Proxy","type":"select","now":"jp-1","members":[
          {"name":"hk-1","latency_ms":48},{"name":"jp-1","latency_ms":180},{"name":"us-1","latency_ms":null},
          {"name":"DIRECT"}]}]}
        """
        let answer = try XCTUnwrap(RuntimeControl.decode(RuntimeControl.Groups.self, from: Data(json.utf8)))
        let group = try XCTUnwrap(answer.groups.first)
        let strip = LatencyGrade.strip(members: ["jp-1", "hk-1", "us-1", "DIRECT", "Auto"], latencies: group.members)
        XCTAssertEqual(strip, [.fair, .good, .unknown, .unknown, .unknown])
        XCTAssertEqual(LatencyGrade.reachable(strip), 2)
    }
}
