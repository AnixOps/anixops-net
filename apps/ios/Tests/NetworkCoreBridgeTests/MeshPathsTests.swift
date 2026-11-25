import NetworkCoreBridge
import XCTest

/// Custom paths: the snapshot by member id, `mesh_set_paths` by virtual address (the desktop's mesh.test.ts too).
final class MeshPathsTests: XCTestCase {
    private let status: [String: Any] = [
        "self": ["id": "me", "address": "10.144.0.2/16"],
        "members": [
            ["id": "a", "address": "10.144.0.1/16"],
            ["id": "b", "address": "10.144.1.1/16"],
            ["id": "c"],
        ],
        "paths": [
            ["id": "p1", "target": ["subnet": "172.20.0.0/16"], "via": ["a", "b"], "state": "active"],
            ["id": "p2", "target": ["peer": "b"], "via": ["a"], "state": "fallback"],
        ],
    ]

    func testReadsTheSnapshotsPaths() {
        let paths = MeshPaths.current(status: status)
        XCTAssertEqual(paths.map(\.id), ["p1", "p2"])
        XCTAssertEqual(paths[0].draft, MeshPaths.Draft(isMember: false, target: "172.20.0.0/16", via: ["a", "b"]))
        XCTAssertEqual(paths[1].draft, MeshPaths.Draft(isMember: true, target: "b", via: ["a"]))
    }

    func testSendsTheWholeListByAddress() {
        XCTAssertEqual(
            MeshPaths.routing(status: status, remove: "p1"),
            [MeshRoutingPath(target: .peer("10.144.1.1"), via: ["10.144.0.1"])]
        )
        XCTAssertEqual(
            MeshPaths.routing(status: status, add: MeshPaths.Draft(isMember: false, target: "192.168.9.0/24", via: ["b"])),
            [
                MeshRoutingPath(target: .subnet("172.20.0.0/16"), via: ["10.144.0.1", "10.144.1.1"]),
                MeshRoutingPath(target: .peer("10.144.1.1"), via: ["10.144.0.1"]),
                MeshRoutingPath(target: .subnet("192.168.9.0/24"), via: ["10.144.1.1"]),
            ]
        )
    }

    func testReplacesTheSameTargetAndRefusesWhatCannotBeSent() {
        XCTAssertEqual(
            MeshPaths.routing(status: status, add: MeshPaths.Draft(isMember: true, target: "b", via: ["me"])),
            [
                MeshRoutingPath(target: .subnet("172.20.0.0/16"), via: ["10.144.0.1", "10.144.1.1"]),
                MeshRoutingPath(target: .peer("10.144.1.1"), via: ["10.144.0.2"]),
            ]
        )
        XCTAssertNil(MeshPaths.routing(status: status, add: MeshPaths.Draft(isMember: true, target: "c", via: ["a"])))
        XCTAssertNil(MeshPaths.routing(status: status, add: MeshPaths.Draft(isMember: false, target: "10.9.0.0/16", via: [])))
        XCTAssertNil(MeshPaths.routing(
            status: status,
            add: MeshPaths.Draft(isMember: false, target: "10.9.0.0/16", via: Array(repeating: "a", count: 7))
        ))
    }
}
