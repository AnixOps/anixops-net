import NetworkCoreBridge
import SwiftUI

/// 节点体检 of one node: started by the user, run in the app through the node's own protocol (`NodeCheckupFFI`).
struct NodeCheckupSheet: View {
    let node: NativeImportReport.Node
    /// Runs the checkup; true adds the download suite.
    let run: (Bool) async -> Result<NodeCheckupReport, NodeCheckupFailure>
    @Environment(\.brand) private var brand
    @Environment(\.dismiss) private var dismiss
    @AppStorage("checkup.throughput") private var throughput = false
    @State private var running = false
    @State private var result: Result<NodeCheckupReport, NodeCheckupFailure>?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Toggle("测下载速度", isOn: $throughput)
                        .disabled(running)
                    Button {
                        start()
                    } label: {
                        HStack(spacing: 8) {
                            if running {
                                ProgressView()
                                    .controlSize(.small)
                                Text("正在体检…")
                            } else if result == nil {
                                Text("开始体检")
                            } else {
                                Text("再测一次")
                            }
                        }
                    }
                    .disabled(running)
                } header: {
                    Text(verbatim: node.displayName)
                } footer: {
                    if throughput {
                        Text("经该节点访问 1.1.1.1 和 www.gstatic.com 测延迟，并从 speed.cloudflare.com 下载约 8 秒测速度（最多 200 MB，会消耗节点流量）。仅在点击开始时运行，不经过隧道。")
                    } else {
                        Text("经该节点访问 1.1.1.1 和 www.gstatic.com 各 5 次，测握手、请求延迟、抖动和丢包。仅在点击开始时运行，不经过隧道。")
                    }
                }
                .listRowBackground(brand.surface)

                switch result {
                case nil:
                    EmptyView()
                case let .failure(failure)?:
                    Section {
                        Label(failure.message, systemImage: "exclamationmark.circle.fill")
                            .foregroundStyle(brand.danger)
                    }
                    .listRowBackground(brand.surface)
                case let .success(report)?:
                    latencySection(report.latency)
                    if let throughput = report.throughput {
                        throughputSection(throughput)
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle("节点体检")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("完成") {
                        dismiss()
                    }
                }
            }
        }
    }

    private func start() {
        running = true
        let withThroughput = throughput
        Task {
            result = await run(withThroughput)
            running = false
        }
    }

    private func latencySection(_ latency: NodeCheckupReport.Latency) -> some View {
        Section {
            metric("握手延迟", Self.millis(latency.handshakeMs))
            metric("首次握手", Self.millis(latency.coldHandshakeMs))
            if latency.samples > 1 {
                metric("后续握手", Self.millis(latency.warmHandshakeMs))
            }
            metric("请求延迟", Self.millis(latency.requestMs))
            metric("抖动", Self.millis(latency.requestJitterMs))
            metric("丢包", Self.percent(max(latency.handshakeLossPct, latency.requestLossPct)), warn: latency.requestLossPct > 0)
            if let failure = NodeCheckupReport.failureText(latency.failure) {
                metric("失败原因", failure, warn: true)
            }
        } header: {
            Text("延迟")
        }
        .listRowBackground(brand.surface)
    }

    private func throughputSection(_ throughput: NodeCheckupReport.Throughput) -> some View {
        Section {
            metric("平均速度", Self.mbps(throughput.mbps))
            metric("峰值速度", Self.mbps(throughput.peakMbps))
            metric("下载量", ByteCountFormatter.string(fromByteCount: Int64(clamping: throughput.bytes), countStyle: .file))
            metric("空闲延迟", Self.millis(throughput.idleMs))
            metric("负载下延迟", Self.millis(throughput.loadedMs))
            metric("延迟增加", Self.millis(throughput.bloatMs), warn: (throughput.bloatMs ?? 0) > 100)
            if let failure = NodeCheckupReport.failureText(throughput.failure) {
                metric("失败原因", failure, warn: true)
            }
        } header: {
            Text("下载")
        } footer: {
            if throughput.capped {
                Text("达到 200 MB 上限，提前结束。")
            }
        }
        .listRowBackground(brand.surface)
    }

    private func metric(_ title: LocalizedStringKey, _ value: String, warn: Bool = false) -> some View {
        LabeledContent {
            Text(verbatim: value)
                .monospacedDigit()
                .foregroundStyle(warn ? brand.danger : brand.text2)
        } label: {
            Text(title)
                .foregroundStyle(brand.text)
        }
    }

    static func millis(_ value: Double?) -> String {
        guard let value else {
            return "—"
        }
        return value < 10 ? String(format: "%.1f ms", value) : String(format: "%.0f ms", value)
    }

    static func percent(_ value: Double) -> String {
        String(format: "%.0f%%", value)
    }

    static func mbps(_ value: Double) -> String {
        value < 10 ? String(format: "%.2f Mbps", value) : String(format: "%.1f Mbps", value)
    }
}
