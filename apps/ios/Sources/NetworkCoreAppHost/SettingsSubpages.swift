import NetworkCoreBridge
import SwiftUI

/// The pages the settings screen pushes. The first screen keeps the everyday items; everything else sits one level
/// down so a long list never has to be scrolled to find the common switches (docs/ui-ux.md, settings). Shared by the
/// iOS settings and the macOS settings page.
enum SettingsSubpage: String, Hashable {
    case advanced
    case privacy
    case notifications

    var title: LocalizedStringKey {
        switch self {
        case .advanced:
            "高级"
        case .privacy:
            "隐私与诊断"
        case .notifications:
            "通知"
        }
    }

    /// The page the app opens on. CI screenshots open a given one: `-AnixOpsSettingsPage advanced` (debug builds only).
    static var launch: SettingsSubpage? {
        #if DEBUG
        UserDefaults.standard.string(forKey: "AnixOpsSettingsPage").flatMap(SettingsSubpage.init(rawValue:))
        #else
        nil
        #endif
    }
}

/// Settings: what the app stores and sends. One section for the iOS and the macOS "隐私与诊断" page, which used to be
/// written twice. The long footers live here, one tap below the first screen.
struct PrivacySettingsSection: View {
    var tunnel: TunnelController
    @State private var crashReports = ClientHealthMonitor.shared.crashReports.isEnabled
    @State private var panelReport = ClientReportStore().isEnabled
    #if os(iOS)
    @Environment(\.brand) private var brand
    #endif

    var body: some View {
        Section {
            Label("不收集流量和浏览记录", systemImage: "hand.raised")
            Label("节点密码和密钥只保存在本机钥匙串", systemImage: "key")
            Toggle("崩溃报告", isOn: $crashReports)
                .onChange(of: crashReports) { _, enabled in
                    ClientHealthMonitor.shared.crashReports.isEnabled = enabled
                }
            Toggle("向订阅服务商报告连接质量", isOn: $panelReport)
                .onChange(of: panelReport) { _, enabled in
                    tunnel.setPanelReport(enabled: enabled)
                }
        } header: {
            Text("隐私")
        } footer: {
            VStack(alignment: .leading, spacing: 8) {
                Text("崩溃时记录脱敏后的信息（不含网址、账号和节点地址），只保存在本机，复制诊断信息时附上。关闭会删除已保存的报告。")
                Text("开启后，App 打开且已连接时，约每小时将 ANX 节点的连接计数发给提供这些节点的订阅服务商：发往订阅链接去掉参数和扩展名后加 /report 的地址，链接中的令牌随之发出，服务商可据此对应到本人账号；内容为 App 版本、平台、网络类型（Wi-Fi、蜂窝、有线）、统计时长，以及每个节点的名称、服务器地址和端口、协议、成功和各类失败次数、是否因被拒而暂停、时间偏差，不含所访问的网址。默认关闭，关闭时删除保存的计数。")
            }
        }
        #if os(iOS)
        .listRowBackground(brand.surface)
        #endif
    }
}
