import SwiftUI

/// The appearance picked in Settings. `system` follows iOS. AnixOps Design's dark theme is already pure black, so
/// the former `oled` choice is kept only so a stored value still decodes; it reads as `dark`.
enum Appearance: String, CaseIterable, Identifiable {
    case system
    case light
    case dark
    case oled

    static let storageKey = "appearance"
    /// The choices Settings offers.
    static let choices: [Appearance] = [.system, .light, .dark]

    var id: String { rawValue }

    /// `oled` folds into `dark`.
    var normalized: Appearance { self == .oled ? .dark : self }

    var label: String {
        switch normalized {
        case .system:
            return String(localized: "跟随系统")
        case .light:
            return String(localized: "浅色")
        case .dark, .oled:
            return String(localized: "深色")
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

extension Binding where Value == Appearance {
    /// The Settings picker's selection: a stored `oled` shows as, and is rewritten to, `dark`.
    var normalized: Binding<Appearance> {
        Binding(get: { wrappedValue.normalized }, set: { wrappedValue = $0 })
    }
}

/// Brand colors for one theme, from AnixOps Design. The `light` and `dark` values, `Brand`, `BrandRadius`,
/// `BrandSpacing`, `BrandMotion` and `BrandFont` are generated into BrandTokens.swift by
/// .github/scripts/design_tokens.py from the vendored AnixOps Design tokens and apps/ios/Design/tokens.json;
/// change those, never the generated file.
struct BrandPalette: Sendable {
    /// The screen behind grouped cards: the grouped gray in light, pure black in dark.
    let bg: Color
    /// Cards and grouped rows.
    let surface: Color
    /// A translucent fill for chips, wells and pressed rows.
    let raised: Color
    /// Hairline separators and card edges.
    let border: Color
    let borderStrong: Color
    /// The accent at low opacity: selected rows, chips, icon wells.
    let soft: Color
    /// The accent for text, links, icons and selection. Filled controls use `primaryFill`.
    let primary: Color
    /// The background of filled primary buttons; white text passes AA on it in both themes.
    let primaryFill: Color
    let onPrimary: Color
    let text: Color
    let text2: Color
    /// Non-essential text only (placeholders, timestamps), at 14 pt and up.
    let text3: Color
    /// Connected and healthy.
    let success: Color
    let successSoft: Color
    let info: Color
    let warning: Color
    let warningSoft: Color
    let danger: Color
    let dangerFill: Color
    let dangerSoft: Color
    let amberText: Color
    /// Series colors for charts, in order.
    let chart1: Color
    let chart2: Color
    let chart3: Color
    let chart4: Color
    /// The connect hero's slate backdrop (a brand moment).
    let heroStart: Color
    let heroEnd: Color

    static func resolve(appearance: Appearance, scheme: ColorScheme) -> BrandPalette {
        switch appearance {
        case .dark, .oled:
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

    /// The AnixOps Design brand gradient (#0EA5E9 → #6366F1 at 135°). Brand moments only: the mark, hero numbers
    /// and the route line on the slate hero; never controls or text that must be read.
    static var brandGradient: LinearGradient {
        LinearGradient(colors: [Brand.sky, Brand.indigo], startPoint: .topLeading, endPoint: .bottomTrailing)
    }
}

extension Color {
    init(hex: UInt32, opacity: Double = 1) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: opacity
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
            .background(brand.soft, in: RoundedRectangle(cornerRadius: BrandRadius.control, style: .continuous))
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
            BrandIconWell(systemImage: systemImage, size: 64)
            Text(title)
                .font(BrandFont.title)
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

/// AnixOps Design buttons: pills, the accent fill with white text for the one primary action on a screen, a soft
/// accent pill for secondary actions. Labels set their own font and width; the style adds the 44 pt minimum height
/// (36 pt for `compact`), side padding, the press state and the disabled fade.
struct BrandButtonStyle: ButtonStyle {
    enum Kind {
        case primary
        case secondary
        case destructive
    }

    var kind: Kind = .primary
    var compact = false
    @Environment(\.brand) private var brand
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(compact ? BrandFont.callout.weight(.semibold) : BrandFont.headline)
            .padding(.horizontal, compact ? BrandSpacing.sm : BrandSpacing.md)
            .frame(minHeight: compact ? 36 : 44)
            .foregroundStyle(foreground)
            .background(background, in: Capsule())
            .contentShape(Capsule())
            .opacity(isEnabled ? (configuration.isPressed ? 0.82 : 1) : 0.4)
            .scaleEffect(configuration.isPressed && !reduceMotion ? 0.98 : 1)
            .animation(.easeOut(duration: BrandMotion.micro), value: configuration.isPressed)
    }

    private var foreground: Color {
        switch kind {
        case .primary:
            return brand.onPrimary
        case .secondary:
            return brand.primary
        case .destructive:
            return brand.danger
        }
    }

    private var background: Color {
        switch kind {
        case .primary:
            return brand.primaryFill
        case .secondary:
            return brand.soft
        case .destructive:
            return brand.dangerSoft
        }
    }
}

extension ButtonStyle where Self == BrandButtonStyle {
    /// The one filled pill per screen.
    static var brandPrimary: BrandButtonStyle { BrandButtonStyle(kind: .primary) }
    /// A soft accent pill.
    static var brandSecondary: BrandButtonStyle { BrandButtonStyle(kind: .secondary) }
    static var brandDestructive: BrandButtonStyle { BrandButtonStyle(kind: .destructive) }
    static var brandPrimaryCompact: BrandButtonStyle { BrandButtonStyle(kind: .primary, compact: true) }
    static var brandSecondaryCompact: BrandButtonStyle { BrandButtonStyle(kind: .secondary, compact: true) }
}

/// A soft icon well: an SF Symbol on the accent's soft fill, used by list rows and empty states.
struct BrandIconWell: View {
    @Environment(\.brand) private var brand
    let systemImage: String
    var size: CGFloat = 32
    var tint: Color?

    var body: some View {
        Image(systemName: systemImage)
            .font(.system(size: size * 0.47, weight: .semibold))
            .foregroundStyle(tint ?? brand.primary)
            .frame(width: size, height: size)
            .background((tint ?? brand.primary).opacity(0.12), in: RoundedRectangle(cornerRadius: size * 0.3, style: .continuous))
            .accessibilityHidden(true)
    }
}
