import NetworkCoreBridge
import SwiftUI
import UniformTypeIdentifiers

// Shared by the iOS certificate page and the macOS extensions page.

extension MitmTrustState {
    var title: String {
        switch self {
        case .notGenerated: String(localized: "尚未生成证书")
        case .notTrusted: String(localized: "证书未被信任")
        case .trusted: String(localized: "证书已信任")
        }
    }

    var detail: String {
        switch self {
        case .notGenerated: String(localized: "生成本机专属的根证书后才能解密 HTTPS。")
        case .notTrusted: Self.notTrustedDetail
        case .trusted: String(localized: "可以解密 MITM 主机列表中的 HTTPS 流量。")
        }
    }

    private static var notTrustedDetail: String {
        #if os(macOS)
        String(localized: "证书可能未加入钥匙串，或尚未设为“始终信任”。")
        #else
        String(localized: "描述文件可能未安装，或尚未在证书信任设置中开启完全信任。")
        #endif
    }

    var symbol: String {
        switch self {
        case .notGenerated: "lock.open"
        case .notTrusted: "exclamationmark.shield"
        case .trusted: "checkmark.shield"
        }
    }
}

/// The root certificate as a shareable `.cer` file.
struct CertificateFile: Transferable {
    let data: Data
    let name: String

    static var transferRepresentation: some TransferRepresentation {
        DataRepresentation(exportedContentType: .x509Certificate) { $0.data }
            .suggestedFileName { "\($0.name).cer" }
    }
}
