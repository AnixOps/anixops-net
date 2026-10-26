import Foundation
import NetworkCoreBridge
import XCTest

final class NodeManagementTests: XCTestCase {
    private func node(_ id: String, _ name: String, server: String = "s.example", protocol kind: String = "trojan") -> NativeImportReport.Node {
        NativeImportReport.Node(
            id: id,
            name: name,
            protocol: kind,
            server: server,
            port: 443,
            outbound: ["type": .string(kind), "server": .string(server), "port": .int(443), "password": .string("p")]
        )
    }

    func testEditChangesTheOutboundServerAndPortToo() throws {
        let edited = try NodeEdit.apply(to: node("a", "Tokyo"), name: " Tokyo 2 ", server: "t.example", port: "8443").get()
        XCTAssertEqual(edited.id, "a")
        XCTAssertEqual(edited.name, "Tokyo 2")
        XCTAssertEqual(edited.server, "t.example")
        XCTAssertEqual(edited.port, 8443)
        XCTAssertEqual(edited.outbound["server"], .string("t.example"))
        XCTAssertEqual(edited.outbound["port"], .int(8443))
        XCTAssertEqual(edited.outbound["password"], .string("p"))
    }

    func testEditRejectsBadServersAndPorts() {
        let base = node("a", "Tokyo")
        XCTAssertEqual(NodeEdit.apply(to: base, name: "", server: " ", port: "443"), .failure(.emptyServer))
        XCTAssertEqual(NodeEdit.apply(to: base, name: "", server: "a b", port: "443"), .failure(.invalidServer))
        XCTAssertEqual(NodeEdit.apply(to: base, name: "", server: "a.example", port: "0"), .failure(.invalidPort))
        XCTAssertEqual(NodeEdit.apply(to: base, name: "", server: "a.example", port: "70000"), .failure(.invalidPort))
    }

    func testRemovingMovesTheSelectionAndKeepsNodesTheRulesName() {
        var library = NodeLibrary(
            subscriptionURL: nil,
            nodes: [node("a", "A"), node("b", "B"), node("c", "C")],
            selectedID: "a",
            updatedAt: Date(timeIntervalSince1970: 0)
        )
        let removed = library.removing("a")
        XCTAssertEqual(removed.nodes.map(\.id), ["b", "c"])
        XCTAssertEqual(removed.selectedID, "b")
        XCTAssertFalse(removed.removing("b").removing("c").nodes.isEmpty)

        library.routing = try? JSONDecoder().decode(
            NativeImportReport.Routing.self,
            from: Data(#"{"rules":["DOMAIN,x.example,c","FINAL,G"],"groups":[{"id":"G","type":"select","members":["b"]}]}"#.utf8)
        )
        XCTAssertTrue(library.canRemove("a"))
        XCTAssertFalse(library.canRemove("b"))
        XCTAssertFalse(library.canRemove("c"))
    }

    func testSortByNameAndLatency() {
        let nodes = [node("a", "Tokyo 10"), node("b", "Tokyo 2"), node("c", "Hong Kong")]
        XCTAssertEqual(NodeSort.original.sorted(nodes).map(\.id), ["a", "b", "c"])
        XCTAssertEqual(NodeSort.name.sorted(nodes).map(\.id), ["c", "b", "a"])
        let delays = ["a": 300, "c": 80]
        XCTAssertEqual(NodeSort.latency.sorted(nodes) { delays[$0.id] }.map(\.id), ["c", "a", "b"])
    }

    func testFilterByWordsOrPattern() {
        let nodes = [
            node("a", "香港 01", server: "hk.example"),
            node("b", "Tokyo HK relay", server: "jp.example", protocol: "vless"),
            node("c", "Singapore", server: "sg.example", protocol: "hysteria2"),
        ]
        XCTAssertEqual(NodeFilter("").apply(nodes).map(\.id), ["a", "b", "c"])
        XCTAssertEqual(NodeFilter("hk").apply(nodes).map(\.id), ["a", "b"])
        XCTAssertEqual(NodeFilter("tokyo VLESS").apply(nodes).map(\.id), ["b"])
        XCTAssertEqual(NodeFilter("/香港|sing/").apply(nodes).map(\.id), ["a", "c"])
        XCTAssertNil(NodeFilter("/(/").pattern)
    }
}
