import Foundation
import NetworkCoreBridge
import XCTest

final class NodeManagementTests: XCTestCase {
    private func node(
        _ id: String,
        _ name: String,
        server: String = "s.example",
        protocol kind: String = "trojan",
        via: String? = nil
    ) -> NativeImportReport.Node {
        let link = via.map { ",\"via\":\"\($0)\"" } ?? ""
        let json = """
        {"id":"\(id)","name":"\(name)","protocol":"\(kind)","server":"\(server)","port":443,
         "outbound":{"type":"\(kind)","server":"\(server)","port":443,"password":"p"\(link)}}
        """
        return try! JSONDecoder().decode(NativeImportReport.Node.self, from: Data(json.utf8))
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

    func testShareRequestCarriesTheNameAndFullOutbound() throws {
        let request = try node("a", "Tokyo").shareLinkRequest()
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: request) as? [String: Any])
        XCTAssertEqual(Set(json.keys), ["name", "outbound"])
        XCTAssertEqual(json["name"] as? String, "Tokyo")
        let outbound = try XCTUnwrap(json["outbound"] as? [String: Any])
        XCTAssertEqual(outbound["password"] as? String, "p")
        XCTAssertEqual(outbound["port"] as? Int, 443)
        // A node without a name is shared under its server.
        let unnamed = try XCTUnwrap(JSONSerialization.jsonObject(with: node("b", "").shareLinkRequest()) as? [String: Any])
        XCTAssertEqual(unnamed["name"] as? String, "s.example")
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

    /// A node another node connects through (`via`) stays while that node does: deleting it would leave the other
    /// node's `via` naming nothing, and the engine would refuse the rules and that node with them.
    func testNodesOthersConnectThroughCannotBeRemoved() {
        var library = NodeLibrary(
            subscriptionURL: nil,
            nodes: [node("line", "Line", protocol: "iwan"), node("land", "Land", via: "line"), node("solo", "Solo")],
            selectedID: "line",
            updatedAt: Date(timeIntervalSince1970: 0)
        )
        XCTAssertEqual(library.node(id: "land")?.via, "line")
        XCTAssertFalse(library.canRemove("line"))
        XCTAssertTrue(library.canRemove("land"))
        XCTAssertTrue(library.canRemove("solo"))
        XCTAssertEqual(library.removing("line"), library)
        // Once nothing connects through it, it can go.
        XCTAssertTrue(library.removing("land").canRemove("line"))

        // Subscription rules and groups add to that, they do not lift it.
        library.routing = try? JSONDecoder().decode(
            NativeImportReport.Routing.self,
            from: Data(#"{"rules":["FINAL,G"],"groups":[{"id":"G","type":"select","members":["land"]}]}"#.utf8)
        )
        XCTAssertNotNil(library.routing)
        XCTAssertFalse(library.canRemove("line"))
        XCTAssertFalse(library.canRemove("land"))
        XCTAssertTrue(library.canRemove("solo"))

        // Along a chain only the node nothing connects through can go; a node naming itself does not hold itself.
        let chain = NodeLibrary(
            subscriptionURL: nil,
            nodes: [node("a", "A", via: "b"), node("b", "B", via: "c"), node("c", "C"), node("loop", "Loop", via: "loop")],
            selectedID: "a",
            updatedAt: Date(timeIntervalSince1970: 0)
        )
        XCTAssertTrue(chain.canRemove("a"))
        XCTAssertFalse(chain.canRemove("b"))
        XCTAssertFalse(chain.canRemove("c"))
        XCTAssertTrue(chain.canRemove("loop"))
    }

    /// Deleting a node others connect through asks first and takes them along, so no `via` is left naming nothing.
    func testRemovalTakesTheNodesThatConnectThroughIt() {
        var library = NodeLibrary(
            subscriptionURL: nil,
            nodes: [node("a", "A", via: "b"), node("b", "B", via: "c"), node("c", "C"), node("solo", "Solo"),
                    node("loop", "Loop", via: "loop")],
            selectedID: "a",
            updatedAt: Date(timeIntervalSince1970: 0)
        )
        XCTAssertEqual(library.dependents(of: "c").map(\.id), ["a", "b"])
        XCTAssertEqual(library.dependents(of: "b").map(\.id), ["a"])
        XCTAssertEqual(library.removal(of: "c")?.map(\.id), ["a", "b"])
        XCTAssertEqual(library.removal(of: "solo")?.map(\.id), [])
        XCTAssertEqual(library.removal(of: "loop")?.map(\.id), [])
        XCTAssertNil(library.removal(of: "missing"))

        let removed = library.removingWithDependents("c")
        XCTAssertEqual(removed.nodes.map(\.id), ["solo", "loop"])
        let remaining = Set(removed.nodes.map(\.id))
        XCTAssertTrue(removed.nodes.allSatisfy { node in node.via.map { remaining.contains($0) } ?? true })
        // The selected node went with it: the selection moves to a node that remains.
        XCTAssertNotNil(removed.selectedID.flatMap(removed.node(id:)))

        // Nothing may go that the rules name, the dependents included.
        library.routing = try? JSONDecoder().decode(
            NativeImportReport.Routing.self,
            from: Data(#"{"rules":["FINAL,G"],"groups":[{"id":"G","type":"select","members":["a","solo"]}]}"#.utf8)
        )
        XCTAssertNil(library.removal(of: "c"))
        XCTAssertEqual(library.removingWithDependents("c"), library)

        // One node always remains.
        let pair = NodeLibrary(
            subscriptionURL: nil,
            nodes: [node("x", "X", via: "y"), node("y", "Y")],
            selectedID: "x",
            updatedAt: Date(timeIntervalSince1970: 0)
        )
        XCTAssertNil(pair.removal(of: "y"))
        XCTAssertEqual(pair.removal(of: "x")?.map(\.id), [])
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
