import AppKit
import NetworkCoreBridge
import Security
import SwiftUI

/// Extensions: the HTTPS decryption root and the same plugin and script section as on iOS. A Mac trusts the root
/// through the login keychain instead of a configuration profile.
struct MacExtensionsPage: View {
    @ObservedObject var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @StateObject private var certificate = MacCertificateModel()
    @StateObject private var plugins = PluginsModel()
    @State private var confirmingRegenerate = false

    var body: some View {
        Form {
            certificateSection
            PluginsSection(tunnel: tunnel, model: plugins)
        }
        .formStyle(.grouped)
        .scrollContentBackground(.hidden)
        .confirmationDialog("重新生成根证书？", isPresented: $confirmingRegenerate, titleVisibility: .visible) {
            Button("重新生成", role: .destructive) {
                certificate.generate()
                plugins.refreshTrust()
            }
        } message: {
            Text("旧证书立即失效，需要重新加入钥匙串并信任。")
        }
        .task {
            certificate.load()
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            // Coming back from Keychain Access is when trust usually changes.
            certificate.refresh()
            plugins.refreshTrust()
        }
    }

    @ViewBuilder
    private var certificateSection: some View {
        Section {
            HStack(spacing: 12) {
                Image(systemName: certificate.state.symbol)
                    .font(.title2)
                    .foregroundStyle(certificate.state == .trusted ? brand.primary : brand.warning)
                VStack(alignment: .leading, spacing: 2) {
                    Text(certificate.state.title)
                        .font(.headline)
                    Text(certificate.state.detail)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                    if certificate.state == .notTrusted, let reason = certificate.failureReason {
                        Text("系统返回：\(reason)")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .textSelection(.enabled)
                    }
                }
                Spacer(minLength: 8)
                if certificate.authority == nil {
                    Button("生成根证书") {
                        certificate.generate()
                        plugins.refreshTrust()
                    }
                } else if certificate.state != .trusted {
                    Button("信任证书…") {
                        certificate.trust()
                        plugins.refreshTrust()
                    }
                    .help("把根证书加入登录钥匙串并设为始终信任，系统会要求输入登录密码。")
                }
            }

            if let authority = certificate.authority {
                LabeledContent("名称", value: authority.commonName)
                VStack(alignment: .leading, spacing: 4) {
                    Text("SHA-256 指纹")
                    Text(authority.sha256Fingerprint)
                        .font(.caption2.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
                HStack {
                    Button("在“钥匙串访问”中打开") {
                        certificate.openInKeychainAccess()
                    }
                    ShareLink(
                        item: CertificateFile(data: authority.certificateDER, name: authority.commonName),
                        preview: SharePreview("\(authority.commonName).cer")
                    ) {
                        Text("导出证书（.cer）")
                    }
                    Spacer()
                    Button("重新检测") {
                        certificate.refresh()
                        plugins.refreshTrust()
                    }
                    Button("重新生成…", role: .destructive) {
                        confirmingRegenerate = true
                    }
                }
            }

            if let error = certificate.error {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(brand.danger)
                    .textSelection(.enabled)
            }
        } header: {
            Text("HTTPS 解密证书")
        } footer: {
            Text("证书和私钥只保存在本机钥匙串。如果“信任证书”没有成功：在“钥匙串访问”中双击这张证书，展开“信任”，把“使用此证书时”设为“始终信任”。Firefox 使用自己的证书列表，需要另外导入。")
        }
    }
}

/// The root certificate on a Mac: generated into the shared keychain group (where the tunnel extension issues
/// leaves from it), then added to the login keychain with user trust settings so Safari and Chrome accept it.
@MainActor
final class MacCertificateModel: ObservableObject {
    @Published private(set) var authority: MitmCertificateAuthority?
    @Published private(set) var state = MitmTrustState.notGenerated
    @Published private(set) var failureReason: String?
    @Published private(set) var error: String?

    func load() {
        do {
            authority = try MitmCertificateStore.load()
            error = nil
        } catch {
            self.error = String(localized: "读取证书失败（\(Self.describe(error))）")
        }
        refresh()
    }

    func generate() {
        let previous = authority
        do {
            let created = try MitmCertificateAuthority.generate()
            try MitmCertificateStore.save(created)
            authority = created
            error = nil
        } catch {
            self.error = String(localized: "生成证书失败（\(Self.describe(error))）")
        }
        if let previous, previous.certificateDER != authority?.certificateDER {
            Self.removeFromLoginKeychain(previous)
        }
        refresh()
    }

    func refresh() {
        let evaluation = authority?.evaluateTrust()
        state = evaluation?.state ?? .notGenerated
        failureReason = evaluation?.failureReason
    }

    /// Adds the root to the login keychain and marks it always trusted for this user. macOS asks for the login
    /// password itself; when it refuses, the page points to Keychain Access instead.
    func trust() {
        guard let authority, let certificate = SecCertificateCreateWithData(nil, authority.certificateDER as CFData) else {
            return
        }
        let added = SecItemAdd(Self.query(certificate) as CFDictionary, nil)
        guard added == errSecSuccess || added == errSecDuplicateItem else {
            error = String(localized: "无法把证书加入钥匙串（OSStatus \(added)），请用“钥匙串访问”手动信任。")
            return
        }
        let trusted = SecTrustSettingsSetTrustSettings(certificate, .user, nil)
        if trusted == errSecSuccess {
            error = nil
        } else if trusted == errSecAuthFailed || trusted == errAuthorizationCanceled {
            error = String(localized: "已取消。证书已加入钥匙串，但还没有设为始终信任。")
        } else {
            error = String(localized: "无法设置信任（OSStatus \(trusted)），请用“钥匙串访问”手动信任。")
        }
        refresh()
    }

    /// Hands a copy of the certificate to Keychain Access, which imports it and shows it for the trust setting.
    func openInKeychainAccess() {
        guard let authority else {
            return
        }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("\(authority.commonName).cer")
        do {
            try authority.certificateDER.write(to: url, options: .atomic)
        } catch {
            self.error = String(localized: "无法导出证书文件")
            return
        }
        if !NSWorkspace.shared.open(url) {
            error = String(localized: "无法打开“钥匙串访问”，请导出证书后双击打开。")
        }
    }

    /// The old root stays in the login keychain after a regeneration; nothing issues leaves from it any more.
    private static func removeFromLoginKeychain(_ authority: MitmCertificateAuthority) {
        guard let certificate = SecCertificateCreateWithData(nil, authority.certificateDER as CFData) else {
            return
        }
        _ = SecItemDelete(query(certificate) as CFDictionary)
    }

    /// The certificate in the login keychain, where trust evaluation finds the anchor (not the data protection
    /// keychain that holds the private key).
    private static func query(_ certificate: SecCertificate) -> [String: Any] {
        [
            kSecClass as String: kSecClassCertificate,
            kSecValueRef as String: certificate,
        ]
    }

    private static func describe(_ error: Error) -> String {
        if let failure = error as? MitmCertificateStore.Failure {
            return "OSStatus \(failure.status)"
        }
        return String(describing: error)
    }
}
