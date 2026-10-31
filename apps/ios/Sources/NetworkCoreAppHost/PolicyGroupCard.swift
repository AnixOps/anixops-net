import NetworkCoreBridge
import SwiftUI

/// One policy group on the rules page: its name and kind, what it uses now with that member's delay, a health
/// strip of every member's last delay, and for a select group a menu that opens where it is tapped.
struct PolicyGroupCard: View {
    let group: NativeImportReport.Routing.Group
    /// The member the saved profile selects; the running engine's `now` wins when it is known.
    let selected: String?
    let runtime: RuntimeControl.Groups.Group?
    let name: (String) -> String
    let select: (String) -> Void
    @Environment(\.brand) private var brand
    @Environment(\.isEnabled) private var isEnabled

    private var current: String? {
        runtime?.now ?? selected
    }

    private var strip: [LatencyGrade] {
        LatencyGrade.strip(members: group.members, latencies: runtime?.members ?? [])
    }

    private func latency(of member: String) -> UInt32? {
        runtime?.members.first { $0.name == member }?.latencyMs
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Text(verbatim: group.id)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(brand.text)
                    .lineLimit(1)
                Text(group.typeTitle)
                    .font(.caption2.weight(.bold))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .foregroundStyle(brand.text2)
                    .background(brand.raised, in: RoundedRectangle(cornerRadius: 5, style: .continuous))
                Spacer(minLength: 8)
                if group.isSelectable {
                    switchMenu
                }
            }

            HStack(spacing: 8) {
                Text(current.map(name) ?? String(localized: "未选择"))
                    .font(.subheadline)
                    .foregroundStyle(brand.text2)
                    .lineLimit(1)
                if let current, let value = latency(of: current) {
                    Text("\(value) ms")
                        .font(.footnote.weight(.semibold).monospacedDigit())
                        .foregroundStyle(brand.color(for: LatencyGrade(milliseconds: value)))
                }
                if !group.isSelectable {
                    Text("内核自动选择")
                        .font(.caption)
                        .foregroundStyle(brand.text3)
                }
            }

            if !group.members.isEmpty {
                HealthStrip(grades: strip)
            }

            Text(runtime == nil
                ? String(localized: "\(group.members.count) 个成员 · 连接后显示延迟")
                : String(localized: "\(group.members.count) 个成员 · \(LatencyGrade.reachable(strip)) 个有延迟"))
                .font(.caption)
                .foregroundStyle(brand.text3)
        }
        .padding(.vertical, 6)
        .opacity(isEnabled ? 1 : 0.6)
        .accessibilityElement(children: .contain)
    }

    private var switchMenu: some View {
        Menu {
            Picker(selection: Binding(get: { current ?? "" }, set: select)) {
                ForEach(group.members, id: \.self) { member in
                    Text(verbatim: menuLabel(member))
                        .tag(member)
                }
            } label: {
                Text(verbatim: group.id)
            }
            .pickerStyle(.inline)
        } label: {
            HStack(spacing: 4) {
                Text("切换")
                Image(systemName: "chevron.up.chevron.down")
                    .font(.caption2.weight(.bold))
                    .accessibilityHidden(true)
            }
            .font(.footnote.weight(.semibold))
            .foregroundStyle(brand.primary)
            .padding(.horizontal, 12)
            .frame(minHeight: 32)
            .brandGlass(Capsule(), fallback: brand.raised)
        }
        .menuOrder(.fixed)
        .accessibilityLabel(Text("切换 \(group.id)"))
    }

    private func menuLabel(_ member: String) -> String {
        guard let value = latency(of: member) else {
            return name(member)
        }
        return "\(name(member))  ·  \(value) ms"
    }
}

/// One block per member, colored by its last delay; long groups show their first 40 members.
private struct HealthStrip: View {
    let grades: [LatencyGrade]
    @Environment(\.brand) private var brand

    var body: some View {
        let shown = Array(grades.prefix(40))
        HStack(spacing: 2) {
            ForEach(Array(shown.enumerated()), id: \.offset) { _, grade in
                RoundedRectangle(cornerRadius: 2, style: .continuous)
                    .fill(brand.color(for: grade))
                    .frame(maxWidth: 18)
            }
            if shown.count < 40 {
                Spacer(minLength: 0)
            }
        }
        .frame(height: 6)
        .accessibilityElement()
        .accessibilityLabel(Text("\(grades.count) 个成员中 \(LatencyGrade.reachable(grades)) 个有延迟"))
    }
}
