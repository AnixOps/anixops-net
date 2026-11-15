import Foundation

/// A JSON value as the request inspector shows it. The parser is our own rather than `JSONSerialization` for three
/// reasons a debugger cares about: numbers keep the digits the server sent (a 64-bit id is not rounded through a
/// double), keys keep their order, and a key that appears twice shows twice.
public enum JsonNode: Equatable, Sendable {
    case object([JsonMember])
    case array([JsonNode])
    case string(String)
    /// The number exactly as written in the document.
    case number(String)
    case bool(Bool)
    case null

    /// How many children an object or an array has; 0 for a scalar.
    public var childCount: Int {
        switch self {
        case let .object(members):
            return members.count
        case let .array(items):
            return items.count
        default:
            return 0
        }
    }

    /// The children in order, each with its key (objects) or nil (arrays).
    var children: [(name: String?, value: JsonNode)] {
        switch self {
        case let .object(members):
            return members.map { (name: $0.name, value: $0.value) }
        case let .array(items):
            return items.map { (name: nil, value: $0) }
        default:
            return []
        }
    }
}

/// One `"name": value` of an object.
public struct JsonMember: Equatable, Sendable {
    public let name: String
    public let value: JsonNode

    public init(name: String, value: JsonNode) {
        self.name = name
        self.value = value
    }
}

/// One line of the tree. `id` is stable for the document: it is made of positions, so a repeated key still gets
/// its own.
public struct JsonRow: Equatable, Identifiable, Sendable {
    public let id: String
    /// The id of the row this one sits under; nil for the root.
    public let parent: String?
    public let depth: Int
    /// The object key; nil for array items and the root.
    public let name: String?
    /// The position in the parent array; nil for object members and the root.
    public let index: Int?
    public let node: JsonNode
    /// JSONPath to this value, for copying: `$.items[0].title`.
    public let path: String
    /// For an object or an array with children: whether its children are listed. Nil for everything else.
    public let open: Bool?
}

public enum JsonDocument {
    /// Deeper nesting than this is not shown as a tree.
    public static let maxDepth = 200
    /// A document with more values than this opens as a two-level outline.
    public static let outlineAbove = 400

    /// The document, or nil when the text is not one complete JSON value (a cut body, JSON lines, plain text).
    public static func parse(_ text: String) -> JsonNode? {
        var reader = Reader(bytes: Array(text.utf8))
        if reader.bytes.starts(with: [0xEF, 0xBB, 0xBF]) {
            reader.at = 3
        }
        guard let node = try? reader.value(depth: 0) else {
            return nil
        }
        reader.space()
        return reader.at == reader.bytes.count ? node : nil
    }

    /// The visible lines of the tree, top to bottom, given which containers are folded.
    public static func rows(_ root: JsonNode, collapsed: Set<String>) -> [JsonRow] {
        var rows: [JsonRow] = []
        func visit(_ node: JsonNode, id: String, parent: String?, depth: Int, name: String?, index: Int?, path: String) {
            let open: Bool? = node.childCount > 0 ? !collapsed.contains(id) : nil
            rows.append(JsonRow(id: id, parent: parent, depth: depth, name: name, index: index, node: node, path: path, open: open))
            guard open == true else {
                return
            }
            for (position, child) in node.children.enumerated() {
                let childPath = child.name.map { memberPath(path, $0) } ?? "\(path)[\(position)]"
                visit(
                    child.value, id: "\(id)/\(position)", parent: id, depth: depth + 1,
                    name: child.name, index: child.name == nil ? position : nil, path: childPath
                )
            }
        }
        visit(root, id: "$", parent: nil, depth: 0, name: nil, index: nil, path: "$")
        return rows
    }

    /// The id of every container that has children, from `fromDepth` down, for "fold all". The root stays out by
    /// default, or the tree would be one line.
    public static func containerIDs(_ root: JsonNode, fromDepth: Int = 1) -> [String] {
        var ids: [String] = []
        func visit(_ node: JsonNode, id: String, depth: Int) {
            guard node.childCount > 0 else {
                return
            }
            if depth >= fromDepth {
                ids.append(id)
            }
            for (position, child) in node.children.enumerated() {
                visit(child.value, id: "\(id)/\(position)", depth: depth + 1)
            }
        }
        visit(root, id: "$", depth: 0)
        return ids
    }

    /// How many values the document holds, counting containers. Stops counting at `limit`.
    public static func nodeCount(_ root: JsonNode, limit: Int = .max) -> Int {
        var count = 0
        func visit(_ node: JsonNode) {
            guard count < limit else {
                return
            }
            count += 1
            for child in node.children {
                visit(child.value)
            }
        }
        visit(root)
        return count
    }

    /// A small document opens fully; a large one opens two levels, so the first screen is an outline.
    public static func initiallyCollapsed(_ root: JsonNode) -> Set<String> {
        nodeCount(root, limit: outlineAbove + 1) > outlineAbove ? Set(containerIDs(root, fromDepth: 2)) : []
    }

    /// The value indented by two spaces. Numbers are written as received.
    public static func pretty(_ node: JsonNode) -> String {
        var out = ""
        func visit(_ node: JsonNode, indent: String) {
            switch node {
            case let .string(value):
                out += quoted(value)
            case let .number(raw):
                out += raw
            case let .bool(value):
                out += value ? "true" : "false"
            case .null:
                out += "null"
            case .object, .array:
                let isObject: Bool
                if case .object = node {
                    isObject = true
                } else {
                    isObject = false
                }
                let children = node.children
                if children.isEmpty {
                    out += isObject ? "{}" : "[]"
                    return
                }
                let inner = indent + "  "
                out += isObject ? "{" : "["
                for (position, child) in children.enumerated() {
                    out += "\n" + inner
                    if let name = child.name {
                        out += quoted(name) + ": "
                    }
                    visit(child.value, indent: inner)
                    if position < children.count - 1 {
                        out += ","
                    }
                }
                out += "\n" + indent + (isObject ? "}" : "]")
            }
        }
        visit(node, indent: "")
        return out
    }

    /// What copying one tree row puts on the pasteboard: a string's own text, otherwise the value as JSON.
    public static func copyText(_ node: JsonNode) -> String {
        if case let .string(value) = node {
            return value
        }
        return pretty(node)
    }

    /// A scalar as one line of the tree shows it; nil for a container.
    public static func scalarText(_ node: JsonNode) -> String? {
        switch node {
        case let .string(value):
            return quoted(value)
        case let .number(raw):
            return raw
        case let .bool(value):
            return value ? "true" : "false"
        case .null:
            return "null"
        case .object, .array:
            return nil
        }
    }

    /// `text` as a JSON string literal.
    public static func quoted(_ text: String) -> String {
        var out = "\""
        for scalar in text.unicodeScalars {
            switch scalar {
            case "\"":
                out += "\\\""
            case "\\":
                out += "\\\\"
            case "\n":
                out += "\\n"
            case "\r":
                out += "\\r"
            case "\t":
                out += "\\t"
            default:
                if scalar.value < 0x20 {
                    out += String(format: "\\u%04x", scalar.value)
                } else {
                    out.unicodeScalars.append(scalar)
                }
            }
        }
        return out + "\""
    }

    static func memberPath(_ parent: String, _ name: String) -> String {
        let plain = !name.isEmpty && name.unicodeScalars.enumerated().allSatisfy { position, scalar in
            let letter = (scalar >= "a" && scalar <= "z") || (scalar >= "A" && scalar <= "Z") || scalar == "_" || scalar == "$"
            return letter || (position > 0 && scalar >= "0" && scalar <= "9")
        }
        return plain ? "\(parent).\(name)" : "\(parent)[\(quoted(name))]"
    }
}

private struct Reader {
    struct Failure: Error {}

    let bytes: [UInt8]
    var at = 0

    var peek: UInt8? {
        at < bytes.count ? bytes[at] : nil
    }

    mutating func space() {
        while let byte = peek, byte == 0x20 || byte == 0x0A || byte == 0x0D || byte == 0x09 {
            at += 1
        }
    }

    mutating func value(depth: Int) throws -> JsonNode {
        guard depth <= JsonDocument.maxDepth else {
            throw Failure()
        }
        space()
        switch peek ?? 0 {
        case UInt8(ascii: "{"):
            return try object(depth: depth)
        case UInt8(ascii: "["):
            return try array(depth: depth)
        case UInt8(ascii: "\""):
            return .string(try string())
        default:
            if word("true") {
                return .bool(true)
            }
            if word("false") {
                return .bool(false)
            }
            if word("null") {
                return .null
            }
            return try number()
        }
    }

    mutating func word(_ word: String) -> Bool {
        let wanted = Array(word.utf8)
        guard at + wanted.count <= bytes.count, bytes[at..<at + wanted.count].elementsEqual(wanted) else {
            return false
        }
        at += wanted.count
        return true
    }

    private func isDigit(_ byte: UInt8?) -> Bool {
        guard let byte else {
            return false
        }
        return byte >= UInt8(ascii: "0") && byte <= UInt8(ascii: "9")
    }

    private mutating func digits() throws {
        guard isDigit(peek) else {
            throw Failure()
        }
        while isDigit(peek) {
            at += 1
        }
    }

    mutating func number() throws -> JsonNode {
        let start = at
        if peek == UInt8(ascii: "-") {
            at += 1
        }
        if peek == UInt8(ascii: "0") {
            at += 1
        } else {
            try digits()
        }
        if peek == UInt8(ascii: ".") {
            at += 1
            try digits()
        }
        if peek == UInt8(ascii: "e") || peek == UInt8(ascii: "E") {
            at += 1
            if peek == UInt8(ascii: "+") || peek == UInt8(ascii: "-") {
                at += 1
            }
            try digits()
        }
        return .number(String(decoding: bytes[start..<at], as: UTF8.self))
    }

    /// Four hex digits at the cursor, as a UTF-16 code unit.
    private mutating func hex4() throws -> UInt32 {
        guard at + 4 <= bytes.count, let unit = UInt32(String(decoding: bytes[at..<at + 4], as: UTF8.self), radix: 16) else {
            throw Failure()
        }
        // `UInt32(_:radix:)` also takes a sign; four hex digits never have one.
        guard bytes[at] != UInt8(ascii: "+"), bytes[at] != UInt8(ascii: "-") else {
            throw Failure()
        }
        at += 4
        return unit
    }

    mutating func string() throws -> String {
        at += 1
        var out: [UInt8] = []
        func append(_ scalar: UInt32) {
            out.append(contentsOf: Array(String(Character(Unicode.Scalar(scalar) ?? "\u{FFFD}")).utf8))
        }
        while true {
            guard let byte = peek else {
                throw Failure()
            }
            if byte == UInt8(ascii: "\"") {
                at += 1
                return String(decoding: out, as: UTF8.self)
            }
            if byte < 0x20 {
                throw Failure()
            }
            if byte != UInt8(ascii: "\\") {
                out.append(byte)
                at += 1
                continue
            }
            at += 1
            guard let escape = peek else {
                throw Failure()
            }
            at += 1
            switch escape {
            case UInt8(ascii: "\""), UInt8(ascii: "\\"), UInt8(ascii: "/"):
                out.append(escape)
            case UInt8(ascii: "b"):
                out.append(0x08)
            case UInt8(ascii: "f"):
                out.append(0x0C)
            case UInt8(ascii: "n"):
                out.append(0x0A)
            case UInt8(ascii: "r"):
                out.append(0x0D)
            case UInt8(ascii: "t"):
                out.append(0x09)
            case UInt8(ascii: "u"):
                let unit = try hex4()
                if (0xD800...0xDBFF).contains(unit), at + 6 <= bytes.count, bytes[at] == UInt8(ascii: "\\"), bytes[at + 1] == UInt8(ascii: "u") {
                    // A surrogate pair: two escapes for one character.
                    let saved = at
                    at += 2
                    let low = try hex4()
                    if (0xDC00...0xDFFF).contains(low) {
                        append(0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00))
                    } else {
                        at = saved
                        append(0xFFFD)
                    }
                } else {
                    // A lone surrogate is not a character; `Unicode.Scalar` refuses it and U+FFFD stands in.
                    append(unit)
                }
            default:
                throw Failure()
            }
        }
    }

    mutating func object(depth: Int) throws -> JsonNode {
        at += 1
        var members: [JsonMember] = []
        space()
        if peek == UInt8(ascii: "}") {
            at += 1
            return .object(members)
        }
        while true {
            space()
            guard peek == UInt8(ascii: "\"") else {
                throw Failure()
            }
            let name = try string()
            space()
            guard peek == UInt8(ascii: ":") else {
                throw Failure()
            }
            at += 1
            members.append(JsonMember(name: name, value: try value(depth: depth + 1)))
            space()
            let next = peek
            at += 1
            if next == UInt8(ascii: "}") {
                return .object(members)
            }
            guard next == UInt8(ascii: ",") else {
                throw Failure()
            }
        }
    }

    mutating func array(depth: Int) throws -> JsonNode {
        at += 1
        var items: [JsonNode] = []
        space()
        if peek == UInt8(ascii: "]") {
            at += 1
            return .array(items)
        }
        while true {
            items.append(try value(depth: depth + 1))
            space()
            let next = peek
            at += 1
            if next == UInt8(ascii: "]") {
                return .array(items)
            }
            guard next == UInt8(ascii: ",") else {
                throw Failure()
            }
        }
    }
}
