import NetworkCoreBridge
import SwiftUI

extension DiagnosticLevel {
    var label: LocalizedStringKey {
        switch self {
        case .debug: return "调试"
        case .info: return "信息"
        case .warning: return "警告"
        case .error: return "错误"
        }
    }

    var symbol: String {
        switch self {
        case .debug: return "ant"
        case .info: return "info.circle"
        case .warning: return "exclamationmark.triangle"
        case .error: return "xmark.octagon"
        }
    }
}

extension DiagnosticModule {
    var label: LocalizedStringKey {
        switch self {
        case .tunnel: return "隧道"
        case .runtime: return "内核"
        case .network: return "网络"
        case .rules: return "规则"
        case .dns: return "DNS"
        case .mitm: return "HTTPS 解密"
        case .scripts: return "脚本"
        case .memory: return "内存"
        }
    }
}

/// The keys the event log's filter is remembered under, shared by iOS developer mode and the macOS Activity page.
enum DiagnosticFilterKeys {
    static let minimum = "diagnostics.minimum-level"
    static let module = "diagnostics.module"

    static func modules(_ raw: String) -> Set<DiagnosticModule> {
        DiagnosticModule(rawValue: raw).map { [$0] } ?? []
    }
}

/// The two pickers: level and above, and one module or all.
struct DiagnosticFilterPickers: View {
    @Binding var minimum: DiagnosticLevel
    @Binding var module: String

    var body: some View {
        Picker("级别", selection: $minimum) {
            ForEach(DiagnosticLevel.allCases) { level in
                Text(level.label).tag(level)
            }
        }
        Picker("模块", selection: $module) {
            Text("全部模块").tag("")
            ForEach(DiagnosticModule.allCases) { module in
                Text(module.label).tag(module.rawValue)
            }
        }
    }
}

/// One line of the event log: time, level glyph, module and the message.
struct DiagnosticEventRow: View {
    let event: TunnelDiagnosticEvent

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: event.level.symbol)
                .foregroundStyle(color)
                .accessibilityLabel(Text(event.level.label))
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(event.time, format: .dateTime.hour().minute().second())
                    Text(event.module.label)
                }
                .font(.caption2)
                .foregroundStyle(.secondary)
                Text(event.message)
                    .font(.caption.monospaced())
                    .textSelection(.enabled)
            }
        }
    }

    private var color: Color {
        switch event.level {
        case .debug: return .secondary
        case .info: return .accentColor
        case .warning: return .orange
        case .error: return .red
        }
    }
}
