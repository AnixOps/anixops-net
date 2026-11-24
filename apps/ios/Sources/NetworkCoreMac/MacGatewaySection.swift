import AppKit
import NetworkCoreBridge
import SwiftUI

/// Settings > Gateway mode: other devices on the local network route through this Mac (`GatewayMode`). Shows what
/// to enter on the device and whether IPv4 forwarding is on, with the command to turn it on when the extension may
/// not.
struct MacGatewaySection: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var addresses: [String] = []
    @State private var forwarding: Bool?
    @State private var copied = false

    var body: some View {
        Section {
            Toggle(
                "把这台 Mac 当作网关",
                isOn: Binding(
                    get: { tunnel.gateway },
                    set: { on in
                        Task {
                            await tunnel.setGateway(on)
                        }
                    }
                )
            )
            .disabled(!tunnel.takeover.routesTraffic)
            if tunnel.gateway, tunnel.takeover.routesTraffic {
                LabeledContent("设备的路由器填") {
                    Text(addresses.isEmpty ? String(localized: "没有连上局域网") : addresses.formatted(.list(type: .or)))
                        .textSelection(.enabled)
                }
                LabeledContent("设备的 DNS 填") {
                    Text(verbatim: GatewayMode.dnsServer)
                        .textSelection(.enabled)
                }
                LabeledContent("IP 转发") {
                    Text(forwarding == true ? String(localized: "已开启") : String(localized: "未开启"))
                        .foregroundStyle(forwarding == true ? brand.text2 : brand.danger)
                }
                if forwarding != true {
                    HStack {
                        Text(verbatim: GatewayMode.forwardingCommand)
                            .font(.body.monospaced())
                            .textSelection(.enabled)
                        Spacer()
                        Button(copied ? String(localized: "已复制") : String(localized: "复制命令")) {
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(GatewayMode.forwardingCommand, forType: .string)
                            copied = true
                        }
                        .buttonStyle(.borderless)
                    }
                }
            }
        } header: {
            Text("网关模式")
        } footer: {
            Text(footer)
        }
        .task(id: tunnel.gateway) {
            await refreshLoop()
        }
    }

    private var footer: String {
        guard tunnel.takeover.routesTraffic else {
            return String(localized: "网关模式要用 VPN 接管流量，“仅系统代理”下不能开启。")
        }
        return String(localized: "开启后，局域网设备将路由器和 DNS 设为上方地址，其流量经这台 Mac 按规则转发。连接时开启 IPv4 转发，断开时关闭；仅支持 IPv4。")
    }

    private func refreshLoop() async {
        while !Task.isCancelled {
            addresses = LocalNetworkAddresses.privateIPv4()
            let now = IPv4Forwarding.isOn()
            if now != forwarding {
                forwarding = now
                copied = false
            }
            try? await Task.sleep(for: .seconds(3))
        }
    }
}
