import NetworkCoreBridge
import SwiftUI
import UIKit

/// "导出日志": asks the running extension for fresh diagnostics, writes the redacted report as a `.log` file
/// (`DiagnosticLogExport`) and opens the system share sheet with it (AirDrop, Files, Mail, chat apps).
struct LogExportButton: View {
    var tunnel: TunnelController
    @State private var exporting = false
    @State private var file: ExportedLog?
    @State private var failed = false

    var body: some View {
        Button {
            Task { await export() }
        } label: {
            HStack {
                Label("导出日志", systemImage: "square.and.arrow.up")
                if exporting {
                    Spacer()
                    ProgressView()
                }
            }
        }
        .disabled(exporting)
        .sheet(item: $file) { file in
            ActivityView(items: [file.url])
                .presentationDetents([.medium, .large])
        }
        .alert("日志导出失败", isPresented: $failed) {
            Button("好", role: .cancel) {}
        }
    }

    @MainActor
    private func export() async {
        exporting = true
        defer { exporting = false }
        tunnel.refreshDiagnostics()
        tunnel.refreshLastDisconnectError()
        // The extension answers asynchronously; give it a moment so the file has the latest events.
        if tunnel.isConnected {
            try? await Task.sleep(for: .milliseconds(600))
        }
        let report = DeveloperReport(
            appVersion: DeveloperMode.appVersion,
            appBuild: DeveloperMode.appBuild,
            systemVersion: "\(UIDevice.current.systemName) \(UIDevice.current.systemVersion)",
            connection: ConnectPhase.describe(tunnel.connection),
            outboundKind: tunnel.outboundKind,
            lastDisconnectError: tunnel.lastDisconnectError,
            diagnostics: tunnel.diagnostics,
            generatedAt: Date(),
            health: ClientHealthMonitor.shared.reportText
        )
        let contents = DiagnosticLogExport.contents(report: report, power: Self.powerLines(tunnel: tunnel))
        do {
            file = ExportedLog(url: try DiagnosticLogExport.write(contents))
        } catch {
            failed = true
        }
    }

    /// Heat and battery facts for the power section: thermal state, Low Power Mode, battery, connected time.
    @MainActor
    static func powerLines(tunnel: TunnelController) -> [String] {
        let device = UIDevice.current
        let wasMonitoring = device.isBatteryMonitoringEnabled
        device.isBatteryMonitoringEnabled = true
        defer { device.isBatteryMonitoringEnabled = wasMonitoring }
        let battery: String
        switch device.batteryState {
        case .charging: battery = "charging"
        case .full: battery = "full"
        case .unplugged: battery = "unplugged"
        default: battery = "unknown"
        }
        var lines = [
            "thermal state: \(DiagnosticLogExport.describe(thermalState: ProcessInfo.processInfo.thermalState))",
            "low power mode: \(ProcessInfo.processInfo.isLowPowerModeEnabled)",
            "battery: \(device.batteryLevel < 0 ? "unknown" : "\(Int(device.batteryLevel * 100))%") \(battery)",
        ]
        if let since = tunnel.connectedSince {
            lines.append("connected for: \(DeveloperReport.duration(Date().timeIntervalSince(since)))")
        }
        return lines
    }
}

private struct ExportedLog: Identifiable {
    let url: URL
    var id: String { url.path }
}

/// The system share sheet.
private struct ActivityView: UIViewControllerRepresentable {
    let items: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: items, applicationActivities: nil)
    }

    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}
