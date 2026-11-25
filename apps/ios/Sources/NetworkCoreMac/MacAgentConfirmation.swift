import AppKit
import NetworkCoreBridge
import SwiftUI

/// The confirmation the user answers before the app takes an action an AI tool asked for (docs/mcp-desktop.md §6):
/// a window of the app's own, which the AI tool cannot answer, with 允许 and 拒绝 and the choice to allow actions of
/// the same kind for 15 minutes. No answer within 60 seconds counts as a refusal, and so does closing the window. One
/// confirmation is shown at a time; an action asked for while one is open is refused.
@MainActor
final class MacAgentConfirmation: NSObject, NSWindowDelegate {
    static let shared = MacAgentConfirmation()

    enum Answer: Equatable {
        case allowed
        /// Allowed under an earlier 「15 分钟内允许此类操作」, without asking.
        case granted
        case denied
        case noAnswer
        /// Another confirmation is open.
        case busy
    }

    /// What the window answered: the answer, and whether the user also allowed the kind for 15 minutes.
    private typealias Reply = (answer: Answer, forKind: Bool)

    private var grants = AgentGrants()
    private var panel: NSPanel?
    private var pending: CheckedContinuation<Reply, Never>?
    /// The open confirmation, so a late timer or close answers only its own.
    private var pendingID: UUID?

    /// Asks the user about `action`, or allows it at once under a grant for its kind.
    func confirm(_ action: AgentAction) async -> Answer {
        if grants.allows(action.kind) {
            return .granted
        }
        guard pending == nil else {
            return .busy
        }
        let id = UUID()
        let reply = await withCheckedContinuation { (continuation: CheckedContinuation<Reply, Never>) in
            pending = continuation
            pendingID = id
            show(Self.describe(action), id: id)
            Task { [weak self] in
                try? await Task.sleep(for: .seconds(AgentGrants.confirmationTimeout))
                self?.finish((.noAnswer, false), id: id)
            }
        }
        if reply.answer == .allowed, reply.forKind {
            grants.grant(action.kind)
        }
        return reply.answer
    }

    /// Ends every 15-minute grant; when the user turns actions off.
    func revokeAll() {
        grants.revokeAll()
    }

    private func show(_ text: String, id: UUID) {
        let view = MacAgentConfirmationView(action: text) { [weak self] allowed, forKind in
            self?.finish(allowed ? (.allowed, forKind) : (.denied, false), id: id)
        }
        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 380, height: 180),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        panel.title = "AnixOps Net"
        panel.level = .floating
        panel.isReleasedWhenClosed = false
        panel.hidesOnDeactivate = false
        panel.contentViewController = NSHostingController(rootView: view)
        panel.delegate = self
        panel.center()
        self.panel = panel
        NSApp.activate(ignoringOtherApps: true)
        panel.makeKeyAndOrderFront(nil)
    }

    /// Answers the open confirmation once; a late timer, or a close after a button, does nothing.
    private func finish(_ reply: Reply, id: UUID?) {
        guard let continuation = pending, id != nil, id == pendingID else {
            return
        }
        pending = nil
        pendingID = nil
        let panel = panel
        self.panel = nil
        panel?.delegate = nil
        panel?.close()
        continuation.resume(returning: reply)
    }

    func windowWillClose(_ notification: Notification) {
        finish((.denied, false), id: pendingID)
    }

    /// The action in the confirmation's words.
    static func describe(_ action: AgentAction) -> String {
        switch action {
        case .connect:
            return String(localized: "连接到当前节点")
        case .disconnect:
            return String(localized: "断开连接")
        case let .setMode(mode):
            let title = ProxyModeText.title(mode)
            return String(localized: "切换到“\(title)”模式")
        case let .selectNode(node, group):
            if let group {
                return String(localized: "切换到订阅“\(group)”的节点“\(node)”")
            }
            return String(localized: "切换到节点“\(node)”")
        case .testNodeDelays:
            return String(localized: "对全部节点测速")
        case let .setCapture(on):
            return on ? String(localized: "开始抓包") : String(localized: "停止抓包")
        case .clearCapture:
            return String(localized: "清空抓包")
        case let .closeConnection(id):
            return String(localized: "关闭连接 #\(id)")
        }
    }
}

/// The confirmation's content: what the AI tool asks for, the choice of a 15-minute grant, 拒绝 and 允许. 允许 does
/// not answer the Return key, so a keystroke meant for another app cannot allow an action.
private struct MacAgentConfirmationView: View {
    let action: String
    let answer: (_ allowed: Bool, _ forKind: Bool) -> Void
    @State private var forKind = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("AI 工具请求操作")
                .font(.headline)
            Text(verbatim: action)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
            Toggle("15 分钟内允许此类操作", isOn: $forKind)
                .toggleStyle(.checkbox)
            Text("60 秒内未选择视为拒绝。")
                .font(.caption)
                .foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("拒绝") {
                    answer(false, false)
                }
                .keyboardShortcut(.cancelAction)
                Button("允许") {
                    answer(true, forKind)
                }
            }
        }
        .padding(20)
        .frame(width: 380)
    }
}
