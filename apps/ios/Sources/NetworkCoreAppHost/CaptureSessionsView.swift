import NetworkCoreBridge
import SwiftUI

/// Saved capture sessions (plan F1/E11): what was captured, sealed with a key that stays in this device's Keychain,
/// kept for seven days. Opening one replaces what the request list shows and stops live capture.
struct CaptureSessionsView: View {
    var tunnel: TunnelController
    /// Called after a session was opened, so the request list starts over.
    let opened: () -> Void
    @Environment(\.brand) private var brand
    @Environment(\.dismiss) private var dismiss
    @State private var sessions: [PacketCapture.Session] = []
    @State private var loaded = false
    @State private var unavailable = false
    @State private var opening: PacketCapture.Session?
    @State private var deleting: PacketCapture.Session?
    @State private var failure: String?

    var body: some View {
        List {
            Section {
                if sessions.isEmpty {
                    Text(emptyText)
                        .font(.subheadline)
                        .foregroundStyle(brand.text2)
                        .listRowBackground(brand.surface)
                }
                ForEach(sessions) { session in
                    Button {
                        opening = session
                    } label: {
                        row(session)
                    }
                    .buttonStyle(.plain)
                    .disabled(session.unreadable)
                    .listRowBackground(brand.surface)
                    .swipeActions(edge: .trailing) {
                        Button(role: .destructive) {
                            deleting = session
                        } label: {
                            Label("删除", systemImage: "trash")
                        }
                    }
                    .contextMenu {
                        Button(role: .destructive) {
                            deleting = session
                        } label: {
                            Label("删除会话", systemImage: "trash")
                        }
                    }
                }
            } header: {
                if let failure {
                    Text(failure)
                        .foregroundStyle(brand.danger)
                }
            } footer: {
                Text("会话用只保存在本机钥匙串里的密钥加密，保留 7 天后自动删除。打开会话会替换当前的抓包内容并停止抓包，之后可以照常查看、筛选和导出。")
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("已保存的会话")
        .inlineNavigationTitle()
        .confirmationDialog(
            "打开会话“\(opening.map(Self.title) ?? "")”？",
            isPresented: Binding(get: { opening != nil }, set: { if !$0 { opening = nil } }),
            titleVisibility: .visible
        ) {
            Button("打开会话") {
                if let session = opening {
                    Task {
                        await open(session)
                    }
                }
                opening = nil
            }
        } message: {
            Text("当前未保存的抓包内容会被替换，抓包会停止。")
        }
        .confirmationDialog(
            "删除会话“\(deleting.map(Self.title) ?? "")”？",
            isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
            titleVisibility: .visible
        ) {
            Button("删除会话", role: .destructive) {
                if let session = deleting {
                    Task {
                        await delete(session)
                    }
                }
                deleting = nil
            }
        } message: {
            Text("删除后无法恢复。")
        }
        .task(id: tunnel.connection) {
            await load()
        }
    }

    private func row(_ session: PacketCapture.Session) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(Self.title(session))
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(brand.text)
                .lineLimit(1)
            Text(Self.detail(session))
                .font(.caption.monospacedDigit())
                .foregroundStyle(brand.text2)
            if let expires = session.expiresAt {
                Text("\(expires.formatted(date: .abbreviated, time: .shortened)) 自动删除")
                    .font(.caption2)
                    .foregroundStyle(brand.text2)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
    }

    static func title(_ session: PacketCapture.Session) -> String {
        if let name = session.name, !name.isEmpty {
            return name
        }
        return session.savedAt.formatted(date: .abbreviated, time: .standard)
    }

    static func detail(_ session: PacketCapture.Session) -> String {
        if session.unreadable {
            return String(localized: "无法读取（密钥不符或文件损坏）")
        }
        let requests = session.requests.map { String(localized: "\($0) 个请求") }
        let connections = session.connections.map { String(localized: "\($0) 个连接") }
        return ([requests, connections].compactMap { $0 } + [TrafficFormat.bytes(session.size)]).joined(separator: " · ")
    }

    private var emptyText: LocalizedStringKey {
        if tunnel.connection != .connected {
            return "连接后可以查看和打开已保存的会话。"
        }
        if unavailable {
            return "这台设备上无法保存会话（钥匙串不可用）。"
        }
        return loaded ? "还没有保存的会话。在请求列表右上角“更多”中保存当前抓包。" : "正在读取…"
    }

    private func load() async {
        guard tunnel.connection == .connected else {
            sessions = []
            loaded = false
            return
        }
        guard let answer = RuntimeControl.decode(PacketCapture.SessionsAnswer.self, from: await tunnel.control(.captureSessions)) else {
            unavailable = true
            loaded = true
            return
        }
        unavailable = false
        sessions = answer.sessions.sorted { $0.savedAt > $1.savedAt }
        loaded = true
    }

    private func open(_ session: PacketCapture.Session) async {
        guard RuntimeControl.decode(PacketCapture.Status.self, from: await tunnel.control(.loadCapture(session.id))) != nil else {
            failure = String(localized: "无法打开这个会话")
            return
        }
        failure = nil
        opened()
        dismiss()
    }

    private func delete(_ session: PacketCapture.Session) async {
        if await tunnel.control(.deleteCapture(session.id)) == nil {
            failure = String(localized: "无法删除这个会话")
        }
        await load()
    }
}
