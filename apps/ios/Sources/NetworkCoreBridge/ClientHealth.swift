import Foundation

/// Removes personal data from text that leaves the user's hands (crash reports, diagnostics): URLs, e-mail
/// addresses, IP addresses, host names, UUIDs and long key-like tokens. Same rules as the desktop clients'
/// `client-health` crate. Source locations (`App.swift:12`) and symbol names are kept.
public enum Redaction {
    private static let patterns: [(String, String)] = [
        (#"[A-Za-z][A-Za-z0-9+.\-]*://[^\s"'<>()]+"#, "<url>"),
        (#"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}"#, "<email>"),
        (#"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b"#, "<id>"),
        (#"\[[0-9A-Fa-f:.]*:[0-9A-Fa-f:.]*\](:\d+)?"#, "<ip>"),
        (#"\b(?:[0-9A-Fa-f]{1,4}:){7}[0-9A-Fa-f]{1,4}\b"#, "<ip>"),
        (#"(?<![\w:])(?:[0-9A-Fa-f]{1,4}:){1,6}:(?:[0-9A-Fa-f]{1,4}(?::[0-9A-Fa-f]{1,4})*)?"#, "<ip>"),
        (#"\b(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?\b"#, "<ip>"),
        (#"(?<![A-Za-z0-9+/=_\-])(?=[A-Za-z0-9+/=_\-]*\d)(?=[A-Za-z0-9+/=_\-]*[A-Za-z])[A-Za-z0-9+/=_\-]{20,}"#, "<secret>"),
        (#"\b(?:[A-Za-z0-9](?:[A-Za-z0-9\-]{0,61}[A-Za-z0-9])?\.)+(?!(?:swift|m|h|c|mm|json|plist|txt|log|dylib|framework|appex|app)\b)[A-Za-z]{2,24}(?::\d+)?\b"#, "<host>"),
    ]

    private static let rules: [(NSRegularExpression, String)] = patterns.compactMap { pattern, template in
        (try? NSRegularExpression(pattern: pattern)).map { ($0, template) }
    }

    public static func redact(_ text: String) -> String {
        var result = text
        for (expression, template) in rules {
            let range = NSRange(result.startIndex..., in: result)
            result = expression.stringByReplacingMatches(in: result, range: range, withTemplate: template)
        }
        return result
    }
}

/// Crash reports kept in the app group container (or Application Support), redacted before they are written.
/// They come from MetricKit's crash diagnostics and from uncaught Objective-C exceptions; nothing is sent
/// anywhere: the reports are added to the diagnostics the user copies. Turning them off deletes the kept ones.
public final class CrashReportStore: @unchecked Sendable {
    public static let enabledKey = "crash-reports-enabled"
    public static let maxReports = 10

    public let directory: URL
    private let defaults: UserDefaults
    private let lock = NSLock()

    public init(directory: URL, defaults: UserDefaults = .standard) {
        self.directory = directory
        self.defaults = defaults
    }

    /// The store under `Library/Application Support/crashes` of the app group container when there is one.
    public static func standard(appGroup: String? = nil) -> CrashReportStore {
        let base = appGroup.flatMap { FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: $0) }
            ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        let defaults = appGroup.flatMap { UserDefaults(suiteName: $0) } ?? .standard
        return CrashReportStore(directory: base.appendingPathComponent("crashes", isDirectory: true), defaults: defaults)
    }

    /// On unless the user turned it off.
    public var isEnabled: Bool {
        get { defaults.object(forKey: Self.enabledKey) as? Bool ?? true }
        set {
            defaults.set(newValue, forKey: Self.enabledKey)
            if !newValue {
                lock.lock()
                try? FileManager.default.removeItem(at: directory)
                lock.unlock()
            }
        }
    }

    /// Writes one report (redacted here) and keeps the newest `maxReports`.
    @discardableResult
    public func write(_ report: String, at date: Date = Date()) -> URL? {
        guard isEnabled else {
            return nil
        }
        lock.lock()
        defer { lock.unlock() }
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let millis = UInt64(max(0, date.timeIntervalSince1970) * 1000)
        var url = directory.appendingPathComponent("crash-\(millis).txt")
        var suffix = 1
        while FileManager.default.fileExists(atPath: url.path) {
            url = directory.appendingPathComponent("crash-\(millis)-\(suffix).txt")
            suffix += 1
        }
        guard (try? Data(Redaction.redact(report).utf8).write(to: url, options: .atomic)) != nil else {
            return nil
        }
        var kept = pendingUnlocked()
        while kept.count > Self.maxReports {
            try? FileManager.default.removeItem(at: kept.removeFirst())
        }
        return url
    }

    /// Kept reports, oldest first.
    public func pending() -> [URL] {
        lock.lock()
        defer { lock.unlock() }
        return pendingUnlocked()
    }

    private func pendingUnlocked() -> [URL] {
        let names = (try? FileManager.default.contentsOfDirectory(atPath: directory.path)) ?? []
        return names
            .filter { $0.hasPrefix("crash-") && $0.hasSuffix(".txt") }
            .sorted { $0.localizedStandardCompare($1) == .orderedAscending }
            .map { directory.appendingPathComponent($0) }
    }

    /// The kept reports for the diagnostics export.
    public func export() -> String {
        pending().compactMap { url in
            (try? String(contentsOf: url, encoding: .utf8)).map { "--- \(url.lastPathComponent) ---\n\(Redaction.redact($0))" }
        }
        .joined(separator: "\n")
    }

    /// The report for an uncaught Objective-C exception.
    public static func exceptionReport(app: String, version: String, name: String, reason: String?, symbols: [String]) -> String {
        """
        \(app) \(version) crash report
        kind: uncaught exception
        exception: \(name)
        reason: \(reason ?? "none")
        backtrace:
        \(symbols.joined(separator: "\n"))
        """
    }

    /// The report for one MetricKit crash diagnostic. `callStack` is the diagnostic's call stack tree JSON: binary
    /// names and offsets (the redaction also replaces the binary UUIDs).
    public static func diagnosticReport(
        app: String,
        version: String,
        osVersion: String,
        exceptionType: Int?,
        exceptionCode: Int?,
        signal: Int?,
        terminationReason: String?,
        callStack: String
    ) -> String {
        """
        \(app) \(version) crash report
        kind: metrickit crash diagnostic
        os: \(osVersion)
        exception type: \(exceptionType.map(String.init) ?? "none")
        exception code: \(exceptionCode.map(String.init) ?? "none")
        signal: \(signal.map(String.init) ?? "none")
        termination reason: \(terminationReason ?? "none")
        call stack:
        \(callStack)
        """
    }
}

/// Install integrity at launch: every app extension the build ships is present and carries the app's version
/// and build. A damaged or half-updated install (an extension left from another build) is the case this catches;
/// the code signature itself is the operating system's job.
public enum InstallIntegrity: Equatable {
    case verified(extensions: Int)
    case damaged(String)

    public var state: String {
        switch self {
        case .verified: return "verified"
        case .damaged: return "damaged"
        }
    }

    public var detail: String {
        switch self {
        case let .verified(count): return "\(count) extensions match the app"
        case let .damaged(reason): return reason
        }
    }

    /// Checks the extensions (name and Info.plist, nil when unreadable) against the app's Info.plist; one of them
    /// must be the packet tunnel.
    public static func check(appInfo: [String: Any], extensions: [(name: String, info: [String: Any]?)]) -> InstallIntegrity {
        let version = appInfo["CFBundleShortVersionString"] as? String
        let build = appInfo["CFBundleVersion"] as? String
        var tunnel = false
        for (name, info) in extensions {
            guard let info = info else {
                return .damaged("\(name) has no readable Info.plist")
            }
            if info["CFBundleShortVersionString"] as? String != version || info["CFBundleVersion"] as? String != build {
                return .damaged("\(name) is from another build")
            }
            tunnel = tunnel || isPacketTunnel(info)
        }
        guard tunnel else {
            return .damaged("the packet tunnel extension is missing")
        }
        return .verified(extensions: extensions.count)
    }

    /// An iOS packet tunnel app extension or a macOS packet tunnel system extension.
    static func isPacketTunnel(_ info: [String: Any]) -> Bool {
        let point = (info["NSExtension"] as? [String: Any])?["NSExtensionPointIdentifier"] as? String
        let providers = (info["NetworkExtension"] as? [String: Any])?["NEProviderClasses"] as? [String: Any]
        return point == "com.apple.networkextension.packet-tunnel"
            || providers?["com.apple.networkextension.packet-tunnel"] != nil
    }

    /// Checks the running app's bundle.
    public static func check(bundle: Bundle = .main) -> InstallIntegrity {
        let extensions = embeddedExtensions(of: bundle).map { url in
            (name: url.lastPathComponent, info: Bundle(url: url)?.infoDictionary)
        }
        return check(appInfo: bundle.infoDictionary ?? [:], extensions: extensions)
    }

    /// The app extensions (`PlugIns/*.appex`) and system extensions (`Library/SystemExtensions/*.systemextension`)
    /// of `bundle`.
    public static func embeddedExtensions(of bundle: Bundle) -> [URL] {
        var urls: [URL] = []
        if let plugIns = bundle.builtInPlugInsURL {
            urls += ((try? FileManager.default.contentsOfDirectory(at: plugIns, includingPropertiesForKeys: nil)) ?? [])
                .filter { $0.pathExtension == "appex" }
        }
        let system = bundle.bundleURL.appendingPathComponent("Contents/Library/SystemExtensions", isDirectory: true)
        urls += ((try? FileManager.default.contentsOfDirectory(at: system, includingPropertiesForKeys: nil)) ?? [])
            .filter { $0.pathExtension == "systemextension" }
        return urls.sorted { $0.lastPathComponent < $1.lastPathComponent }
    }
}

/// The health section of the diagnostics the user copies.
public enum ClientHealthReport {
    public static func text(integrity: InstallIntegrity?, crashReports: CrashReportStore) -> String {
        var lines = [
            "install integrity: \(integrity.map { "\($0.state) (\($0.detail))" } ?? "not checked")",
            "crash reports: \(crashReports.isEnabled ? "on" : "off") (\(crashReports.pending().count) kept)",
        ]
        let reports = crashReports.export()
        if !reports.isEmpty {
            lines.append("crash reports:")
            lines.append(reports)
        }
        return lines.joined(separator: "\n")
    }
}
