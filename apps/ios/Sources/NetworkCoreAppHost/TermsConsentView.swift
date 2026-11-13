import NetworkCoreBridge
import SwiftUI

/// First start, on iOS and macOS: the user agreement in short, a link to the full text, one primary action to accept
/// it and a way to decline. Nothing else in the app is usable until it is accepted (`TermsConsent`). Declining keeps
/// this screen and says why; the app never quits by itself.
struct TermsConsentView: View {
    let accept: () -> Void
    @State private var declined = false
    @Environment(\.brand) private var brand

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                Spacer(minLength: 24)
                BrandMark(size: 64)
                Text("使用前请阅读用户协议")
                    .font(BrandFont.display)
                    .foregroundStyle(brand.text)
                    .fixedSize(horizontal: false, vertical: true)
                VStack(alignment: .leading, spacing: 14) {
                    point("iphone", "只在你自己的设备上使用，不替别人在其不知情时安装或开启代理、HTTPS 解密和脚本。")
                    point("lock.shield", "HTTPS 解密只用于你自己的流量。根证书由你本人在系统设置里手动信任，只解密你列出的域名。")
                    point("hand.raised", "不得用于入侵他人系统、窃取数据，或绕过游戏、支付、登录等服务的安全机制。")
                    point("shippingbox", "本 App 不提供节点或订阅，导入的内容由提供方负责。软件按现状提供，不作担保。")
                }
                Link(destination: TermsConsent.url) {
                    Label("阅读完整协议", systemImage: "doc.text")
                        .font(.body.weight(.semibold))
                }
                .foregroundStyle(brand.primary)
                if declined {
                    Text("不同意就不能使用 AnixOps Net。你可以重新阅读后同意，或删除本 App。")
                        .font(.subheadline)
                        .foregroundStyle(brand.danger)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Button(action: accept) {
                    Text("同意并继续")
                        .font(.headline)
                        .frame(maxWidth: .infinity, minHeight: 52)
                }
                .buttonStyle(.brandPrimary)
                Button {
                    declined = true
                } label: {
                    Text("不同意")
                        .font(.body.weight(.semibold))
                        .frame(maxWidth: .infinity, minHeight: 44)
                }
                .buttonStyle(.plain)
                .foregroundStyle(brand.text2)
            }
            .padding(.horizontal, 24)
            .padding(.bottom, 16)
            .frame(maxWidth: 520)
            .frame(maxWidth: .infinity)
        }
        .background(brand.bg.ignoresSafeArea())
    }

    private func point(_ systemImage: String, _ text: LocalizedStringKey) -> some View {
        HStack(alignment: .top, spacing: 12) {
            BrandIconWell(systemImage: systemImage, size: 32)
            Text(text)
                .font(.subheadline)
                .foregroundStyle(brand.text2)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
