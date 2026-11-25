import Foundation

/// One entry of the kernel's `routing.paths` (networkcore_anixops docs/architecture/mesh-module.md): a member by
/// virtual address or a subnet, and the members to pass, by virtual address, in order.
public struct MeshRoutingPath: Equatable {
    public enum Target: Equatable {
        case peer(String)
        case subnet(String)
    }

    public let target: Target
    public let via: [String]

    public init(target: Target, via: [String]) {
        self.target = target
        self.via = via
    }

    /// The entry as `mesh_set_paths` takes it.
    public var json: [String: Any] {
        switch target {
        case let .peer(address):
            return ["to_peer": address, "via": via]
        case let .subnet(cidr):
            return ["to_subnet": cidr, "via": via]
        }
    }
}

/// Custom paths (docs/mesh-ui.md, 路径; kernel 0.5). The snapshot names members by id; `mesh_set_paths` takes them by
/// virtual address, and the whole list each time. The same as the desktop's `routingPaths` (mesh.ts).
public enum MeshPaths {
    /// The most waypoints a path may name.
    public static let maxWaypoints = 6

    /// A path as the editor holds it: a member id (`isMember`) or a subnet, and the waypoints by member id.
    public struct Draft: Equatable {
        public var isMember: Bool
        public var target: String
        public var via: [String]

        public init(isMember: Bool, target: String, via: [String]) {
            self.isMember = isMember
            self.target = target
            self.via = via
        }
    }

    /// The snapshot's paths: id and draft.
    public static func current(status: [String: Any]?) -> [(id: String, draft: Draft)] {
        let paths = status?["paths"] as? [[String: Any]] ?? []
        return paths.map { path in
            let target = path["target"] as? [String: Any] ?? [:]
            let via = (path["via"] as? [Any] ?? []).compactMap { $0 as? String }
            let id = path["id"] as? String ?? ""
            if let peer = target["peer"] as? String {
                return (id, Draft(isMember: true, target: peer, via: via))
            }
            return (id, Draft(isMember: false, target: target["subnet"] as? String ?? "", via: via))
        }
    }

    /// The list for `mesh_set_paths` after removing the path `remove` (an id) and adding `add`, which replaces a path
    /// to the same target. Nil when a member has no virtual address in the snapshot, or `add` names no waypoint or
    /// too many.
    public static func routing(status: [String: Any]?, remove: String? = nil, add: Draft? = nil) -> [MeshRoutingPath]? {
        let known = addresses(status)
        var drafts = current(status: status).filter { $0.id != remove }.map(\.draft)
        if let add {
            if add.target.isEmpty || add.via.isEmpty || add.via.count > maxWaypoints {
                return nil
            }
            drafts.removeAll { $0.isMember == add.isMember && $0.target == add.target }
            drafts.append(add)
        }
        var result: [MeshRoutingPath] = []
        for draft in drafts {
            let via = draft.via.compactMap { known[$0] }
            guard via.count == draft.via.count else {
                return nil
            }
            if draft.isMember {
                guard let address = known[draft.target] else {
                    return nil
                }
                result.append(MeshRoutingPath(target: .peer(address), via: via))
            } else {
                result.append(MeshRoutingPath(target: .subnet(draft.target), via: via))
            }
        }
        return result
    }

    /// Member id to virtual address without its prefix, this device included.
    private static func addresses(_ status: [String: Any]?) -> [String: String] {
        var known: [String: String] = [:]
        let rows = [status?["self"] as? [String: Any]].compactMap { $0 } + (status?["members"] as? [[String: Any]] ?? [])
        for row in rows {
            if let id = row["id"] as? String,
               let address = (row["address"] as? String)?.split(separator: "/").first, !address.isEmpty
            {
                known[id] = String(address)
            }
        }
        return known
    }
}
