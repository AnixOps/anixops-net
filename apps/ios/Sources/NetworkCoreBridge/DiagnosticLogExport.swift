import Foundation

/// The diagnostics report as a `.log` file the user can share (Settings → 导出日志): the same text as 复制诊断信息
/// (`DeveloperReport.text`: versions, extension memory and uptime, runtime status, the extension's event log,
/// engine metrics, crash reports), with a power section for heat and battery reports, run through `Redaction`
/// so addresses, URLs, accounts and keys never leave the device in it.
public enum DiagnosticLogExport {
    /// Folder under the temporary directory the files are written to; older exports there are removed.
    public static let folderName = "diagnostic-logs"

    /// `AnixOps-Net-20261114-083000.log` in local time.
    public static func fileName(at date: Date, timeZone: TimeZone = .current) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = timeZone
        formatter.dateFormat = "yyyyMMdd-HHmmss"
        return "AnixOps-Net-\(formatter.string(from: date)).log"
    }

    /// The redacted file contents. `power` holds lines such as `thermal state: serious`, written before the report.
    public static func contents(
        report: DeveloperReport,
        power: [String] = [],
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> String {
        var text = report.text
        if !power.isEmpty {
            text += "\npower:\n" + power.joined(separator: "\n")
        }
        return Redaction.redact(text, environment: environment) + "\n"
    }

    /// Writes `contents` to a new file in `directory`/`folderName` and returns its URL, removing earlier exports.
    public static func write(
        _ contents: String,
        at date: Date = Date(),
        in directory: URL = FileManager.default.temporaryDirectory
    ) throws -> URL {
        let folder = directory.appendingPathComponent(folderName, isDirectory: true)
        let manager = FileManager.default
        try manager.createDirectory(at: folder, withIntermediateDirectories: true)
        for old in (try? manager.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? []
        where old.pathExtension == "log" {
            try? manager.removeItem(at: old)
        }
        let url = folder.appendingPathComponent(fileName(at: date))
        try Data(contents.utf8).write(to: url, options: .atomic)
        return url
    }

    /// `ProcessInfo.ThermalState` as a word for the power section.
    public static func describe(thermalState: ProcessInfo.ThermalState) -> String {
        switch thermalState {
        case .nominal: return "nominal"
        case .fair: return "fair"
        case .serious: return "serious"
        case .critical: return "critical"
        @unknown default: return "unknown"
        }
    }
}
