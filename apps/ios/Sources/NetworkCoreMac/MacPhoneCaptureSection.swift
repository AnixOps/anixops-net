import AppKit
import CoreImage
import CoreImage.CIFilterBuiltins
import NetworkCoreBridge
import SwiftUI

/// Phone debugging on the Extensions page: the tunnel opens a second proxy port to the local network, the phone sets
/// this Mac as its Wi-Fi proxy and scans the QR code for the engine's setup page, which also hands out the HTTPS
/// decryption root (`engine_embedded::lan_share`). Only devices on the local network are answered, and by default
/// only with the proxy username and password this section shows (`LanShareAuth`, kept in the Keychain).
struct MacPhoneCaptureSection: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var share: RuntimeControl.LanShare?
    @State private var portText = String(MacPhoneCaptureSection.defaultPort)
    @State private var address: String?
    @State private var working = false
    /// The share's credentials, read from the Keychain (made there on first use).
    @State private var auth: LanShareAuth?
    @State private var revealed = false
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

            Toggle(isOn: Binding(get: { auth?.required ?? true }, set: { on in Task { await setRequired(on) } })) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("需要密码")
                    Text("手机要在 Wi-Fi 代理里填用户名和密码才能使用这个代理")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .disabled(working || auth == nil)

            if let auth {
                if auth.required {
                    LabeledContent("用户名") {
                        HStack(spacing: 8) {
                            Text(auth.username)
                                .font(.body.monospaced())
                                .textSelection(.enabled)
                            Button {
                                Self.copy(auth.username)
                            } label: {
                                Image(systemName: "doc.on.doc")
                            }
                            .buttonStyle(.borderless)
                            .help("拷贝用户名")
                            .accessibilityLabel("拷贝用户名")
                        }
                    }
                    LabeledContent("密码") {
                        HStack(spacing: 8) {
                            Text(revealed ? auth.password : Self.masked(auth.password))
                                .font(.body.monospaced())
                                .textSelection(.enabled)
                            Button {
                                revealed.toggle()
                            } label: {
                                Image(systemName: revealed ? "eye.slash" : "eye")
                            }
                            .buttonStyle(.borderless)
                            .help(revealed ? "隐藏密码" : "显示密码")
                            .accessibilityLabel(revealed ? "隐藏密码" : "显示密码")
                            Button {
                                Self.copy(auth.password)
                            } label: {
                                Image(systemName: "doc.on.doc")
                            }
                            .buttonStyle(.borderless)
                            .help("拷贝密码")
                            .accessibilityLabel("拷贝密码")
                            Button {
                                Task { await regenerate() }
                            } label: {
                                Image(systemName: "arrow.clockwise")
                            }
                            .buttonStyle(.borderless)
                            .disabled(working)
                            .help("重新生成用户名和密码")
                            .accessibilityLabel("重新生成用户名和密码")
                        }
                    }
                    Text("用随机数生成，只保存在这台 Mac 的钥匙串里。重新生成后，已经设置过的手机要重新填写。")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                } else {
                    Label(
                        "任何人都能使用此代理：同一网络内的设备均可通过这台 Mac 和所用节点上网，其请求也会进入抓包。仅在可信网络中关闭密码。",
                        systemImage: "exclamationmark.triangle.fill"
                    )
                    .font(.footnote)
                    .foregroundStyle(brand.warning)
                }
            }

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
                                Text("设置页提供 iPhone 和 Android 的步骤及根证书下载。设好代理后，在手机上打开 http://\(share.pageHost) 检查。")
                                    .font(.footnote)
                                    .foregroundStyle(.secondary)
                                if share.credentials == true {
                                    Text("iPhone：在 Wi-Fi 代理设置中开启认证并填写上方用户名和密码。Android 的 Wi-Fi 代理无密码栏，浏览器首次访问时询问，其他应用可能无法连接。")
                                        .font(.footnote)
                                        .foregroundStyle(.secondary)
                                }
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
            Text("将手机的 Wi-Fi 代理设为这台 Mac（同一 Wi-Fi）后，手机请求出现在请求页。仅接受局域网设备，默认需用户名和密码；断开连接时关闭。")
        }
        .task {
            loadAuth()
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
            guard let auth = loadAuth() else {
                return
            }
            request = .startLanShare(port: port, auth: auth)
        } else {
            request = .stopLanShare
        }
        guard let answer = RuntimeControl.decode(RuntimeControl.LanShare.self, from: await tunnel.control(request)) else {
            error = on
                ? String(localized: "手机抓包未开启：端口可能被占用，或网络扩展版本过旧。")
                : String(localized: "手机抓包未关闭，请重试。")
            return
        }
        error = nil
        share = answer
    }

    /// The kept credentials, made on first use; nil, with the error shown, when the Keychain refuses. The share is
    /// then not started at all rather than started without a password.
    @discardableResult
    private func loadAuth() -> LanShareAuth? {
        do {
            let loaded = try LanShareAuthStore.load()
            auth = loaded
            return loaded
        } catch {
            self.error = String(localized: "无法读取钥匙串里的手机抓包密码，手机抓包没有开启。")
            return nil
        }
    }

    private func regenerate() async {
        await changeAuth {
            try LanShareAuthStore.regenerate()
        }
    }

    private func setRequired(_ required: Bool) async {
        await changeAuth {
            try LanShareAuthStore.setRequired(required)
        }
    }

    /// Changes the kept credentials and starts a running share again on its port, which is when it takes them.
    private func changeAuth(_ change: () throws -> LanShareAuth) async {
        working = true
        defer { working = false }
        let changed: LanShareAuth
        do {
            changed = try change()
        } catch {
            self.error = String(localized: "无法把手机抓包密码保存到钥匙串。")
            return
        }
        auth = changed
        revealed = false
        error = nil
        guard running, let port = share?.port else {
            return
        }
        guard let answer = RuntimeControl.decode(
            RuntimeControl.LanShare.self,
            from: await tunnel.control(.startLanShare(port: port, auth: changed))
        ) else {
            share = nil
            self.error = String(localized: "手机抓包没有用新的设置重新开启，请关闭后再打开。")
            return
        }
        share = answer
    }

    static func masked(_ password: String) -> String {
        String(repeating: "\u{2022}", count: password.count)
    }

    static func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
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
