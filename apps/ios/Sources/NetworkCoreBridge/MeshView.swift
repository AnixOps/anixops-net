import Foundation
#if canImport(Darwin)
import Darwin
#else
import Glibc
#endif

/// What the mesh page (组网) makes of the kernel's status snapshot: its entry, member list, subnet list,
/// path view and notices.
///
/// The same rules as `crates/client-view/src/mesh.rs` (the `view.mesh_*` commands the other platforms
/// use), written for the Apple apps, which do not use the Rust client service. `MeshViewTests` runs the
/// shared samples in `crates/client-view/fixtures/cases.json` against this file, so the two cannot drift.
///
/// Values are JSON as `JSONSerialization` gives and takes it (`[String: Any]`, `[Any]`, `NSNull`).
/// Answers carry ids and values, not wording.
public enum MeshView {
    /// Children a parent row shows before "show all" (N31).
    public static let foldedChildren = 5

    // MARK: Reading the snapshot

    static func text(_ value: Any?, _ name: String) -> String {
        ((value as? [String: Any])?[name] as? String) ?? ""
    }

    static func optText(_ value: Any?, _ name: String) -> String? {
        let text = text(value, name)
        return text.isEmpty ? nil : text
    }

    static func list(_ value: Any?, _ name: String) -> [Any] {
        ((value as? [String: Any])?[name] as? [Any]) ?? []
    }

    static func unsigned(_ value: Any?) -> Int? {
        guard let number = value as? NSNumber, !isBool(number) else { return nil }
        let double = number.doubleValue
        guard double >= 0, double == double.rounded(), double < 9.0e18 else { return nil }
        return number.intValue
    }

    static func isBool(_ number: NSNumber) -> Bool {
        CFGetTypeID(number) == CFBooleanGetTypeID()
    }

    static func bool(_ value: Any?, _ name: String) -> Bool? {
        guard let number = (value as? [String: Any])?[name] as? NSNumber, isBool(number) else { return nil }
        return number.boolValue
    }

    static func latency(_ value: Any?) -> Int? {
        unsigned((value as? [String: Any])?["latencyMs"])
    }

    static func orNull(_ value: Any?) -> Any {
        value ?? NSNull()
    }

    /// Text in the list's order: without regard to case, then as written.
    static func compareText(_ a: String, _ b: String) -> ComparisonResult {
        let (la, lb) = (a.lowercased(), b.lowercased())
        if la != lb {
            return la.unicodeScalars.lexicographicallyPrecedes(lb.unicodeScalars) ? .orderedAscending : .orderedDescending
        }
        if a == b { return .orderedSame }
        return a.unicodeScalars.lexicographicallyPrecedes(b.unicodeScalars) ? .orderedAscending : .orderedDescending
    }

    // MARK: Latency

    /// `fast` under 120 ms, `medium` under 250 ms, `slow` from there, `timeout` without an answer.
    public static func latencyTone(_ milliseconds: Int?) -> String {
        guard let milliseconds else { return "timeout" }
        if milliseconds < 120 { return "fast" }
        if milliseconds < 250 { return "medium" }
        return "slow"
    }

    // MARK: Addresses

    /// An IPv4 or IPv6 address as its family and bytes.
    struct Address: Comparable {
        let v6: Bool
        let bytes: [UInt8]

        init?(_ text: String) {
            var v4 = in_addr()
            var v6Address = in6_addr()
            if text.withCString({ inet_pton(AF_INET, $0, &v4) }) == 1 {
                v6 = false
                bytes = withUnsafeBytes(of: &v4) { Array($0) }
            } else if text.withCString({ inet_pton(AF_INET6, $0, &v6Address) }) == 1 {
                v6 = true
                bytes = withUnsafeBytes(of: &v6Address) { Array($0) }
            } else {
                return nil
            }
        }

        static func < (lhs: Address, rhs: Address) -> Bool {
            if lhs.v6 != rhs.v6 { return !lhs.v6 }
            return lhs.bytes.lexicographicallyPrecedes(rhs.bytes)
        }
    }

    /// A network in CIDR form, or a bare address as a one-address network.
    struct Cidr {
        let address: Address
        let prefix: Int

        init?(_ text: String) {
            let text = text.trimmingCharacters(in: .whitespaces)
            let parts = text.split(separator: "/", maxSplits: 1, omittingEmptySubsequences: false)
            guard let address = Address(String(parts[0])) else { return nil }
            let width = address.v6 ? 128 : 32
            if parts.count == 2 {
                guard let prefix = Int(parts[1]), parts[1].allSatisfy({ $0.isASCII && $0.isNumber }), prefix <= width else { return nil }
                self.prefix = prefix
            } else {
                prefix = width
            }
            self.address = address
        }

        func contains(_ other: Address) -> Bool {
            guard other.v6 == address.v6 else { return false }
            var remaining = prefix
            for (mine, theirs) in zip(address.bytes, other.bytes) {
                if remaining <= 0 { return true }
                let bits = min(remaining, 8)
                let mask = UInt8(truncatingIfNeeded: 0xFF << (8 - bits))
                if mine & mask != theirs & mask { return false }
                remaining -= bits
            }
            return true
        }
    }

    static func bareAddress(_ address: String) -> String {
        String(address.split(separator: "/", maxSplits: 1, omittingEmptySubsequences: false).first ?? "")
    }

    /// What follows the host name in an internal domain name.
    static func regionDomain(_ domain: String) -> String? {
        guard let dot = domain.firstIndex(of: ".") else { return nil }
        let rest = String(domain[domain.index(after: dot)...])
        return rest.isEmpty ? nil : rest
    }

    // MARK: Members

    static let roles = ["border", "gateway", "host", "server", "client"]

    static func roleRank(_ role: String) -> Int {
        roles.firstIndex(of: role) ?? roles.count
    }

    /// The snapshot's members with this device first, marked `self`.
    static func allMembers(_ status: Any?) -> [[String: Any]] {
        var members: [[String: Any]] = []
        if var own = (status as? [String: Any])?["self"] as? [String: Any] {
            own["self"] = true
            own["state"] = "online"
            own["connection"] = "self"
            members.append(own)
        }
        members.append(contentsOf: list(status, "members").map { ($0 as? [String: Any]) ?? [:] })
        return members
    }

    static func memberName(_ members: [[String: Any]], _ id: String) -> String {
        guard let member = members.first(where: { text($0, "id") == id }) else { return id }
        return optText(member, "name") ?? text(member, "id")
    }

    static func regionName(_ status: Any?, _ code: String) -> String {
        list(status, "regions").first(where: { text($0, "code") == code }).flatMap { optText($0, "name") } ?? code
    }

    static func localRegion(_ status: Any?) -> String? {
        if let region = list(status, "regions").first(where: { bool($0, "local") == true }) {
            return text(region, "code")
        }
        return optText((status as? [String: Any])?["self"], "region")
    }

    static func children(_ member: [String: Any], devices: [Any]) -> [[String: Any]] {
        var children: [[String: Any]] = []
        let subnets = list(member, "subnets").compactMap { $0 as? String }
        for subnet in subnets {
            children.append(["kind": "subnet", "name": subnet])
        }
        if !subnets.isEmpty {
            let networks = subnets.compactMap(Cidr.init)
            for device in devices {
                let inside = Address(bareAddress(text(device, "address"))).map { address in
                    networks.contains { $0.contains(address) }
                } ?? false
                if inside {
                    children.append([
                        "kind": "device",
                        "name": text(device, "name"),
                        "address": text(device, "address"),
                    ])
                }
            }
        }
        for vm in list(member, "vms") {
            let object = vm as? [String: Any]
            children.append([
                "kind": "vm",
                "name": text(vm, "name"),
                "address": orNull(object?["address"]),
                "domain": orNull(object?["domain"]),
            ])
        }
        return children
    }

    static func childMatches(_ child: [String: Any], query: String, address: Address?) -> Bool {
        if let address, text(child, "kind") == "subnet",
           Cidr(text(child, "name"))?.contains(address) == true {
            return true
        }
        return ["name", "address", "domain"].contains { text(child, $0).lowercased().contains(query) }
    }

    static func memberMatches(_ member: [String: Any], query: String) -> Bool {
        ["name", "domain", "address", "addressV6"].contains { text(member, $0).lowercased().contains(query) }
    }

    static func passes(_ member: [String: Any], filter: String) -> Bool {
        switch filter {
        case "online", "unreachable": return text(member, "state") == filter
        case "gateway", "host", "border": return text(member, "role") == filter
        case "custom_path": return ["custom", "fallback", "first_hop_only"].contains(text(member, "pathState"))
        default: return true
        }
    }

    static func marks(_ member: [String: Any]) -> [String] {
        var marks: [String] = []
        if !list(member, "duplicateOf").isEmpty { marks.append("duplicate") }
        let infrastructure = ["border", "gateway", "host", "server"].contains(text(member, "role"))
        if infrastructure && bool(member, "addressManaged") == false { marks.append("unmanaged") }
        switch text(member, "pathState") {
        case "fallback": marks.append("fallback")
        case "first_hop_only": marks.append("first_hop_only")
        default: break
        }
        return marks
    }

    static func ascending(_ a: [String: Any], _ b: [String: Any], sort: String) -> ComparisonResult {
        let ownA = bool(a, "self") == true, ownB = bool(b, "self") == true
        if ownA != ownB { return ownA ? .orderedAscending : .orderedDescending }
        switch sort {
        case "latency":
            let x = latency(a) ?? Int.max, y = latency(b) ?? Int.max
            return x == y ? .orderedSame : (x < y ? .orderedAscending : .orderedDescending)
        case "address":
            switch (Address(bareAddress(text(a, "address"))), Address(bareAddress(text(b, "address")))) {
            case let (x?, y?): return x == y ? .orderedSame : (x < y ? .orderedAscending : .orderedDescending)
            case (.some, nil): return .orderedAscending
            case (nil, .some): return .orderedDescending
            case (nil, nil): return .orderedSame
            }
        default:
            return compareText(text(a, "name"), text(b, "name"))
        }
    }

    /// A stable sort, like the Rust side's.
    static func stableSorted<T>(_ items: [T], by compare: (T, T) -> ComparisonResult) -> [T] {
        items.enumerated().sorted { lhs, rhs in
            switch compare(lhs.element, rhs.element) {
            case .orderedAscending: return true
            case .orderedDescending: return false
            case .orderedSame: return lhs.offset < rhs.offset
            }
        }.map(\.element)
    }

    static func memberRow(_ member: [String: Any], devices: [Any], query: String, address: Address?) -> [String: Any]? {
        let kids = children(member, devices: devices)
        let ownMatch = query.isEmpty || memberMatches(member, query: query)
        let matched = kids.map { !query.isEmpty && childMatches($0, query: query, address: address) }
        let childMatch = matched.contains(true)
        if !ownMatch && !childMatch { return nil }
        let visible: Int
        if let last = matched.lastIndex(of: true), last >= foldedChildren {
            visible = kids.count
        } else {
            visible = min(kids.count, foldedChildren)
        }
        let contains: Any
        if kids.isEmpty {
            contains = NSNull()
        } else if text(member, "role") == "host" {
            contains = ["noun": "vms", "count": list(member, "vms").count] as [String: Any]
        } else {
            contains = ["noun": "subnets", "count": list(member, "subnets").count] as [String: Any]
        }
        let rows: [[String: Any]] = zip(kids, matched).map { child, matched in
            var child = child
            child["match"] = matched
            return child
        }
        let tone: Any = text(member, "connection") == "self" ? NSNull() as Any : latencyTone(latency(member)) as Any
        return [
            "id": text(member, "id"),
            "name": text(member, "name"),
            "self": bool(member, "self") ?? false,
            "role": orNull(optText(member, "role")),
            "region": orNull(optText(member, "region")),
            "state": text(member, "state"),
            "address": orNull(optText(member, "address")),
            "domain": orNull(optText(member, "domain")),
            "regionDomain": orNull(regionDomain(text(member, "domain"))),
            "connection": optText(member, "connection") ?? "unreachable",
            "hops": unsigned(member["hops"]) ?? 0,
            "transport": orNull(optText(member, "transport")),
            "latencyMs": orNull(latency(member)),
            "tone": tone,
            "contains": contains,
            "children": rows,
            "visibleChildren": visible,
            "open": childMatch,
            "marks": marks(member),
        ]
    }

    static func worst(_ rows: [[String: Any]]) -> Any {
        let unreachable = rows.filter { text($0, "state") == "unreachable" }.count
        if unreachable > 0 { return ["state": "unreachable", "count": unreachable] as [String: Any] }
        let fallback = rows.filter { text($0, "pathState") == "fallback" }.count
        if fallback > 0 { return ["state": "fallback", "count": fallback] as [String: Any] }
        return NSNull()
    }

    /// The member list, grouped (N28, N29). See `mesh::members` for the arguments and the answer.
    public static func members(
        status: Any?,
        groupBy requested: String,
        filter: String,
        sort: String,
        query rawQuery: String,
        devices: [Any]
    ) -> [String: Any] {
        let query = rawQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let address = Address(query)
        let narrowed = !query.isEmpty || !(filter.isEmpty || filter == "all")
        let regions = list(status, "regions")
        let groupBy: String
        if requested == "role" {
            groupBy = "role"
        } else if (requested == "region" || requested.isEmpty) && !regions.isEmpty {
            groupBy = "region"
        } else {
            groupBy = "none"
        }
        let local = localRegion(status)
        let everyone = stableSorted(allMembers(status)) { ascending($0, $1, sort: sort) }

        func key(_ member: [String: Any]) -> String? {
            switch groupBy {
            case "region": return text(member, "region")
            case "role": return text(member, "role")
            default: return nil
            }
        }
        var keys: [String?] = []
        for member in everyone where !keys.contains(key(member)) {
            keys.append(key(member))
        }
        switch groupBy {
        case "region":
            keys = stableSorted(keys) { a, b in
                func rank(_ key: String?) -> (Int, Int) {
                    let code = key ?? ""
                    let at = regions.firstIndex { text($0, "code") == code } ?? regions.count
                    return (code == local ? 0 : 1, at)
                }
                let (x, y) = (rank(a), rank(b))
                return x == y ? .orderedSame : (x < y ? .orderedAscending : .orderedDescending)
            }
        case "role":
            keys = stableSorted(keys) { a, b in
                let (x, y) = (roleRank(a ?? ""), roleRank(b ?? ""))
                return x == y ? .orderedSame : (x < y ? .orderedAscending : .orderedDescending)
            }
        default:
            break
        }

        var matchedTotal = 0
        var groups: [[String: Any]] = []
        for groupKey in keys {
            let inGroup = everyone.filter { key($0) == groupKey }
            let rows = inGroup
                .filter { passes($0, filter: filter) }
                .compactMap { memberRow($0, devices: devices, query: query, address: address) }
            if narrowed && rows.isEmpty { continue }
            matchedTotal += rows.count
            let attention = worst(inGroup)
            let isLocal = groupBy == "region" && groupKey == local
            var link: Any = NSNull()
            if groupBy == "region", let code = groupKey, let local, code != local,
               let found = list(status, "links").first(where: {
                   (text($0, "from") == local && text($0, "to") == code)
                       || (text($0, "from") == code && text($0, "to") == local)
               }) {
                link = [
                    "latencyMs": orNull(latency(found)),
                    "via": orNull((found as? [String: Any])?["via"]),
                    "state": text(found, "state"),
                ] as [String: Any]
            }
            let open = groupBy == "none" || isLocal || !(attention is NSNull) || narrowed
            groups.append([
                "key": orNull(groupKey),
                "count": inGroup.count,
                "matched": rows.count,
                "open": open,
                "worst": attention,
                "link": link,
                "rows": rows,
            ])
        }
        return ["groupBy": groupBy, "groups": groups, "total": everyone.count, "matched": matchedTotal]
    }

    // MARK: Subnets

    /// The subnet list. See `mesh::subnets`.
    public static func subnets(status: Any?, query rawQuery: String) -> [String: Any] {
        let query = rawQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let address = Address(query)
        let members = allMembers(status)
        var rows: [[String: Any]] = []
        for subnet in list(status, "subnets") {
            let cidr = text(subnet, "cidr")
            let advertisers: [[String: Any]] = list(subnet, "advertisers").map { advertiser in
                let id = text(advertiser, "member")
                return ["id": id, "name": memberName(members, id), "inUse": bool(advertiser, "inUse") ?? false]
            }
            let matches = query.isEmpty
                || cidr.lowercased().contains(query)
                || (address.map { Cidr(cidr)?.contains($0) == true } ?? false)
                || advertisers.contains { text($0, "name").lowercased().contains(query) }
            if !matches { continue }
            let inUse: Any = (advertisers.first { bool($0, "inUse") == true } ?? advertisers.first)?["name"] ?? NSNull()
            let state = optText(subnet, "state") ?? "ok"
            let route = optText(subnet, "route") ?? "mesh"
            let overlaps = list(subnet, "overlaps")
            var marks: [String] = []
            if advertisers.count > 1 { marks.append("multi_source") }
            if !overlaps.isEmpty { marks.append("overlap") }
            if bool(subnet, "localOverlap") == true { marks.append("local_overlap") }
            if state == "refused" { marks.append("refused") }
            let path = list(subnet, "path").compactMap { $0 as? String }.map { memberName(members, $0) }
            let region = optText(subnet, "region")
            let tone: Any = state == "refused" ? NSNull() as Any : latencyTone(latency(subnet)) as Any
            rows.append([
                "cidr": cidr,
                "region": orNull(region),
                "regionName": orNull(region.map { regionName(status, $0) }),
                "advertiser": inUse,
                "advertisers": advertisers,
                "path": path,
                "latencyMs": orNull(latency(subnet)),
                "tone": tone,
                "state": state,
                "route": route,
                "overlaps": overlaps,
                "marks": marks,
            ])
        }
        rows = stableSorted(rows) { a, b in
            let (x, y) = (text(a, "state") == "refused", text(b, "state") == "refused")
            return x == y ? .orderedSame : (y ? .orderedAscending : .orderedDescending)
        }
        let refused = unsigned((status as? [String: Any])?["refusedAdvertisements"]) ?? 0
        return ["rows": rows, "refused": refused]
    }

    // MARK: Paths

    /// The path view. See `mesh::paths`.
    public static func paths(status: Any?) -> [String: Any] {
        let object = status as? [String: Any]
        let members = allMembers(status)
        func names(_ ids: [Any]) -> [String] {
            ids.compactMap { $0 as? String }.map { memberName(members, $0) }
        }
        let links: Any = (object?["links"] as? [Any]).map { links in
            links.map { link -> [String: Any] in
                let via = optText(link, "via")
                return [
                    "from": text(link, "from"),
                    "fromName": regionName(status, text(link, "from")),
                    "to": text(link, "to"),
                    "toName": regionName(status, text(link, "to")),
                    "via": orNull(via),
                    "viaName": orNull(via.map { regionName(status, $0) }),
                    "borders": names(list(link, "borders")),
                    "latencyMs": orNull(latency(link)),
                    "tone": latencyTone(latency(link)),
                    "loss": orNull(((link as? [String: Any])?["loss"] as? NSNumber).flatMap { isBool($0) ? nil : $0.doubleValue }),
                    "state": optText(link, "state") ?? "ok",
                ]
            }
        } ?? NSNull()
        let custom: Any = (object?["paths"] as? [Any]).map { paths in
            paths.map { path -> [String: Any] in
                let target = (path as? [String: Any])?["target"]
                let kind: String, value: String, label: String
                var advertiser: Any = NSNull()
                if let peer = optText(target, "peer") {
                    (kind, value, label) = ("member", peer, memberName(members, peer))
                } else {
                    let subnet = text(target, "subnet")
                    (kind, value, label) = ("subnet", subnet, subnet)
                    if let known = list(status, "subnets").first(where: { text($0, "cidr") == subnet }) {
                        let all = list(known, "advertisers")
                        if let chosen = all.first(where: { bool($0, "inUse") == true }) ?? all.first {
                            advertiser = memberName(members, text(chosen, "member"))
                        }
                    }
                }
                return [
                    "id": text(path, "id"),
                    "targetKind": kind,
                    "target": value,
                    "targetLabel": label,
                    "advertiser": advertiser,
                    "via": names(list(path, "via")),
                    "state": optText(path, "state") ?? "active",
                    "reason": orNull(optText(path, "reason")),
                    "unreachable": orNull(optText(path, "unreachable").map { memberName(members, $0) }),
                ]
            }
        } ?? NSNull()
        return [
            "policy": optText(object?["network"], "routingPolicy") ?? "latency",
            "links": links,
            "custom": custom,
        ]
    }

    // MARK: Notices

    /// The banners of the page, in the order of `mesh-ui.md` (提示). See `mesh::notices`.
    public static func notices(status: Any?) -> [[String: Any]] {
        let members = allMembers(status)
        let network = (status as? [String: Any])?["network"]
        var notices: [[String: Any]] = []
        func push(_ id: String, _ key: String, _ values: [Any], _ action: String?, _ notify: Bool) {
            notices.append(["id": id, "key": key, "values": values, "action": orNull(action), "notify": notify])
        }
        // `network.failure` stands on its own (mesh-status.md): `unreachable_peers` and `peer_not_native` leave the
        // state as it is, `native_keys_pending` comes with `not_joined`. A failed network without one is `other`.
        let reported = (network as? [String: Any])?["failure"]
        let failure: Any? = reported is NSNull ? nil : reported
        if failure != nil || text(network, "state") == "failed" {
            let code = optText(failure, "code") ?? "other"
            let detail = (failure as? [String: Any])?["detail"]
            let mapping: [String: (String, String?)] = [
                "unencrypted_refused": ("mesh.notice.unencrypted_refused", "network_settings"),
                "secret_mismatch": ("mesh.notice.secret_mismatch", nil),
                "security_mismatch": ("mesh.notice.security_mismatch", "network_settings"),
                "port_in_use": ("mesh.notice.port_in_use", nil),
                "tun_in_use": ("mesh.notice.tun_in_use", nil),
                "route_overlap": ("mesh.notice.route_overlap", nil),
                "address_in_use": ("mesh.notice.address_in_use", nil),
                "permission_denied": ("mesh.notice.permission_denied", nil),
                "identity_online": ("mesh.notice.identity_online", nil),
                // Put in by the desktop client or the installer, never by the kernel.
                "existing_program": ("mesh.notice.existing_program", "take_over"),
                "unreachable_peers": ("mesh.notice.unreachable_peers", nil),
                // `detail` is the refused peer's node ID.
                "peer_not_native": ("mesh.notice.peer_not_native", nil),
                // `detail` (`empty`, `self_missing`) is for the log, not the page.
                "native_keys_pending": ("mesh.notice.native_keys_pending", nil),
            ]
            let (id, action) = mapping[code] ?? ("mesh.notice.join_failed", nil)
            // A refused peer is named by its member name where the snapshot lists it, and each peer has a notice key of
            // its own, so that one closed notice does not hide the next peer's.
            if code == "peer_not_native", let peer = detail as? String {
                push(id, "failure:\(code):\(peer)", [memberName(members, peer)], action, false)
            } else {
                let values: [Any] = (detail == nil || detail is NSNull || code == "native_keys_pending") ? [] : [detail!]
                push(id, "failure:\(code)", values, action, false)
            }
        }
        for path in list(status, "paths") where text(path, "state") == "fallback" {
            let unreachable = text(path, "unreachable")
            let target = (path as? [String: Any])?["target"]
            let label = optText(target, "subnet") ?? optText(target, "peer").map { memberName(members, $0) } ?? ""
            // `reason`: a node that does not forward on this path is not an unreachable one.
            let reason = text(path, "reason")
            let (id, values): (String, [Any])
            switch reason {
            case "unsupported", "not_relay":
                (id, values) = ("mesh.notice.path_unsupported", [memberName(members, unreachable), label])
            case "hop_limit":
                (id, values) = ("mesh.notice.path_hop_limit", [label])
            case "no_echo":
                (id, values) = ("mesh.notice.path_no_echo", [memberName(members, unreachable), label])
            default:
                (id, values) = ("mesh.notice.path_fallback", [memberName(members, unreachable), label])
            }
            push(id, "fallback:\(text(path, "id")):\(unreachable)", values, nil, true)
        }
        for link in list(status, "links") where text(link, "state") == "detour" {
            let reason = (link as? [String: Any])?["reason"]
            let id: String, why: Any
            if let loss = ((reason as? [String: Any])?["loss"] as? NSNumber).flatMap({ isBool($0) ? nil : $0.doubleValue }) {
                (id, why) = ("mesh.notice.detour_loss", Int((loss * 100).rounded()))
            } else {
                (id, why) = ("mesh.notice.detour_latency", orNull(latency(reason)))
            }
            let (from, to, via) = (text(link, "from"), text(link, "to"), text(link, "via"))
            push(
                id,
                "detour:\(from):\(to):\(via)",
                [regionName(status, from), regionName(status, to), regionName(status, via), why],
                nil,
                false
            )
        }
        if text(network, "state") == "partial" {
            let names = members.filter { text($0, "state") == "unreachable" }.map { text($0, "name") }
            if !names.isEmpty {
                push("mesh.notice.partial", "partial", [names.count, names], nil, false)
            }
        }
        return notices
    }

    // MARK: Entry

    /// The entry on the overview, in the menu bar and in the debug settings. See `mesh::entry`.
    public static func entry(status: Any?) -> [String: Any] {
        guard let object = status as? [String: Any], bool(object, "available") != false else {
            return ["available": false]
        }
        guard let network = object["network"] as? [String: Any] else {
            return ["available": true, "network": NSNull(), "state": "not_joined"]
        }
        let others = list(status, "members")
        let online = others.filter { text($0, "state") == "online" }.count
        return [
            "available": true,
            "network": orNull(optText(network, "name") ?? optText(network, "id")),
            "state": optText(network, "state") ?? "not_joined",
            "security": orNull(optText(network, "security")),
            "online": online,
            "total": others.count,
            "attention": !notices(status: status).isEmpty,
        ]
    }
}
