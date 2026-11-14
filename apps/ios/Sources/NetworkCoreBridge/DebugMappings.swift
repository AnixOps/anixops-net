import Foundation

/// Map Local and Map Remote (plan E3): answer a request with a response kept on the device, or send it to another
/// URL, made from the debugging views instead of hand-written plugin lines. The engine already runs both as plugin
/// lines, so the enabled entries become one generated Loon plugin (`pluginText`) that goes to the tunnel with the
/// imported plugins: `[Map Local]` for local responses, `[Rewrite] … 307 …` for remote ones (307 keeps the method and
/// body), and `[MITM] hostname` for the hosts they need decrypted. Not secret; kept in `debug-mappings.json`.
public struct DebugMappings: Codable, Equatable, Sendable {
    /// Most entries kept.
    public static let maxEntries = 200
    /// Largest local response body: the engine inlines `[Map Local]` data up to this size.
    public static let maxBodyBytes = 256 * 1024
    /// The generated plugin's name, as the engine's plugin report shows it.
    public static let pluginName = "AnixOps 调试映射"

    /// How the URL field matches.
    public enum Match: String, Codable, CaseIterable, Identifiable, Sendable {
        /// This URL with any query string.
        case url
        /// Every URL that starts with this one; Map Remote keeps the rest of the path.
        case prefix
        /// A regular expression over the whole URL, as plugin lines write it.
        case regex

        public var id: String {
            rawValue
        }
    }

    public enum Kind: String, Codable, CaseIterable, Identifiable, Sendable {
        /// Answer from the device (`[Map Local]`).
        case local
        /// Send the request to another URL (`[Rewrite]` 307).
        case remote

        public var id: String {
            rawValue
        }
    }

    public struct Header: Codable, Equatable, Hashable, Sendable {
        public var name: String
        public var value: String

        public init(name: String, value: String) {
            self.name = name
            self.value = value
        }
    }

    public struct Entry: Codable, Equatable, Identifiable, Sendable {
        public var id: UUID
        public var name: String
        public var enabled: Bool
        public var kind: Kind
        public var match: Match
        /// A URL, a URL prefix or a regular expression, by `match`.
        public var pattern: String
        // Map Local.
        public var status: Int
        public var headers: [Header]
        public var body: Data
        // Map Remote.
        public var target: String

        public init(
            id: UUID = UUID(),
            name: String = "",
            enabled: Bool = true,
            kind: Kind,
            match: Match = .url,
            pattern: String,
            status: Int = 200,
            headers: [Header] = [],
            body: Data = Data(),
            target: String = ""
        ) {
            self.id = id
            self.name = name
            self.enabled = enabled
            self.kind = kind
            self.match = match
            self.pattern = pattern
            self.status = status
            self.headers = headers
            self.body = body
            self.target = target
        }

        /// The regular expression the engine matches the request URL with.
        public var regex: String {
            switch match {
            case .url:
                // The query, if any, is captured so Map Remote can pass it on.
                return "^" + DebugMappings.escape(pattern) + "(\\?.*)?$"
            case .prefix:
                return "^" + DebugMappings.escape(pattern) + "(.*)$"
            case .regex:
                return pattern
            }
        }

        /// The host the entry needs decrypted, when the URL names one.
        public var host: String? {
            match == .regex ? DebugMappings.regexHost(pattern) : DebugMappings.host(of: pattern)
        }

        /// The plugin line for the entry.
        public var line: String {
            switch kind {
            case .local:
                var fields = [
                    DebugMappings.quote(regex),
                    "data-type=base64",
                    "data=\"\(body.base64EncodedString())\"",
                    "status-code=\(status)",
                ]
                let header = headers
                    .filter { !$0.name.trimmingCharacters(in: .whitespaces).isEmpty }
                    .map { "\($0.name.trimmingCharacters(in: .whitespaces)):\($0.value)" }
                    .joined(separator: "|")
                if !header.isEmpty {
                    fields.append("header=" + DebugMappings.quote(header, always: true))
                }
                return fields.joined(separator: " ")
            case .remote:
                // The captured query (`url`) or rest of the URL (`prefix`) follows the new address.
                let next = match == .regex ? target : target + "$1"
                return "\(DebugMappings.quote(regex)) 307 \(DebugMappings.quote(next))"
            }
        }

        /// The body as text, when it is UTF-8.
        public var bodyText: String? {
            String(data: body, encoding: .utf8)
        }

        /// The header named `name`, case-insensitively.
        public func header(_ name: String) -> String? {
            headers.first { $0.name.caseInsensitiveCompare(name) == .orderedSame }?.value
        }

        /// Throws when the engine could not use the entry; the message says which field is wrong.
        public func validate() throws {
            let trimmed = pattern.trimmingCharacters(in: .whitespaces)
            guard !trimmed.isEmpty, !pattern.contains("\n") else {
                throw DebugMappingsError.emptyPattern
            }
            if match != .regex {
                guard let url = URL(string: trimmed), ["http", "https"].contains(url.scheme?.lowercased() ?? ""),
                      url.host != nil
                else {
                    throw DebugMappingsError.invalidURL
                }
            }
            guard (try? NSRegularExpression(pattern: regex)) != nil else {
                throw DebugMappingsError.invalidRegex
            }
            switch kind {
            case .local:
                guard (100...599).contains(status) else {
                    throw DebugMappingsError.invalidStatus
                }
                guard body.count <= DebugMappings.maxBodyBytes else {
                    throw DebugMappingsError.bodyTooLarge
                }
                for header in headers where !header.name.trimmingCharacters(in: .whitespaces).isEmpty {
                    let name = header.name.trimmingCharacters(in: .whitespaces)
                    let validName = name.unicodeScalars.allSatisfy { (0x21...0x7e).contains($0.value) && $0 != ":" }
                    guard validName, !header.value.contains("|"), !header.value.contains("\n"),
                          !header.value.contains("\r")
                    else {
                        throw DebugMappingsError.invalidHeader
                    }
                }
            case .remote:
                guard let url = URL(string: target), ["http", "https"].contains(url.scheme?.lowercased() ?? ""),
                      url.host != nil, !target.contains(where: \.isWhitespace)
                else {
                    throw DebugMappingsError.invalidTarget
                }
            }
        }
    }

    public var entries: [Entry]

    public init(entries: [Entry] = []) {
        self.entries = entries
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        entries = try container.decodeIfPresent([Entry].self, forKey: .entries) ?? []
    }

    /// The entries the tunnel gets: enabled and usable.
    public var active: [Entry] {
        entries.filter { $0.enabled && (try? $0.validate()) != nil }
    }

    /// The generated plugin, or nil when no entry is on.
    public var pluginText: String? {
        let active = active
        guard !active.isEmpty else {
            return nil
        }
        var lines = ["#!name=\(Self.pluginName)", "#!desc=由调试视图生成的 Map Local / Map Remote 规则"]
        let remote = active.filter { $0.kind == .remote }
        let local = active.filter { $0.kind == .local }
        if !remote.isEmpty {
            lines.append("")
            lines.append("[Rewrite]")
            lines.append(contentsOf: remote.map(\.line))
        }
        if !local.isEmpty {
            lines.append("")
            lines.append("[Map Local]")
            lines.append(contentsOf: local.map(\.line))
        }
        var hosts: [String] = []
        for host in active.compactMap(\.host) where !hosts.contains(host) {
            hosts.append(host)
        }
        if !hosts.isEmpty {
            lines.append("")
            lines.append("[MITM]")
            lines.append("hostname = %APPEND% " + hosts.joined(separator: ", "))
        }
        return lines.joined(separator: "\n") + "\n"
    }

    /// Adds `entry`, or replaces the entry with its id.
    public func saving(_ entry: Entry) throws -> DebugMappings {
        try entry.validate()
        var copy = self
        if let index = copy.entries.firstIndex(where: { $0.id == entry.id }) {
            copy.entries[index] = entry
        } else {
            guard copy.entries.count < Self.maxEntries else {
                throw DebugMappingsError.tooMany
            }
            copy.entries.append(entry)
        }
        return copy
    }

    // MARK: - Text helpers

    /// `text` with every regular-expression metacharacter escaped.
    public static func escape(_ text: String) -> String {
        let special = Set("\\^$.|?*+()[]{}")
        var escaped = ""
        for character in text {
            if special.contains(character) {
                escaped.append("\\")
            }
            escaped.append(character)
        }
        return escaped
    }

    /// The host a regular expression names literally, as in `^https://api\.example\.com/v1/.*`; nil when the
    /// host part is itself a pattern.
    static func regexHost(_ regex: String) -> String? {
        var text = Substring(regex)
        if text.hasPrefix("^") {
            text = text.dropFirst()
        }
        let unslashed = text.replacingOccurrences(of: "\\/", with: "/")
        for scheme in ["https?://", "http://", "https://"] where unslashed.hasPrefix(scheme) {
            var name = ""
            var escaping = false
            for character in unslashed.dropFirst(scheme.count) {
                if escaping {
                    name.append(character)
                    escaping = false
                } else if character == "\\" {
                    escaping = true
                } else if character == "/" || character == ":" || "^$|?*+()[]{}".contains(character) {
                    break
                } else if character == "." {
                    // An unescaped dot matches any character: not one host.
                    return nil
                } else {
                    name.append(character)
                }
            }
            return host(of: "https://" + name)
        }
        return nil
    }

    /// The host of an `http(s)://host/...` text, lower-cased; nil without one.
    static func host(of text: String) -> String? {
        guard let schemeEnd = text.range(of: "://") else {
            return nil
        }
        let scheme = text[..<schemeEnd.lowerBound].lowercased()
        guard scheme == "http" || scheme == "https" else {
            return nil
        }
        var host = text[schemeEnd.upperBound...].prefix { $0 != "/" && $0 != "?" && $0 != "#" }
        if let at = host.lastIndex(of: "@") {
            host = host[host.index(after: at)...]
        }
        if host.hasPrefix("[") {
            return nil
        }
        let name = host.split(separator: ":", maxSplits: 1).first.map(String.init) ?? ""
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: ".-"))
        guard !name.isEmpty, name.unicodeScalars.allSatisfy(allowed.contains), name.contains(".") else {
            return nil
        }
        return name.lowercased()
    }

    /// A plugin token: bare when it has no space or quote, else in double quotes with `"` and `\` escaped.
    static func quote(_ token: String, always: Bool = false) -> String {
        if !always, !token.isEmpty, !token.hasPrefix("\""), !token.hasPrefix("'"),
           !token.contains(where: \.isWhitespace)
        {
            return token
        }
        var quoted = "\""
        for character in token {
            if character == "\"" || character == "\\" {
                quoted.append("\\")
            }
            quoted.append(character)
        }
        return quoted + "\""
    }

    // MARK: - From a captured request

    /// A Map Local entry answering `url` with the captured response, ready to edit.
    public static func local(
        url: String,
        status: Int?,
        headers: [Header],
        body: Data
    ) -> Entry {
        // Hop-by-hop and length headers would not match the edited body.
        let skipped: Set<String> = ["content-length", "transfer-encoding", "content-encoding", "connection", "keep-alive"]
        return Entry(
            kind: .local,
            match: .url,
            pattern: withoutQuery(url),
            status: status ?? 200,
            headers: headers.filter { !skipped.contains($0.name.lowercased()) },
            body: body.count <= maxBodyBytes ? body : Data()
        )
    }

    /// A Map Remote entry for `url`, its target left to fill in.
    public static func remote(url: String) -> Entry {
        let base = withoutQuery(url)
        return Entry(kind: .remote, match: .url, pattern: base, target: base)
    }

    static func withoutQuery(_ url: String) -> String {
        String(url.prefix { $0 != "?" && $0 != "#" })
    }

    // MARK: - Storage

    public static var defaultURL: URL {
        get throws {
            try FileManager.default
                .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                .appendingPathComponent("debug-mappings.json")
        }
    }

    public static func load(from url: URL) throws -> DebugMappings {
        guard FileManager.default.fileExists(atPath: url.path) else {
            return DebugMappings()
        }
        return try JSONDecoder().decode(DebugMappings.self, from: Data(contentsOf: url))
    }

    public func save(to url: URL) throws {
        try JSONEncoder().encode(self).write(to: url, options: [.atomic])
    }
}

/// Why a mapping was refused. Fixed text without the values typed.
public enum DebugMappingsError: Error, Equatable {
    case emptyPattern
    case invalidURL
    case invalidRegex
    case invalidStatus
    case invalidHeader
    case bodyTooLarge
    case invalidTarget
    case tooMany

    public var message: String {
        switch self {
        case .emptyPattern:
            return String(localized: "请填写要匹配的 URL")
        case .invalidURL:
            return String(localized: "URL 应以 http:// 或 https:// 开头并带主机名")
        case .invalidRegex:
            return String(localized: "正则表达式无效")
        case .invalidStatus:
            return String(localized: "状态码应为 100–599")
        case .invalidHeader:
            return String(localized: "响应头每行写“名称: 值”，值里不能有 |")
        case .bodyTooLarge:
            return String(localized: "响应体不能超过 256 KB")
        case .invalidTarget:
            return String(localized: "目标应为 http:// 或 https:// 开头的完整 URL")
        case .tooMany:
            return String(localized: "已达到数量上限")
        }
    }
}
