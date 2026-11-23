import CoreTransferable
import NetworkCoreBridge
import SwiftUI
import UniformTypeIdentifiers

/// Saved capture sessions (plan F1/E11): what was captured, sealed with a key that never leaves this device, kept for
/// seven days. The list is the app's own copies (`CaptureViewer`), so it opens with the tunnel off too; while
/// connected it first copies what the tunnel saved meanwhile. Opening one shows it in the request and connection
/// lists instead of live capture, which carries on unseen; each one also exports as a zip on its own.
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
                        if !session.unreadable {
                            let file = CaptureSessionExportFile(tunnel: tunnel, session: session)
                            ShareLink(item: file, preview: SharePreview(file.title)) {
                                Label("打包导出 zip", systemImage: "doc.zipper")
                            }
                        }
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
                Text(footerText)
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
            Text(
                tunnel.connection == .connected
                    ? LocalizedStringKey("请求列表改为显示这个会话，实时抓包照常进行，随时可以回到实时抓包。")
                    : LocalizedStringKey("请求列表改为显示这个会话。")
            )
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
            if tunnel.connection == .connected {
                // Sessions the tunnel saved since the last look: copy them, then list again.
                await tunnel.syncSavedCaptures()
                await load()
            }
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

    private var footerText: LocalizedStringKey {
        "会话用只保存在本机钥匙串里的密钥加密，保留 7 天后自动删除。隧道断开时会自动保存当次的抓包，断开后也能打开。打开会话后请求列表显示它的内容，可以照常查看、筛选和导出；长按会话可以直接打包导出。"
    }

    private var emptyText: LocalizedStringKey {
        if unavailable {
            return "这台设备上无法打开会话（钥匙串不可用）。"
        }
        return loaded ? "还没有保存的会话。隧道断开时会自动保存当次的抓包，也可以在请求列表右上角“更多”中保存当前抓包。" : "正在读取…"
    }

    private func load() async {
        do {
            sessions = try await CaptureViewer.shared.sessions()
            unavailable = false
        } catch {
            unavailable = true
        }
        loaded = true
    }

    private func open(_ session: PacketCapture.Session) async {
        do {
            try await tunnel.openSavedCapture(session)
        } catch {
            failure = String(localized: "无法打开这个会话")
            return
        }
        failure = nil
        opened()
        dismiss()
    }

    private func delete(_ session: PacketCapture.Session) async {
        do {
            try await tunnel.deleteSavedCapture(session)
        } catch {
            failure = String(localized: "无法删除这个会话")
        }
        await load()
    }
}

/// A saved session as one zip for the share sheet: it is opened in the viewer first (and shown in the lists), since
/// the viewer exports the session it has open.
struct CaptureSessionExportFile: Transferable {
    let tunnel: TunnelController
    let session: PacketCapture.Session

    var title: String {
        String(localized: "打包导出 zip（遮盖密码和 Cookie）")
    }

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(exportedContentType: .data) { file in
            SentTransferredFile(try await file.write())
        }
    }

    func write() async throws -> URL {
        try await tunnel.openSavedCapture(session)
        guard let answer = RuntimeControl.decode(
            PacketCapture.ExportAnswer.self,
            from: try await CaptureViewer.shared.send(.exportCapture(.zip, id: nil))
        ) else {
            throw CocoaError(.fileWriteUnknown)
        }
        return try PacketCapture.write(answer)
    }
}

/// Above the request and connection lists while a saved session is shown instead of live capture: which one, and
/// the way back.
struct SavedCaptureBanner: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand

    var body: some View {
        if let session = tunnel.viewedCapture {
            VStack(alignment: .leading, spacing: 4) {
                Label {
                    if tunnel.viewedOnStop {
                        Text("隧道已断开，下面是断开前抓到的内容，已加密保存 7 天，可以在 ⋯ 里打包导出。")
                    } else {
                        Text("正在查看已保存的会话：\(CaptureSessionsView.title(session))")
                    }
                } icon: {
                    Image(systemName: "clock.arrow.circlepath")
                        .foregroundStyle(brand.primary)
                }
                Text(CaptureSessionsView.detail(session))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(brand.text2)
                if tunnel.connection == .connected {
                    Button("回到实时抓包") {
                        tunnel.showLiveCapture()
                    }
                    .buttonStyle(.borderless)
                } else if !tunnel.viewedOnStop {
                    Button("关闭会话") {
                        tunnel.showLiveCapture()
                    }
                    .buttonStyle(.borderless)
                }
            }
        }
    }
}
