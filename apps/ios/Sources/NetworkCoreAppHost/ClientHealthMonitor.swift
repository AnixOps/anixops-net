import Foundation
import MetricKit
import NetworkCoreBridge

/// Crash reports and the install check for the iOS and macOS apps.
///
/// MetricKit hands the app the crash diagnostics of its earlier runs (usually at the next launch); uncaught
/// Objective-C exceptions are written as they happen. Both go through `CrashReportStore`, which redacts them and
/// keeps them on this device for the diagnostics the user copies; turning crash reports off in Settings stops
/// that and deletes the kept ones. Nothing is uploaded.
final class ClientHealthMonitor: NSObject, MXMetricManagerSubscriber, @unchecked Sendable {
    static let shared = ClientHealthMonitor()

    let crashReports = CrashReportStore.standard()
    private(set) var integrity: InstallIntegrity?
    private var started = false

    private static var appVersion: String {
        let info = Bundle.main.infoDictionary
        let version = info?["CFBundleShortVersionString"] as? String ?? "unknown"
        let build = info?["CFBundleVersion"] as? String ?? "unknown"
        return "\(version) (\(build))"
    }

    /// Subscribes to MetricKit, installs the exception handler and checks the install. Called once at launch.
    func start() {
        guard !started else {
            return
        }
        started = true
        MXMetricManager.shared.add(self)
        NSSetUncaughtExceptionHandler { exception in
            ClientHealthMonitor.shared.crashReports.write(CrashReportStore.exceptionReport(
                app: "AnixOps Net",
                version: ClientHealthMonitor.appVersion,
                name: exception.name.rawValue,
                reason: exception.reason,
                symbols: exception.callStackSymbols
            ))
        }
        DispatchQueue.global(qos: .utility).async {
            let result = InstallIntegrity.check()
            DispatchQueue.main.async {
                self.integrity = result
            }
        }
    }

    /// The health section of the diagnostics report.
    var reportText: String {
        ClientHealthReport.text(integrity: integrity, crashReports: crashReports)
    }

    func didReceive(_ payloads: [MXDiagnosticPayload]) {
        for payload in payloads {
            for crash in payload.crashDiagnostics ?? [] {
                let callStack = String(decoding: crash.callStackTree.jsonRepresentation(), as: UTF8.self)
                crashReports.write(CrashReportStore.diagnosticReport(
                    app: "AnixOps Net",
                    version: crash.metaData.applicationBuildVersion,
                    osVersion: crash.metaData.osVersion,
                    exceptionType: crash.exceptionType?.intValue,
                    exceptionCode: crash.exceptionCode?.intValue,
                    signal: crash.signal?.intValue,
                    terminationReason: crash.terminationReason,
                    callStack: callStack
                ), at: payload.timeStampEnd)
            }
        }
    }
}
