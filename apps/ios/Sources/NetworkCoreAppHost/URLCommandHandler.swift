import Foundation
import NetworkCoreBridge

/// Runs an `anixops://` link (`AppURLCommand`) against the tunnel. Connect, disconnect, mode and node picks run at
/// once, as in Surge and Loon; an import link only fills the import sheet, which still asks the user to confirm.
extension TunnelController {
    /// What to show after a link ran: nil when the change is visible on its own.
    @discardableResult
    func handle(url: URL) async -> URLCommandOutcome {
        switch AppURLCommand.parse(url) {
        case .failure(.notOurScheme):
            return .ignored
        case let .failure(failure):
            show(String(localized: "无法处理这个链接") + " (\(Self.describe(failure)))")
            return .handled
        case let .success(command):
            return await run(command)
        }
    }

    private func run(_ command: AppURLCommand) async -> URLCommandOutcome {
        // A link can launch the app: wait for the saved profile and groups, at most a few seconds.
        for _ in 0..<50 where !loaded {
            try? await Task.sleep(for: .milliseconds(100))
        }
        switch command {
        case .connect:
            if !isConnected {
                toggle()
            }
        case .disconnect:
            if isConnected {
                toggle()
            }
        case .toggle:
            toggle()
        case let .mode(mode):
            await setMode(mode)
        case let .selectNode(name, group):
            guard let target = AppURLCommand.resolveNode(name: name, group: group, in: subscriptions) else {
                show(String(localized: "没有找到节点“\(name)”"))
                return .handled
            }
            await selectNode(target.nodeID, inGroup: target.groupID)
        case let .selectMember(policy, member):
            guard let group = library?.routing?.groups.first(where: { $0.id == policy }), group.isSelectable,
                  group.members.contains(member)
            else {
                show(String(localized: "策略组“\(policy)”里没有“\(member)”"))
                return .handled
            }
            await selectMember(member, inGroup: policy)
        case let .importLink(link):
            pendingImportLink = link
            return .openImport
        }
        return .handled
    }

    /// The link an `anixops://import` asked for, handed to the import sheet once when it opens.
    func takePendingImportLink() -> String? {
        defer { pendingImportLink = nil }
        return pendingImportLink
    }

    private static func describe(_ failure: AppURLCommand.Failure) -> String {
        switch failure {
        case .notOurScheme:
            return "scheme"
        case let .unknownAction(action):
            return action
        case let .missing(name), let .invalid(name):
            return name
        }
    }
}

enum URLCommandOutcome: Equatable {
    /// Not an `anixops://` link.
    case ignored
    case handled
    /// The caller opens the import sheet; `takePendingImportLink()` holds the link.
    case openImport
}
