import AppKit
import CoreImage
import CoreImage.CIFilterBuiltins
import NetworkCoreBridge
import SwiftUI

/// Phone debugging on the Extensions page: the tunnel opens a second proxy port to the local network, the phone sets
/// this Mac as its Wi-Fi proxy and scans the QR code for the engine's setup page, which also hands out the HTTPS
/// decryption root (`engine_embedded::lan_share`). Only devices on the local network are answered.
struct MacPhoneCaptureSection: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var share: RuntimeControl.LanShare?
    @State private var portText = String(MacPhoneCaptureSection.defaultPort)
    @State private var address: String?
    @State private var working = false
    /// Shown as written: assign it from String(localized:), never a bare literal.
    @State private var error: String?

    static let defaultPort: UInt16 = 8899

    var body: some View {
        Section {
            Toggle(isOn: Binding(get: { running }, set: { on in Task { await set(on) } })) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("允许手机通过这台 Mac 上网")
                    Text(tunnel.isConnected ? "在局域网里开一个代理端口" : "先连接，再开启手机抓包")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .disabled(working || !tunnel.isConnected || (!running && port == nil))

            if !running {
                LabeledContent("端口") {
                    TextField("端口", text: $portText)
                        .labelsHidden()
                        .multilineTextAlignment(.trailing)
                        .frame(width: 80)
                }
            }

            if let share, running {
                if share.addresses.isEmpty {
                    Text("没有找到局域网地址，确认 Mac 连着 Wi-Fi 或有线局域网。")
                        .font(.footnote)
                        .foregroundStyle(brand.warning)
                } else {
                    if share.addresses.count > 1 {
                        Picker("代理地址", selection: Binding(get: { chosen ?? "" }, set: { address = $0 })) {
                            ForEach(share.addresses, id: \.self) { item in
                                Text("\(item):\(share.port ?? 0)").tag(item)
                            }
                        }
                    } else if let chosen {
                        LabeledContent("代理地址") {
                            Text("\(chosen):\(share.port ?? 0)")
                                .font(.body.monospaced())
                                .textSelection(.enabled)
                        }
                    }
                    if let chosen, let url = share.setupURL(address: chosen) {
                        HStack(alignment: .center, spacing: 16) {
                            if let image = Self.qrImage(url) {
                                Image(nsImage: image)
                                    .interpolation(.none)
                                    .resizable()
                                    .frame(width: 160, height: 160)
                                    .padding(6)
                                    .background(Brand.qrPaper, in: RoundedRectangle(cornerRadius: 10))
                                    .accessibilityLabel("设置页二维码")
                            }
                            VStack(alignment: .leading, spacing: 6) {
                                Text("用手机相机扫码，打开设置页")
                                    .font(.headline)
                                Text(url)
                                    .font(.callout.monospaced())
                                    .textSelection(.enabled)
                                Text("设置页里有 iPhone 和 Android 的步骤和根证书下载。设好代理后在手机上打开 http://\(share.pageHost) 检查是否生效。")
                                    .font(.footnote)
                                    .foregroundStyle(.secondary)
                                Label(
                                    share.httpsDecryption ? "HTTPS 解密已开启" : "HTTPS 解密未开启，只能看到连接",
                                    systemImage: share.httpsDecryption ? "lock.fill" : "lock.open"
                                )
                                .font(.footnote)
                                .foregroundStyle(share.httpsDecryption ? brand.primary : brand.warning)
                                Text("\(share.active) 条手机连接")
                                    .font(.footnote)
                                    .foregroundStyle(.secondary)
                            }
                        }
                        .padding(.vertical, 4)
                    }
                }
            }

            if let error {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(brand.danger)
                    .textSelection(.enabled)
            }
        } header: {
            Text("手机抓包")
        } footer: {
            Text("手机和 Mac 连同一个 Wi-Fi，把手机的 Wi-Fi 代理设成 Mac，手机的请求就会出现在请求页。只接受局域网里的设备；断开连接时一并关闭。")
        }
        .task(id: tunnel.isConnected) {
            await refreshLoop()
        }
    }

    private var running: Bool {
        share?.running == true
    }

    private var port: UInt16? {
        guard let value = UInt16(portText.trimmingCharacters(in: .whitespaces)), value >= 1024 else {
            return nil
        }
        return value
    }

    private var chosen: String? {
        guard let share else {
            return nil
        }
        if let address, share.addresses.contains(address) {
            return address
        }
        return share.addresses.first
    }

    private func refreshLoop() async {
        while !Task.isCancelled {
            share = tunnel.isConnected
                ? RuntimeControl.decode(RuntimeControl.LanShare.self, from: await tunnel.control(.lanShare))
                : nil
            try? await Task.sleep(for: .seconds(3))
        }
    }

    private func set(_ on: Bool) async {
        working = true
        defer { working = false }
        let request: RuntimeControl
        if on {
            guard let port else {
                error = String(localized: "端口要在 1024 到 65535 之间。")
                return
            }
            request = .startLanShare(port: port)
        } else {
            request = .stopLanShare
        }
        guard let answer = RuntimeControl.decode(RuntimeControl.LanShare.self, from: await tunnel.control(request)) else {
            error = on
                ? String(localized: "手机抓包没有开启：端口可能被占用，或者网络扩展版本太旧。")
                : String(localized: "手机抓包没有关闭，请重试。")
            return
        }
        error = nil
        share = answer
    }

    /// A sharp QR code of `text`.
    static func qrImage(_ text: String) -> NSImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage?.transformed(by: CGAffineTransform(scaleX: 8, y: 8)),
              let cgImage = CIContext().createCGImage(output, from: output.extent)
        else {
            return nil
        }
        return NSImage(cgImage: cgImage, size: NSSize(width: output.extent.width, height: output.extent.height))
    }
}
