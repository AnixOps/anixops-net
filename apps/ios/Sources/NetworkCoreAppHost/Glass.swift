import NetworkCoreBridge
import SwiftUI

extension View {
    /// Floating, tappable controls (top bar buttons, menu buttons, the compact connect switch) use the system's
    /// Liquid Glass on iOS 26 and later, and the solid `fallback` fill before that. Content cards stay solid so
    /// text keeps its contrast.
    @ViewBuilder
    func brandGlass<S: Shape>(_ shape: S, fallback: Color, tint: Color? = nil) -> some View {
        #if compiler(>=6.2)
        if #available(iOS 26.0, *) {
            glassEffect(tint.map { Glass.regular.tint($0).interactive() } ?? Glass.regular.interactive(), in: shape)
        } else {
            background(fallback, in: shape)
        }
        #else
        background(fallback, in: shape)
        #endif
    }
}

/// A grade's color in the current palette: teal, amber or orange by delay, gray when unmeasured.
extension BrandPalette {
    func color(for grade: LatencyGrade) -> Color {
        switch grade {
        case .good:
            return primary
        case .fair:
            return amberText
        case .poor:
            return warning
        case .unknown:
            return border
        }
    }
}
