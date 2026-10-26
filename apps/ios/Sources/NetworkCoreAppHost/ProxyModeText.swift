import Foundation
import NetworkCoreBridge

/// What each mode does, for the rules page.
enum ProxyModeText {
    static func title(_ mode: ProxyMode) -> String {
        switch mode {
        case .rule:
            return String(localized: "规则")
        case .global:
            return String(localized: "全局")
        case .direct:
            return String(localized: "直连")
        }
    }

    static func footer(_ mode: ProxyMode) -> String {
        switch mode {
        case .rule:
            return String(localized: "按订阅规则和策略组分流。连接中切换立即生效，已有连接会重新建立。")
        case .global:
            return String(localized: "全部流量走代理页选中的节点。连接中切换立即生效，已有连接会重新建立。")
        case .direct:
            return String(localized: "全部流量直连，不经过代理节点。连接中切换立即生效，已有连接会重新建立。")
        }
    }
}
