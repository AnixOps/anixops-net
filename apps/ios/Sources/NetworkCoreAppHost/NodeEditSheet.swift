import NetworkCoreBridge
import SwiftUI

/// Edits a node's name, server and port. Credentials and transport settings stay as imported.
struct NodeEditSheet: View {
    let node: NativeImportReport.Node
    let save: (NativeImportReport.Node) async -> Bool
    @Environment(\.brand) private var brand
    @Environment(\.dismiss) private var dismiss
    @State private var name: String
    @State private var server: String
    @State private var port: String
    @State private var error: String?
    @State private var saving = false

    init(node: NativeImportReport.Node, save: @escaping (NativeImportReport.Node) async -> Bool) {
        self.node = node
        self.save = save
        _name = State(initialValue: node.name)
        _server = State(initialValue: node.server)
        _port = State(initialValue: String(node.port))
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("名称", text: $name)
                    TextField("服务器", text: $server)
                        .plainTextEntry(url: true)
                    TextField("端口", text: $port)
                    #if os(iOS)
                        .keyboardType(.numberPad)
                    #endif
                } header: {
                    Text(ProtocolBadge.spokenName(for: node.protocol))
                } footer: {
                    VStack(alignment: .leading, spacing: 6) {
                        if let error {
                            Label(error, systemImage: "exclamationmark.circle.fill")
                                .foregroundStyle(brand.danger)
                        }
                        Text("密码、密钥和传输设置保持导入时的值。订阅更新后会恢复为订阅里的节点。")
                    }
                }
                .listRowBackground(brand.surface)
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .navigationTitle("编辑节点")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("保存") {
                        submit()
                    }
                    .disabled(saving)
                }
            }
        }
    }

    private func submit() {
        switch NodeEdit.apply(to: node, name: name, server: server, port: port) {
        case let .failure(failure):
            error = failure.message
        case let .success(edited):
            saving = true
            Task {
                let saved = await save(edited)
                saving = false
                if saved {
                    dismiss()
                } else {
                    error = String(localized: "保存失败")
                }
            }
        }
    }
}
