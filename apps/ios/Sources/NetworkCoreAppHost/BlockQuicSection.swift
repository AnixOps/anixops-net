import SwiftUI

/// Settings: refusing QUIC (UDP to port 443) so browsers and apps fall back to TCP, where HTTPS decryption and domain
/// rules see them. A running tunnel switches in place.
struct BlockQuicSection: View {
    var tunnel: TunnelController

    var body: some View {
        Section {
            Toggle(
                "屏蔽 QUIC",
                isOn: Binding(
                    get: { tunnel.blockQuic },
                    set: { on in
                        Task {
                            await tunnel.setBlockQuic(on)
                        }
                    }
                )
            )
        } header: {
            Text("网络")
        } footer: {
            Text("开启后拒绝 UDP 443 端口的 QUIC（HTTP/3）流量，浏览器和应用改用 TCP；直连和全局模式下同样生效。")
        }
    }
}
