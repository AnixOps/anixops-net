import NetworkCoreBridge
import SwiftUI

/// 添加网络 (docs/mesh-ui.md): name, secret, peers, virtual address and encryption. The region is assigned by the
/// network. The primary action 加入 sits at the bottom across the full width (P3).
struct MeshNetworkSheet: View {
    /// Names of the saved networks; a new network may not reuse one.
    let taken: Set<String>
    let join: (MeshNetwork, String) async -> Bool
    @Environment(\.brand) private var brand
    @Environment(\.dismiss) private var dismiss
    @State private var draft = MeshNetworkDraft()
    @State private var error: String?
    @State private var saving = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("名称", text: $draft.name)
                        .plainTextEntry()
                        .accessibilityIdentifier("mesh-add-name")
                    SecureField("密钥", text: $draft.secret)
                        .accessibilityIdentifier("mesh-add-secret")
                }
                .listRowBackground(brand.surface)
                Section {
                    TextField("对端地址", text: $draft.peers, prompt: Text(verbatim: "203.0.113.10:11010"), axis: .vertical)
                        .lineLimit(2...6)
                        .plainTextEntry(url: true)
                        .accessibilityIdentifier("mesh-add-peers")
                } header: {
                    Text("对端地址")
                } footer: {
                    Text("每行一个地址，可带 tcp:// 或 udp://，默认端口 11010。")
                }
                .listRowBackground(brand.surface)
                Section {
                    TextField("虚拟地址", text: $draft.address, prompt: Text("自动分配"))
                        .plainTextEntry(url: true)
                        .accessibilityIdentifier("mesh-add-address")
                    Picker("加密模式", selection: $draft.security) {
                        Text("前向保密").tag(MeshNetworkDraft.Security.forwardSecrecy)
                        Text("标准").tag(MeshNetworkDraft.Security.standard)
                    }
                } footer: {
                    VStack(alignment: .leading, spacing: 6) {
                        if let error {
                            Label(error, systemImage: "exclamationmark.circle.fill")
                                .foregroundStyle(brand.danger)
                        }
                        if draft.security == .standard {
                            Text("标准模式未启用前向保密。")
                        }
                    }
                }
                .listRowBackground(brand.surface)
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .safeAreaInset(edge: .bottom, spacing: 0) {
                Button {
                    submit()
                } label: {
                    Text("加入")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(BrandButtonStyle())
                .disabled(saving)
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .background(brand.bg)
                .accessibilityIdentifier("mesh-add-join")
            }
            .navigationTitle("添加网络")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
            }
        }
    }

    private func submit() {
        if let problem = draft.problem {
            error = Self.text(problem)
            return
        }
        guard let network = draft.network() else {
            error = String(localized: "无法生成网络配置")
            return
        }
        guard !taken.contains(network.name) else {
            error = String(localized: "已存在同名网络。")
            return
        }
        error = nil
        saving = true
        Task {
            let joined = await join(network, draft.secret)
            saving = false
            if joined {
                dismiss()
            } else {
                error = String(localized: "无法把网络密钥保存到钥匙串")
            }
        }
    }

    static func text(_ problem: MeshNetworkDraft.Problem) -> String {
        switch problem {
        case .name:
            return String(localized: "名称须为 1–64 个字符，不含斜杠。")
        case .secret:
            return String(localized: "缺少密钥。")
        case .peers:
            return String(localized: "缺少对端地址。")
        case let .peer(line):
            return String(localized: "对端地址无效：\(line)")
        case .address:
            return String(localized: "虚拟地址无效，格式如 10.144.144.5/24。")
        }
    }
}
