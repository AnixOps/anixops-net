import Foundation

/// Removes personal data from text that leaves the user's hands (crash reports, diagnostics). A line-for-line port
/// of the desktop clients' `client-health/src/redact.rs`, which is the reference: each run of "word" characters is
/// looked at on its own and replaced when it is a URL (`<url>`), an e-mail address (`<email>`), an IPv4 or IPv6
/// address (`<ip>`), a host name (`<host>`), a UUID (`<id>`) or a long key-like token (`<secret>`, including a
/// `key=` prefix). The home directory and account name become `~` and `<user>` first. Source file names
/// (`App.swift:12`) and symbol paths (`std::panicking`) are kept.
public enum Redaction {
    /// File extensions that mark a token as a file name rather than a host. Same list as redact.rs.
    static let fileExtensions: Set<String> = [
        "rs", "toml", "json", "txt", "swift", "so", "dll", "exe", "dylib", "md", "log", "ts", "tsx", "js", "c", "h",
    ]

    private static let extraTokenScalars = Set(".-_:/@%+=?&#~[]".unicodeScalars)
    private static let secretSeparators = Set("/:?&#@[]".unicodeScalars)

    /// `text` with personal data replaced, using this process's environment for the home directory and account.
    public static func redact(_ text: String) -> String {
        redact(text, environment: ProcessInfo.processInfo.environment)
    }

    /// `text` with personal data replaced; `environment` supplies HOME, USERPROFILE, USER, USERNAME and LOGNAME.
    public static func redact(_ text: String, environment: [String: String]) -> String {
        let text = redactAccount(text, environment: environment)
        var out = String.UnicodeScalarView()
        var token = String.UnicodeScalarView()
        func flush() {
            if !token.isEmpty {
                out.append(contentsOf: redactToken(String(token)).unicodeScalars)
                token.removeAll(keepingCapacity: true)
            }
        }
        for scalar in text.unicodeScalars {
            if isTokenScalar(scalar) {
                token.append(scalar)
            } else {
                flush()
                out.append(scalar)
            }
        }
        flush()
        return String(out)
    }

    /// Rust's `char::is_alphanumeric`: the Alphabetic property or a Nd, Nl or No general category.
    static func isAlphanumeric(_ scalar: Unicode.Scalar) -> Bool {
        if scalar.properties.isAlphabetic {
            return true
        }
        switch scalar.properties.generalCategory {
        case .decimalNumber, .letterNumber, .otherNumber:
            return true
        default:
            return false
        }
    }

    /// Characters that belong to one token. `:` stays inside so `host:port`, `[v6]:port` and URLs stay whole.
    static func isTokenScalar(_ scalar: Unicode.Scalar) -> Bool {
        isAlphanumeric(scalar) || extraTokenScalars.contains(scalar)
    }

    private static func isASCIIAlphanumeric(_ byte: UInt8) -> Bool {
        isASCIIDigit(byte) || isASCIIAlphabetic(byte)
    }

    private static func isASCIIDigit(_ byte: UInt8) -> Bool {
        (0x30...0x39).contains(byte)
    }

    private static func isASCIIAlphabetic(_ byte: UInt8) -> Bool {
        (0x41...0x5A).contains(byte) || (0x61...0x7A).contains(byte)
    }

    private static func isASCIIHexDigit(_ byte: UInt8) -> Bool {
        isASCIIDigit(byte) || (0x41...0x46).contains(byte) || (0x61...0x66).contains(byte)
    }

    private static func trimStart(_ text: String, _ characters: Set<Unicode.Scalar>) -> String {
        String(String.UnicodeScalarView(text.unicodeScalars.drop { characters.contains($0) }))
    }

    private static func trimEnd(_ text: String, _ characters: Set<Unicode.Scalar>) -> String {
        var scalars = Array(text.unicodeScalars)
        while let last = scalars.last, characters.contains(last) {
            scalars.removeLast()
        }
        return String(String.UnicodeScalarView(scalars))
    }

    /// Splits at the first `separator`, like Rust's `split_once`.
    private static func splitOnce(_ text: String, _ separator: Unicode.Scalar) -> (String, String)? {
        let scalars = text.unicodeScalars
        guard let at = scalars.firstIndex(of: separator) else {
            return nil
        }
        return (String(scalars[..<at]), String(scalars[scalars.index(after: at)...]))
    }

    /// Splits at the last `separator`, like Rust's `rsplit_once`.
    private static func rsplitOnce(_ text: String, _ separator: Unicode.Scalar) -> (String, String)? {
        let scalars = text.unicodeScalars
        guard let at = scalars.lastIndex(of: separator) else {
            return nil
        }
        return (String(scalars[..<at]), String(scalars[scalars.index(after: at)...]))
    }

    /// Splits at every scalar in `separators`, keeping empty parts, like Rust's `split`.
    private static func split(_ text: String, _ separators: Set<Unicode.Scalar>) -> [String] {
        text.unicodeScalars
            .split(omittingEmptySubsequences: false, whereSeparator: { separators.contains($0) })
            .map { String(String.UnicodeScalarView($0)) }
    }

    /// The home directory and the account name.
    static func redactAccount(_ text: String, environment: [String: String]) -> String {
        var text = text
        for key in ["HOME", "USERPROFILE"] {
            if let home = environment[key], home.utf8.count > 3 {
                text = text.replacingOccurrences(of: home, with: "~", options: .literal)
            }
        }
        for key in ["USER", "USERNAME", "LOGNAME"] {
            if let user = environment[key], user.utf8.count >= 3 {
                text = replaceWord(text, user, with: "<user>")
            }
        }
        return text
    }

    /// Replaces `word` where it is not part of a longer alphanumeric run.
    static func replaceWord(_ text: String, _ word: String, with replacement: String) -> String {
        var out = ""
        var rest = Substring(text)
        while let range = rest.range(of: word, options: .literal) {
            let before = rest[..<range.lowerBound].unicodeScalars.last
            let after = rest[range.upperBound...].unicodeScalars.first
            let bounded = !(before.map(isAlphanumeric) ?? false) && !(after.map(isAlphanumeric) ?? false)
            out += rest[..<range.lowerBound]
            out += bounded ? replacement : word
            rest = rest[range.upperBound...]
        }
        out += rest
        return out
    }

    static func redactToken(_ token: String) -> String {
        // Keep trailing sentence punctuation outside the token.
        let core = trimEnd(token, [".", ":", ","])
        let tail = String(String.UnicodeScalarView(token.unicodeScalars.dropFirst(core.unicodeScalars.count)))
        return (classify(core) ?? core) + tail
    }

    static func classify(_ token: String) -> String? {
        if token.isEmpty {
            return nil
        }
        if token.contains("://") {
            return "<url>"
        }
        if isEmail(token) {
            return "<email>"
        }
        if isIPLike(token) {
            return "<ip>"
        }
        if isUUID(token) {
            return "<id>"
        }
        if isSecret(token) {
            return "<secret>"
        }
        if isHost(token) {
            return "<host>"
        }
        return nil
    }

    static func isEmail(_ token: String) -> Bool {
        guard let parts = splitOnce(token, "@") else {
            return false
        }
        let (user, domain) = parts
        return !user.isEmpty && domain.contains(".") && !domain.hasPrefix(".")
    }

    /// IPv4 with an optional port, IPv6 (bracketed or bare) with an optional port.
    static func isIPLike(_ token: String) -> Bool {
        let host = trimEnd(trimStart(stripPort(token), ["["]), ["]"])
        return parsesAsAddress(host)
    }

    /// Strict dotted-quad IPv4 or IPv6, as Rust's `Ipv4Addr` and `Ipv6Addr` parse them.
    static func parsesAsAddress(_ text: String) -> Bool {
        var v4 = in_addr()
        var v6 = in6_addr()
        return text.withCString { inet_pton(AF_INET, $0, &v4) == 1 || inet_pton(AF_INET6, $0, &v6) == 1 }
    }

    static func stripPort(_ token: String) -> String {
        if token.hasPrefix("[") {
            let rest = String(token.unicodeScalars.dropFirst())
            return splitOnce(rest, "]").map { $0.0 } ?? token
        }
        if let parts = rsplitOnce(token, ":"),
           !parts.0.unicodeScalars.contains(":"),
           !parts.1.isEmpty,
           parts.1.utf8.allSatisfy(isASCIIDigit) {
            return parts.0
        }
        return token
    }

    static func isUUID(_ token: String) -> Bool {
        let groups = split(token, ["-"])
        return groups.count == 5
            && zip([8, 4, 4, 4, 12], groups).allSatisfy { length, group in
                group.utf8.count == length && group.utf8.allSatisfy(isASCIIHexDigit)
            }
    }

    /// 20 or more base64/hex-like characters mixing letters and digits: keys, passwords, tokens.
    static func isSecret(_ token: String) -> Bool {
        let core = trimStart(token, ["/", "="])
        let run = split(core, secretSeparators).first { $0.utf8.count >= 20 } ?? ""
        let bytes = run.utf8
        return bytes.count >= 20
            && bytes.allSatisfy { isASCIIAlphanumeric($0) || "+=_-.".utf8.contains($0) }
            && bytes.contains(where: isASCIIDigit)
            && bytes.contains(where: isASCIIAlphabetic)
            && !run.contains("..")
            && !looksLikeRustSymbol(run)
    }

    /// Mangled or hashed Rust symbols in backtraces (`h1a2b3c4d5e6f7a8b`) are kept.
    static func looksLikeRustSymbol(_ run: String) -> Bool {
        let bytes = Array(run.utf8)
        return bytes.count == 17 && bytes[0] == UInt8(ascii: "h") && bytes.dropFirst().allSatisfy(isASCIIHexDigit)
    }

    /// `name.tld` or `name.tld:port`, where the last label is alphabetic and not a file extension.
    static func isHost(_ token: String) -> Bool {
        if token.contains("::") || token.unicodeScalars.contains("/") || token.unicodeScalars.contains("@") {
            return false
        }
        let labels = split(stripPort(token), ["."])
        guard labels.count >= 2, !labels.contains(where: \.isEmpty), let tld = labels.last else {
            return false
        }
        return tld.utf8.count >= 2
            && tld.utf8.allSatisfy(isASCIIAlphabetic)
            && !fileExtensions.contains(tld.lowercased())
            && labels.allSatisfy { label in
                label.unicodeScalars.allSatisfy { isAlphanumeric($0) || $0 == "-" || $0 == "_" }
            }
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
