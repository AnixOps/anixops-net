import SwiftUI

/// The appearance picked in Settings. `system` follows iOS; `oled` is dark with a pure black background.
enum Appearance: String, CaseIterable, Identifiable {
    case system
    case light
    case dark
    case oled

    static let storageKey = "appearance"

    var id: String { rawValue }

    var label: String {
        switch self {
        case .system:
            return String(localized: "跟随系统")
        case .light:
            return String(localized: "浅色")
        case .dark:
            return String(localized: "深色")
        case .oled:
            return String(localized: "纯黑")
        }
    }

    var colorScheme: ColorScheme? {
        switch self {
        case .system:
            return nil
        case .light:
            return .light
        case .dark, .oled:
            return .dark
        }
    }
}

/// Brand colors for one theme. The `light`, `dark` and `oled` values, `Brand`, `BrandRadius`, `BrandSpacing`,
/// `BrandMotion` and `BrandFont` are generated into BrandTokens.swift from apps/ios/Design/tokens.json by
/// .github/scripts/design_tokens.py; change the JSON and rerun the script, never the generated file.
struct BrandPalette: Sendable {
    let bg: Color
    let surface: Color
    let raised: Color
    let border: Color
    let soft: Color
    let primary: Color
    let onPrimary: Color
    let text: Color
    let text2: Color
    let text3: Color
    /// Connected and healthy; the brand teal, per U3.
    let success: Color
    let info: Color
    let warning: Color
    let danger: Color
    let amberText: Color
    /// Series colors for charts, in order.
    let chart1: Color
    let chart2: Color
    let chart3: Color
    let chart4: Color
    let heroStart: Color
    let heroEnd: Color
    /// OLED keeps the tab bar black instead of the system material.
    let solidBars: Bool

    static func resolve(appearance: Appearance, scheme: ColorScheme) -> BrandPalette {
        switch appearance {
        case .oled:
            return .oled
        case .dark:
            return .dark
        case .light:
            return .light
        case .system:
            return scheme == .dark ? .dark : .light
        }
    }

    var heroGradient: LinearGradient {
        LinearGradient(colors: [heroStart, heroEnd], startPoint: .topLeading, endPoint: .bottomTrailing)
    }
}

extension Color {
    init(hex: UInt32) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: 1
        )
    }
}

private struct BrandPaletteKey: EnvironmentKey {
    static let defaultValue = BrandPalette.light
}

extension EnvironmentValues {
    var brand: BrandPalette {
        get { self[BrandPaletteKey.self] }
        set { self[BrandPaletteKey.self] = newValue }
    }
}

extension View {
    /// A compact navigation title on iOS; macOS has one title style.
    @ViewBuilder
    func inlineNavigationTitle() -> some View {
        #if os(iOS)
        navigationBarTitleDisplayMode(.inline)
        #else
        self
        #endif
    }

    /// A form sheet's size on macOS, where a sheet otherwise shrinks to its content; iOS sizes sheets itself.
    @ViewBuilder
    func formSheetFrame() -> some View {
        #if os(macOS)
        frame(minWidth: 460, minHeight: 420)
        #else
        self
        #endif
    }

    /// Entry for names, addresses and domains: no capitalization or autocorrection, and the URL keyboard on iOS
    /// when `url` is set.
    @ViewBuilder
    func plainTextEntry(url: Bool = false) -> some View {
        #if os(iOS)
        textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .keyboardType(url ? .URL : .default)
        #else
        autocorrectionDisabled()
        #endif
    }
}

/// A rounded surface card in the current palette.
struct BrandCard<Content: View>: View {
    @Environment(\.brand) private var brand
    var padding: CGFloat = BrandSpacing.md
    @ViewBuilder var content: Content

    var body: some View {
        content
            .padding(padding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(brand.surface, in: RoundedRectangle(cornerRadius: BrandRadius.card, style: .continuous))
    }
}

/// A short protocol badge such as "SS", "VLESS" or "WG"; never a third-party project logo.
struct ProtocolBadge: View {
    @Environment(\.brand) private var brand
    let outboundKind: String?

    var body: some View {
        Text(Self.abbreviation(for: outboundKind))
            .font(.caption.weight(.bold))
            .foregroundStyle(brand.primary)
            .frame(minWidth: 36, minHeight: 36)
            .padding(.horizontal, 4)
            .background(brand.soft, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            .accessibilityLabel(Self.spokenName(for: outboundKind))
    }

    static func abbreviation(for kind: String?) -> String {
        guard let kind = kind?.lowercased() else {
            return "—"
        }
        if kind.hasPrefix("shadowsocks") {
            return "SS"
        }
        if kind.hasPrefix("vless") {
            return "VLESS"
        }
        if kind.hasPrefix("wireguard") {
            return "WG"
        }
        if kind.hasPrefix("hysteria2") {
            return "HY2"
        }
        if kind == "subscription" {
            return "SUB"
        }
        return kind.uppercased()
    }

    static func spokenName(for kind: String?) -> String {
        guard let kind = kind?.lowercased() else {
            return String(localized: "未知协议")
        }
        if kind.hasPrefix("shadowsocks") {
            return "Shadowsocks"
        }
        if kind.hasPrefix("vless") {
            return "VLESS"
        }
        if kind.hasPrefix("wireguard") {
            return "WireGuard"
        }
        if kind == "subscription" {
            return String(localized: "订阅")
        }
        let names = ["trojan": "Trojan", "vmess": "VMess", "hysteria2": "Hysteria2", "tuic": "TUIC"]
        return names[kind] ?? kind
    }
}

/// A centered empty state with an icon, a title and one line of explanation.
struct BrandEmptyState<Actions: View>: View {
    @Environment(\.brand) private var brand
    let systemImage: String
    let title: LocalizedStringKey
    let message: LocalizedStringKey
    @ViewBuilder var actions: Actions

    var body: some View {
        VStack(spacing: 14) {
            Image(systemName: systemImage)
                .font(.system(size: 30, weight: .medium))
                .foregroundStyle(brand.primary)
                .frame(width: 64, height: 64)
                .background(brand.soft, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            Text(title)
                .font(.title3.weight(.semibold))
                .foregroundStyle(brand.text)
            Text(message)
                .font(.subheadline)
                .foregroundStyle(brand.text2)
                .multilineTextAlignment(.center)
            actions
        }
        .padding(24)
        .frame(maxWidth: .infinity)
    }
}

extension BrandEmptyState where Actions == EmptyView {
    init(systemImage: String, title: LocalizedStringKey, message: LocalizedStringKey) {
        self.init(systemImage: systemImage, title: title, message: message) {
            EmptyView()
        }
    }
}
