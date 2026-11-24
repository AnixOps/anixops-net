import Foundation

/// Map Local and Map Remote (plan E3): answer a request with a response kept on the device, or send it to another
/// URL, made from the debugging views instead of hand-written plugin lines. The engine already runs both as plugin
/// lines, so the enabled entries become one generated Loon plugin (`pluginText`) that goes to the tunnel with the
/// imported plugins: `[Map Local]` for local responses, `[Rewrite] … 307 …` for remote ones (307 keeps the method and
/// body), and `[MITM] hostname` for the hosts they need decrypted. Not secret; kept in `debug-mappings.json`.
///
/// The same file keeps the decryption list (`domains`, "解密域名"): HTTPS decryption is per domain, and the list's
/// lines join the plugin's `[MITM] hostname` line even when no mapping is on: domains and wildcard patterns to
/// decrypt, exclusions (`-pattern`, which win over every include) and paused lines. The list starts empty. The
/// desktop clients read and write the same shape (`crates/desktop-mitm/src/mappings.rs`).
public struct DebugMappings: Codable, Equatable, Sendable {
    /// Most entries kept.
    public static let maxEntries = 200
    /// Most decrypted domains kept.
    public static let maxHosts = 500
    /// Largest local response body: the engine inlines `[Map Local]` data up to this size.
    public static let maxBodyBytes = 256 * 1024
    /// Largest generated plugin (`pluginText`), in UTF-8 bytes: the Map Local bodies count Base64-encoded, as the
    /// plugin carries them. Half the engine's decryption budget (`EngineBudget.maxDecryptionBytes`), which the
    /// imported plugins share.
    public static let maxPluginBytes = 4 * 1024 * 1024
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

    /// One line of the decryption list: a domain or a wildcard pattern, decrypted or excluded, on or paused.
    public struct Domain: Codable, Equatable, Hashable, Identifiable, Sendable {
        /// Lower-cased, as `normalizePattern` gives it: `api.example.com`, `*.example.com`, `img-?.example.com`.
        public var pattern: String
        /// Never decrypted, even when another line, a plugin or a mapping names it. Written `-pattern` in the
        /// plugin's `[MITM] hostname` line, and an exclusion from any plugin wins over every include.
        public var exclude: Bool
        /// A paused line stays in the list but does not reach the tunnel.
        public var enabled: Bool

        public init(pattern: String, exclude: Bool = false, enabled: Bool = true) {
            self.pattern = pattern
            self.exclude = exclude
            self.enabled = enabled
        }

        public var id: String {
            pattern
        }

        /// As the plugin line writes it.
        public var item: String {
            exclude ? "-" + pattern : pattern
        }
    }

    public var entries: [Entry]
    /// The decryption list, in the order added. A pattern is listed once, as an include or an exclusion.
    public var domains: [Domain]

    /// The patterns decrypted: the list's lines that are on and not exclusions.
    public var hosts: [String] {
        domains.filter { $0.enabled && !$0.exclude }.map(\.pattern)
    }

    /// The patterns never decrypted: the list's exclusions that are on.
    public var excludedHosts: [String] {
        domains.filter { $0.enabled && $0.exclude }.map(\.pattern)
    }

    public init(entries: [Entry] = [], hosts: [String] = []) {
        self.init(entries: entries, domains: hosts.map { Domain(pattern: $0) })
    }

    public init(entries: [Entry] = [], domains: [Domain]) {
        self.entries = entries
        self.domains = domains
    }

    enum CodingKeys: String, CodingKey {
        case entries, hosts, domains
    }

    /// Files written before the list had exclusions and pauses keep only `hosts`; they load as lines that are on.
    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        entries = try container.decodeIfPresent([Entry].self, forKey: .entries) ?? []
        if let domains = try container.decodeIfPresent([Domain].self, forKey: .domains) {
            self.domains = domains
        } else {
            let hosts = try container.decodeIfPresent([String].self, forKey: .hosts) ?? []
            domains = hosts.map { Domain(pattern: $0) }
        }
    }

    /// `hosts` is still written for older builds, which read only the decrypted patterns.
    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(entries, forKey: .entries)
        if !domains.isEmpty {
            try container.encode(domains, forKey: .domains)
        }
        let hosts = hosts
        if !hosts.isEmpty {
            try container.encode(hosts, forKey: .hosts)
        }
    }

    /// The entries the tunnel gets: enabled and usable.
    public var active: [Entry] {
        entries.filter { $0.enabled && (try? $0.validate()) != nil }
    }

    /// The patterns the generated plugin decrypts: the hosts of the entries that are on, then the list's lines that
    /// are on, without repeats and without the exclusions. Empty when the plugin decrypts nothing, for example when
    /// only exclusions are on (`DecryptionConsent`).
    public var decryptedPatterns: [String] {
        let excluded = excludedHosts
        var items: [String] = []
        for host in active.compactMap(\.host) + hosts where !items.contains(host) && !excluded.contains(host) {
            items.append(host)
        }
        return items
    }

    /// The generated plugin, or nil when no entry is on and no list line is on.
    public var pluginText: String? {
        let active = active
        let excluded = excludedHosts
        guard !active.isEmpty || !hosts.isEmpty || !excluded.isEmpty else {
            return nil
        }
        var lines = ["#!name=\(Self.pluginName)", "#!desc=由调试视图生成的 Map Local / Map Remote 规则和解密域名"]
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
        var items = decryptedPatterns
        items += excluded.map { "-" + $0 }
        if !items.isEmpty {
            lines.append("")
            lines.append("[MITM]")
            lines.append("hostname = %APPEND% " + items.joined(separator: ", "))
        }
        return lines.joined(separator: "\n") + "\n"
    }

    /// What the generated plugin counts against the decryption budget: its UTF-8 bytes, 0 without one.
    public var pluginBytes: Int {
        pluginText?.utf8.count ?? 0
    }

    /// Throws when these mappings, replacing `previous`, make the generated plugin larger and over
    /// `maxPluginBytes` (`totalTooLarge`), or over the engine's decryption budget together with `importedBytes`, the
    /// enabled imported plugins (`decryptionBudgetFull`). A change that does not make it larger always passes, so
    /// mappings saved over a budget by an earlier version can still be turned off, shortened or deleted.
    public func checkBudget(replacing previous: DebugMappings, importedBytes: Int) throws {
        let bytes = pluginBytes
        guard bytes > previous.pluginBytes else {
            return
        }
        guard bytes <= Self.maxPluginBytes else {
            throw DebugMappingsError.totalTooLarge
        }
        guard importedBytes + bytes <= EngineBudget.maxDecryptionBytes else {
            throw DebugMappingsError.decryptionBudgetFull
        }
    }

    /// With one more line in the decryption list: a domain, a wildcard pattern, or an exclusion (`exclude`, or
    /// typed with a leading `-`). A pattern already listed is turned on and takes the new kind.
    public func addingHost(_ text: String, exclude: Bool = false) throws -> DebugMappings {
        let (pattern, excluding) = try Self.parseListLine(text, exclude: exclude)
        var copy = self
        if let index = copy.domains.firstIndex(where: { $0.pattern == pattern }) {
            copy.domains[index].exclude = excluding
            copy.domains[index].enabled = true
            try copy.checkBudget(replacing: self, importedBytes: 0)
            return copy
        }
        guard domains.count < Self.maxHosts else {
            throw DebugMappingsError.tooMany
        }
        copy.domains.append(Domain(pattern: pattern, exclude: excluding))
        try copy.checkBudget(replacing: self, importedBytes: 0)
        return copy
    }

    /// With the line for `pattern` rewritten from `text` (and `exclude`), in the same place and on.
    public func editingHost(_ pattern: String, to text: String, exclude: Bool) throws -> DebugMappings {
        let (next, excluding) = try Self.parseListLine(text, exclude: exclude)
        guard let index = domains.firstIndex(where: { $0.pattern == pattern }) else {
            return try addingHost(text, exclude: exclude)
        }
        if next != pattern, domains.contains(where: { $0.pattern == next }) {
            throw DebugMappingsError.duplicateHost
        }
        var copy = self
        copy.domains[index] = Domain(pattern: next, exclude: excluding)
        try copy.checkBudget(replacing: self, importedBytes: 0)
        return copy
    }

    /// With the line for `pattern` on or paused.
    public func settingHost(_ pattern: String, enabled: Bool) -> DebugMappings {
        var copy = self
        for index in copy.domains.indices where copy.domains[index].pattern == pattern {
            copy.domains[index].enabled = enabled
        }
        return copy
    }

    /// Without the line for `pattern`.
    public func removingHost(_ pattern: String) -> DebugMappings {
        var copy = self
        copy.domains.removeAll { $0.pattern == pattern }
        return copy
    }

    /// Whether the list decrypts `host`: a line that is on matches it and no exclusion that is on does.
    public func decrypts(_ host: String) -> Bool {
        DecryptionDomains(patterns: hosts, excluded: excludedHosts).covers(host)
    }

    /// Whether an exclusion of the list that is on names `host`.
    public func excludes(_ host: String) -> Bool {
        DecryptionDomains(patterns: [], excluded: excludedHosts).excludes(host)
    }

    /// `text` as a list line: the pattern and whether it is an exclusion.
    private static func parseListLine(_ text: String, exclude: Bool) throws -> (String, Bool) {
        var text = text.trimmingCharacters(in: .whitespaces)
        var excluding = exclude
        if let first = text.first, first == "-" || first == "!" {
            excluding = true
            text.removeFirst()
        }
        guard let pattern = normalizePattern(text) else {
            throw DebugMappingsError.invalidHost
        }
        return (pattern, excluding)
    }

    /// A list line as typed: a domain (`normalizeHost`), or a wildcard pattern where `*` is any run of characters
    /// (dots too) and `?` one character, such as `*.example.com`, `*cdn.example.com` or `img-?.example.*`. Same
    /// rules as the engine's `[MITM] hostname` items; a pattern must keep a dot and a literal letter or digit, so a
    /// bare `*` (everything) is refused. Nil when it is neither.
    public static func normalizePattern(_ text: String) -> String? {
        let text = text.trimmingCharacters(in: .whitespaces)
        guard text.contains("*") || text.contains("?") else {
            return normalizeHost(text)
        }
        var name = Substring(text)
        if let scheme = name.range(of: "://") {
            name = name[scheme.upperBound...]
        }
        name = name.prefix { $0 != "/" && $0 != "#" }
        if let colon = name.lastIndex(of: ":"), name[name.index(after: colon)...].allSatisfy(\.isNumber) {
            name = name[..<colon]
        }
        while name.hasSuffix(".") {
            name = name.dropLast()
        }
        let lower = name.lowercased()
        let allowed = Set("abcdefghijklmnopqrstuvwxyz0123456789.-_*?")
        guard lower.count <= 253, lower.allSatisfy(allowed.contains), lower.contains("."),
              !lower.hasPrefix("."), !lower.hasPrefix("-"), !lower.contains(".."),
              lower.contains(where: { $0.isLetter || $0.isNumber })
        else {
            return nil
        }
        return lower
    }

    /// A domain as typed or taken from a request ("API.example.com:443", "https://a.test/x", "*.example.com"),
    /// lower-cased and without scheme, path or port; nil when it is not a domain name. Same rules as the desktop
    /// clients' `normalize_host`.
    public static func normalizeHost(_ text: String) -> String? {
        let text = text.trimmingCharacters(in: .whitespaces)
        if text.contains("://") {
            return host(of: text).flatMap { isIPv4($0) ? nil : $0 }
        }
        let wildcard = text.hasPrefix("*.")
        var name = Substring(wildcard ? String(text.dropFirst(2)) : text)
        name = name.prefix { $0 != "/" && $0 != "?" && $0 != "#" }
        name = name.prefix { $0 != ":" }
        while name.hasSuffix(".") {
            name = name.dropLast()
        }
        let allowed = Set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-")
        guard !name.isEmpty, name.allSatisfy(allowed.contains), name.contains("."),
              !name.hasPrefix("."), !name.hasPrefix("-"), !isIPv4(name)
        else {
            return nil
        }
        let lower = name.lowercased()
        return wildcard ? "*." + lower : lower
    }

    /// Digits and dots only: an IPv4 address, never a domain. Decryption is chosen per domain, so an address is refused.
    private static func isIPv4<S: StringProtocol>(_ name: S) -> Bool {
        name.allSatisfy { $0 == "." || ("0"..."9").contains($0) }
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
        try copy.checkBudget(replacing: self, importedBytes: 0)
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
    case invalidHost
    case duplicateHost
    case tooMany
    /// The generated plugin would be over `DebugMappings.maxPluginBytes`.
    case totalTooLarge
    /// With the enabled plugins, over the engine's decryption budget (`EngineBudget.maxDecryptionBytes`).
    case decryptionBudgetFull

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
        case .invalidHost:
            return String(localized: "请填写域名，例如 api.example.com 或 *.example.com")
        case .duplicateHost:
            return String(localized: "列表里已有这个域名")
        case .tooMany:
            return String(localized: "已达到数量上限")
        case .totalTooLarge:
            return String(localized: "映射合计超过 4 MB（响应体按 Base64 计算）。请删除或缩小部分 Map Local 响应体。")
        case .decryptionBudgetFull:
            return String(localized: "映射和已启用的插件合计超过 8 MB。请减少映射响应体或停用一些插件。")
        }
    }
}
