import CryptoKit
import Foundation

/// The kernel's view of one plugin (crates/engine-embedded `networkcore_ios_mitm_inspect_plugin`): how much of it the
/// runtime supports and which remote files it needs. Loon, Surge modules and Quantumult X snippets all import through
/// the kernel, so this replaces reading section names here. The containing app produces it; this module only decodes
/// it, so it stays free of C symbols.
public struct MitmPluginInspection: Codable, Equatable {
    public struct Lines: Codable, Equatable {
        public var supported = 0
        public var partial = 0
        public var unsupported = 0
        public var deferred = 0

        public init(supported: Int = 0, partial: Int = 0, unsupported: Int = 0, deferred: Int = 0) {
            self.supported = supported
            self.partial = partial
            self.unsupported = unsupported
            self.deferred = deferred
        }
    }

    /// A remote file named by the plugin, as written in it.
    public struct Resource: Codable, Equatable {
        /// `script` (runs only after the user allows it) or `map_local_data` (a response body served as data).
        public var kind: String
        public var url: String
        public var line: Int

        public init(kind: String, url: String, line: Int) {
            self.kind = kind
            self.url = url
            self.line = line
        }

        public static let script = "script"
        public static let mapLocalData = "map_local_data"
    }

    public var name: String?
    /// `full`, `partial` or `unsupported`.
    public var level: String
    public var hostnames: [String]
    public var excludedHostnames: [String]
    public var lines: Lines
    public var resources: [Resource]
    /// One line per graded plugin line, for diagnostics.
    public var report: String

    enum CodingKeys: String, CodingKey {
        case name, level, hostnames, lines, resources, report
        case excludedHostnames = "excluded_hostnames"
    }

    public init(
        name: String? = nil,
        level: String,
        hostnames: [String] = [],
        excludedHostnames: [String] = [],
        lines: Lines = Lines(),
        resources: [Resource] = [],
        report: String = ""
    ) {
        self.name = name
        self.level = level
        self.hostnames = hostnames
        self.excludedHostnames = excludedHostnames
        self.lines = lines
        self.resources = resources
        self.report = report
    }

    public static func decode(_ data: Data) -> MitmPluginInspection? {
        try? JSONDecoder().decode(MitmPluginInspection.self, from: data)
    }

    /// Whether the runtime would do anything with the plugin.
    public var appliesAnything: Bool {
        lines.supported + lines.partial > 0 || !hostnames.isEmpty
    }
}

/// Remote `[Map Local]` bodies (and Quantumult X `echo-response` files) of the imported plugins, by URL as written in
/// the plugin. They are data, never code, so the app downloads them on import without asking; the tunnel gets the
/// ones the enabled plugins name through the tunnel profile and hands them to the runtime as `resource_assets`.
public struct MitmResourceLibrary: Codable, Equatable {
    /// Matches `MAP_LOCAL_REMOTE_DATA_MAX_BYTES` in crates/mitm-policy.
    public static let maxBytes = 256 * 1024
    /// Matches `MAX_RESOURCE_BYTES` in crates/engine-embedded/src/mitm.
    public static let maxActiveBytes = 2 * 1024 * 1024

    public private(set) var items: [String: Data] = [:]

    public init() {}

    public mutating func store(_ data: Data, for url: String) {
        guard data.count <= Self.maxBytes else {
            return
        }
        items[url] = data
    }

    /// Bodies for `urls`, in order, until `maxActiveBytes`; the rest are left out and their lines answer nothing.
    public func data(for urls: [String]) -> [String: Data] {
        var selected: [String: Data] = [:]
        var total = 0
        for url in urls where selected[url] == nil {
            guard let data = items[url], total + data.count <= Self.maxActiveBytes else {
                continue
            }
            selected[url] = data
            total += data.count
        }
        return selected
    }

    /// Drops bodies no plugin names any more.
    public mutating func keep(only urls: Set<String>) {
        items = items.filter { urls.contains($0.key) }
    }

    public static var defaultURL: URL {
        get throws {
            try FileManager.default
                .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
                .appendingPathComponent("mitm-resources.json")
        }
    }

    public static func load(from url: URL) throws -> MitmResourceLibrary {
        guard FileManager.default.fileExists(atPath: url.path) else {
            return MitmResourceLibrary()
        }
        return try JSONDecoder().decode(MitmResourceLibrary.self, from: Data(contentsOf: url))
    }

    public func save(to url: URL) throws {
        try JSONEncoder().encode(self).write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }

    /// The tunnel profile value under `TunnelProfileKeys.mitmResources`.
    public static func profileValue(_ data: [String: Data]) throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return try encoder.encode(data)
    }

    public static func data(fromProfileValue value: Data?) -> [String: Data] {
        value.flatMap { try? JSONDecoder().decode([String: Data].self, from: $0) } ?? [:]
    }

    /// Writes each body into `directory` as one file named by its digest and returns the `resource_assets` map the
    /// runtime expects. Files no longer listed are removed.
    public static func writeAssets(_ data: [String: Data], to directory: URL) throws -> [String: String] {
        let manager = FileManager.default
        try manager.createDirectory(at: directory, withIntermediateDirectories: true)
        var assets: [String: String] = [:]
        var keep: Set<String> = []
        for (url, body) in data {
            let name = SHA256.hash(data: body).map { String(format: "%02x", $0) }.joined() + ".bin"
            let file = directory.appendingPathComponent(name)
            if !manager.fileExists(atPath: file.path) {
                try body.write(to: file, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
            }
            assets[url] = file.path
            keep.insert(name)
        }
        for name in try manager.contentsOfDirectory(atPath: directory.path) where !keep.contains(name) {
            try? manager.removeItem(at: directory.appendingPathComponent(name))
        }
        return assets
    }
}
