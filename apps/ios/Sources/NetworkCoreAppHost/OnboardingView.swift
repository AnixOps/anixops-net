import SwiftUI

/// First run: what the app does in one sentence, why iOS will ask to add a VPN configuration, and one way forward,
/// importing a subscription. Shown once, only while nothing has been imported; 稍后再说 or an import ends it.
struct OnboardingView: View {
    static let storageKey = "onboarding-done"

    /// Opens the import sheet after this screen closes.
    let importNow: () -> Void
    let later: () -> Void
    @Environment(\.brand) private var brand

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            Spacer(minLength: 24)
            BrandMark(size: 80)
            Text("欢迎使用\nAnixOps Net")
                .font(BrandFont.display)
                .foregroundStyle(brand.text)
                .fixedSize(horizontal: false, vertical: true)
            Text("导入你的订阅，就能按规则把需要的流量走代理，其余直连。")
                .font(.title3)
                .foregroundStyle(brand.text2)
                .fixedSize(horizontal: false, vertical: true)
            VStack(alignment: .leading, spacing: 14) {
                point("link", "支持订阅链接、节点链接、二维码，以及 Clash / Surge / Loon 配置。")
                point("lock.shield", "第一次连接时，iOS 会请求添加 VPN 配置。它只用来把流量交给本 App 分流，不收集你的浏览记录。")
                point("key", "节点密码只保存在本机钥匙串里。")
            }
            .padding(.top, 8)
            Spacer()
            Button(action: importNow) {
                Text("导入订阅")
                    .font(.headline)
                    .frame(maxWidth: .infinity, minHeight: 52)
            }
            .buttonStyle(.brandPrimary)
            Button(action: later) {
                Text("稍后再说")
                    .font(.body.weight(.semibold))
                    .frame(maxWidth: .infinity, minHeight: 44)
            }
            .foregroundStyle(brand.primary)
        }
        .padding(.horizontal, 24)
        .padding(.bottom, 16)
        .frame(maxWidth: 520)
        .frame(maxWidth: .infinity)
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
