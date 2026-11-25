import NetworkCoreBridge
import SwiftUI

// The mesh (组网) entry on the overview and the mesh page (docs/mesh-ui.md). The rules for grouping, folding,
// searching and the notices are MeshView's (the Swift side of client-view's view.mesh_*); these views only draw
// its answers. Phones are access endpoints: no relay, no advertising, no host mode.

/// The overview's 组网 section: network, state, members online and the join switch, which is independent of the
/// connect switch. Tapping it opens the mesh page. Hidden while the engine has no mesh module.
struct MeshEntryCard: View {
    var tunnel: TunnelController
    var model: MeshModel
    @Environment(\.brand) private var brand

    var body: some View {
        let entry = model.entry
        let state = entry.string("state")
        BrandCard {
            HStack(spacing: 12) {
                NavigationLink {
                    MeshPage(tunnel: tunnel, model: model)
                } label: {
                    HStack(spacing: 12) {
                        BrandIconWell(systemImage: "point.3.connected.trianglepath.dotted")
                        VStack(alignment: .leading, spacing: 3) {
                            HStack(spacing: 6) {
                                Text("组网")
                                    .font(.subheadline.weight(.semibold))
                                    .foregroundStyle(brand.text)
                                if entry.bool("attention") {
                                    Circle()
                                        .fill(brand.warning)
                                        .frame(width: 8, height: 8)
                                        .accessibilityLabel("有提示")
                                }
                            }
                            summary(entry, state: state)
                        }
                        Spacer(minLength: 0)
                        Image(systemName: "chevron.right")
                            .font(.footnote.weight(.semibold))
                            .foregroundStyle(brand.text2)
                            .accessibilityHidden(true)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("mesh-entry")
                if model.network != nil {
                    MeshJoinToggle(tunnel: tunnel, model: model)
                }
            }
        }
    }

    @ViewBuilder
    private func summary(_ entry: MeshObject, state: String?) -> some View {
        if let network = entry.string("network") {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    MeshStateDot(state: state)
                    Text("\(network) · \(MeshText.state(state))")
                        .font(.footnote)
                        .foregroundStyle(brand.text2)
                        .lineLimit(1)
                }
                HStack(spacing: 6) {
                    if state == "joined" || state == "partial" {
                        Text("\(entry.int("online") ?? 0)/\(entry.int("total") ?? 0) 台在线")
                            .font(.footnote.monospacedDigit())
                            .foregroundStyle(brand.text2)
                    }
                    MeshSecurityTag(security: entry.string("security"), hideSecure: true)
                }
            }
        } else {
            Text("暂无网络")
                .font(.footnote)
                .foregroundStyle(brand.text2)
        }
    }
}

/// The join switch. Its accessible name is what it controls (加入), not the state.
struct MeshJoinToggle: View {
    var tunnel: TunnelController
    var model: MeshModel

    var body: some View {
        let state = model.network?.string("state")
        let joined = state == "joined" || state == "joining" || state == "partial"
        Toggle(
            "加入",
            isOn: Binding(
                get: { joined },
                set: { value in
                    Task {
                        await model.setJoined(value, tunnel: tunnel)
                    }
                }
            )
        )
        .labelsHidden()
        .disabled(model.busy)
        .accessibilityLabel("加入")
        .accessibilityIdentifier("mesh-join")
    }
}

struct MeshStateDot: View {
    let state: String?
    @Environment(\.brand) private var brand

    var body: some View {
        Circle()
            .fill(color)
            .frame(width: 8, height: 8)
            .accessibilityHidden(true)
    }

    private var color: Color {
        switch state {
        case "joined", "online":
            return brand.success
        case "joining":
            return brand.info
        case "partial":
            return brand.warning
        case "failed", "unreachable":
            return brand.danger
        default:
            return brand.text3
        }
    }
}

/// The encryption-mode mark: 未加密 in the danger colour, 未启用前向保密 in the warning colour. With `hideSecure`,
/// forward secrecy (the default) shows nothing.
struct MeshSecurityTag: View {
    let security: String?
    var hideSecure = false
    @Environment(\.brand) private var brand

    var body: some View {
        if let label = MeshText.security(security), !(hideSecure && security == "forward_secrecy") {
            MeshTag(text: label, foreground: foreground, background: background)
        }
    }

    private var foreground: Color {
        switch security {
        case "none":
            return brand.danger
        case "standard":
            return brand.amberText
        default:
            return brand.primary
        }
    }

    private var background: Color {
        switch security {
        case "none":
            return brand.dangerSoft
        case "standard":
            return brand.warningSoft
        default:
            return brand.soft
        }
    }
}

struct MeshTag: View {
    let text: String
    let foreground: Color
    let background: Color

    var body: some View {
        Text(text)
            .font(.caption2.weight(.bold))
            .lineLimit(1)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .foregroundStyle(foreground)
            .background(background, in: RoundedRectangle(cornerRadius: BrandRadius.badge, style: .continuous))
    }
}

/// The latency in ms with a dot for its tone (the node list's thresholds, 120 and 250 ms); the number is not
/// coloured. 超时 when it cannot be measured.
struct MeshLatency: View {
    let milliseconds: Int?
    @Environment(\.brand) private var brand

    var body: some View {
        HStack(spacing: 4) {
            Circle()
                .fill(color)
                .frame(width: 6, height: 6)
                .accessibilityHidden(true)
            Text(MeshText.latency(milliseconds))
                .font(.footnote.monospacedDigit())
                .foregroundStyle(brand.text2)
        }
    }

    private var color: Color {
        switch MeshView.latencyTone(milliseconds) {
        case "fast":
            return brand.success
        case "medium":
            return brand.warning
        case "slow":
            return brand.danger
        default:
            return brand.text3
        }
    }
}

// MARK: - Page

/// The mesh page: the status section, the notices, and the segments 成员 · 子网 · 路径.
struct MeshPage: View {
    enum Segment: String, Hashable {
        case members
        case subnets
        case paths

        /// The view the mesh page opens on. CI screenshots open a given one: `-AnixOpsMesh subnets` (debug builds
        /// only); on the iPhone the page is then pushed at launch.
        static var launch: Segment? {
            #if DEBUG
            UserDefaults.standard.string(forKey: "AnixOpsMesh").flatMap(Segment.init(rawValue:))
            #else
            nil
            #endif
        }
    }

    var tunnel: TunnelController
    var model: MeshModel
    @Environment(\.brand) private var brand
    @State private var segment = Segment.launch ?? .members
    @State private var query = ""
    @State private var groupBy = "region"
    @State private var filter = "all"
    @State private var sort = "name"
    /// Groups the user opened or closed, by key; the others follow the answer's `open`.
    @State private var groupOpen: [String: Bool] = [:]
    /// The member whose detail is shown.
    @State private var detail: String?
    @State private var adding = false
    /// The network whose removal waits for confirmation (T1), by name.
    @State private var removing: String?
    /// 添加路径 (a panel on phones).
    @State private var addingPath = false

    var body: some View {
        List {
            statusSection
            noticesSection
            if model.network != nil {
                switch segment {
                case .members:
                    memberSections
                case .subnets:
                    subnetSection
                case .paths:
                    pathSections
                }
            }
        }
        .scrollContentBackground(.hidden)
        .topControlBar(brand) {
            if model.network != nil {
                Picker("视图", selection: $segment) {
                    Text("成员").tag(Segment.members)
                    Text("子网").tag(Segment.subnets)
                    Text("mesh.segment.paths").tag(Segment.paths)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
            }
        }
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("组网")
        .inlineNavigationTitle()
        .searchable(text: $query, prompt: Text("搜索名称、域名、地址或子网"))
        .toolbar {
            if model.network != nil, segment == .members {
                ToolbarItem(placement: .primaryAction) {
                    memberMenu
                }
            }
            ToolbarItem(placement: .primaryAction) {
                moreMenu
            }
        }
        .sheet(isPresented: $addingPath) {
            MeshPathSheet(status: model.status) { draft in
                await model.addPath(draft, tunnel: tunnel)
            }
        }
        .sheet(isPresented: $adding) {
            MeshNetworkSheet(taken: Set(model.saved.map(\.name))) { network, secret in
                await model.addNetwork(network, secret: secret, tunnel: tunnel)
            }
        }
        .confirmationDialog(
            "移除网络“\(removing ?? "")”？",
            isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
            titleVisibility: .visible
        ) {
            Button("移除网络", role: .destructive) {
                removing = nil
                Task {
                    await model.removeNetwork(tunnel: tunnel)
                }
            }
        } message: {
            Text("本机退出该网络并删除其密钥。")
        }
        .navigationDestination(item: $detail) { id in
            MeshMemberDetail(id: id, tunnel: tunnel, model: model)
        }
        .task(id: tunnel.vpnConnection) {
            await model.poll(tunnel)
        }
    }

    /// 更多: 添加网络, then 移除网络 after a divider (docs/mesh-ui.md, 入口与导航).
    private var moreMenu: some View {
        Menu {
            Button {
                adding = true
            } label: {
                Label("添加网络", systemImage: "plus")
            }
            if let network = model.network {
                Divider()
                Button(role: .destructive) {
                    removing = network.string("name") ?? network.string("id")
                } label: {
                    Label("移除网络", systemImage: "trash")
                }
            }
        } label: {
            Label("更多", systemImage: "ellipsis.circle")
        }
        .accessibilityIdentifier("mesh-more")
    }

    // MARK: Status

    @ViewBuilder
    private var statusSection: some View {
        if let network = model.network {
            let state = network.string("state")
            Section {
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(network.string("name") ?? network.string("id") ?? "")
                            .font(.headline)
                            .foregroundStyle(brand.text)
                        HStack(spacing: 6) {
                            MeshStateDot(state: state)
                            Text(statusLine(network))
                                .font(.footnote)
                                .foregroundStyle(brand.text2)
                        }
                    }
                    Spacer(minLength: 8)
                    MeshJoinToggle(tunnel: tunnel, model: model)
                }
                .listRowBackground(brand.surface)
                if let security = MeshText.security(network.string("security")) {
                    LabeledContent("加密模式") {
                        MeshSecurityTag(security: network.string("security"))
                            .accessibilityLabel(security)
                    }
                    .listRowBackground(brand.surface)
                }
                if let own = model.own {
                    if let domain = own.string("domain") {
                        MeshCopyRow(title: "内网域名", value: domain)
                    }
                    if let address = own.string("address") {
                        MeshCopyRow(title: "虚拟地址", value: address)
                    }
                    if let region = own.string("region") {
                        LabeledContent("地区") {
                            Text(regionLine(own, code: region))
                                .foregroundStyle(brand.text2)
                        }
                        .listRowBackground(brand.surface)
                    }
                }
                if let refusal = model.refusal {
                    MeshNoticeRow(text: MeshText.failure(refusal.code), detail: refusal.detail) {
                        model.clearRefusal()
                    }
                }
            }
        } else {
            Section {
                BrandEmptyState(systemImage: "point.3.connected.trianglepath.dotted", title: "暂无网络", message: "加入网络后显示成员与子网。") {
                    Button("添加网络") {
                        adding = true
                    }
                    .buttonStyle(BrandButtonStyle())
                    .accessibilityIdentifier("mesh-add")
                }
                .listRowBackground(brand.surface)
            }
        }
    }

    private func statusLine(_ network: MeshObject) -> String {
        let state = network.string("state")
        if state == "joining", let joining = network.object("joining") {
            if joining.string("stage") == "connecting", let peers = joining.int("peersConnected") {
                return String(localized: "正在加入：已连接 \(peers) 个对端")
            }
            if let stage = MeshText.joiningStage(joining.string("stage")) {
                return stage
            }
        }
        if state == "joined" || state == "partial" {
            let entry = model.entry
            return String(localized: "\(MeshText.state(state)) · \(entry.int("online") ?? 0)/\(entry.int("total") ?? 0) 台在线")
        }
        return MeshText.state(state)
    }

    private func regionLine(_ own: MeshObject, code: String) -> String {
        let name = regionName(code)
        switch own.string("regionState") {
        case "pending":
            return String(localized: "\(name) · 待批准")
        case "pinned":
            return String(localized: "\(name) · 已固定")
        default:
            if let border = own.object("regionBasis")?.string("border") {
                return String(localized: "\(name) · 到 \(border) 延迟最低")
            }
            return name
        }
    }

    private func regionName(_ code: String?) -> String {
        guard let code else {
            return ""
        }
        let regions = MeshObject(model.status).objects("regions")
        return regions.first { $0.string("code") == code }?.string("name") ?? code
    }

    @ViewBuilder
    private var noticesSection: some View {
        let notices = model.notices
        if !notices.isEmpty {
            Section {
                ForEach(Array(notices.enumerated()), id: \.offset) { _, notice in
                    MeshNoticeRow(text: MeshText.notice(notice), detail: MeshText.noticeDetail(notice), dismiss: nil)
                }
            }
        }
    }

    // MARK: Members

    private var members: MeshObject {
        MeshObject(MeshView.members(status: model.status, groupBy: groupBy, filter: filter, sort: sort, query: query, devices: []))
    }

    @ViewBuilder
    private var memberSections: some View {
        let answer = members
        let groups = answer.objects("groups")
        if groups.isEmpty {
            Section {
                (query.isEmpty && filter == "all" ? Text("暂无成员") : Text("无匹配的成员"))
                    .font(.subheadline)
                    .foregroundStyle(brand.text2)
                    .listRowBackground(brand.surface)
            }
        }
        ForEach(Array(groups.enumerated()), id: \.offset) { _, group in
            let key = group.string("key") ?? ""
            let open = groupOpen[key] ?? group.bool("open")
            Section {
                if open {
                    ForEach(Array(group.objects("rows").enumerated()), id: \.offset) { _, row in
                        MeshMemberRow(row: row) { id in
                            detail = id
                        }
                            .listRowBackground(brand.surface)
                    }
                }
            } header: {
                if answer.string("groupBy") != "none" {
                    Button {
                        groupOpen[key] = !open
                    } label: {
                        groupHeader(group, key: key, by: answer.string("groupBy"), open: open)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private func groupHeader(_ group: MeshObject, key: String, by: String?, open: Bool) -> some View {
        let title = by == "role" ? (MeshText.role(key) ?? key) : regionName(key)
        return HStack(spacing: 6) {
            Image(systemName: open ? "chevron.down" : "chevron.right")
                .font(.caption.weight(.bold))
                .accessibilityHidden(true)
            Text(title)
                .font(.footnote.weight(.semibold))
            Text("\(group.int("count") ?? 0) 台")
                .font(.footnote.monospacedDigit())
            if let link = group.object("link") {
                if let via = link.string("via") {
                    Text("经\(regionName(via))")
                        .font(.footnote)
                }
                Text(MeshText.latency(link.int("latencyMs")))
                    .font(.footnote.monospacedDigit())
            }
            if let worst = group.object("worst") {
                worstText(worst)
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(brand.amberText)
            }
            Spacer(minLength: 0)
        }
        .foregroundStyle(brand.text2)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
        .accessibilityValue(open ? Text("已展开") : Text("已折叠"))
    }

    private func worstText(_ worst: MeshObject) -> Text {
        let count = worst.int("count") ?? 0
        if worst.string("state") == "unreachable" {
            return Text("\(count) 台不可达")
        }
        return Text("\(count) 条路径已回落")
    }

    private var memberMenu: some View {
        Menu {
            Picker("分组", selection: $groupBy) {
                Text("按地区").tag("region")
                Text("按角色").tag("role")
                Text("不分组").tag("none")
            }
            Picker("筛选", selection: $filter) {
                Text("全部").tag("all")
                Text("仅在线").tag("online")
                Text("不可达").tag("unreachable")
                Text("网关").tag("gateway")
                Text("宿主").tag("host")
                Text("边界").tag("border")
                Text("有自定义路径").tag("custom_path")
            }
            Picker("排序", selection: $sort) {
                Text("名称").tag("name")
                Text("延迟").tag("latency")
                Text("虚拟地址").tag("address")
            }
        } label: {
            Image(systemName: "line.3.horizontal.decrease.circle")
        }
        .accessibilityLabel("分组、筛选与排序")
    }

    // MARK: Subnets

    @ViewBuilder
    private var subnetSection: some View {
        let answer = MeshObject(MeshView.subnets(status: model.status, query: query))
        let rows = answer.objects("rows")
        Section {
            if rows.isEmpty {
                (query.isEmpty ? Text("暂无子网") : Text("无匹配的子网"))
                    .font(.subheadline)
                    .foregroundStyle(brand.text2)
                    .listRowBackground(brand.surface)
            }
            ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                MeshSubnetRow(row: row, tunnel: tunnel, model: model)
                    .listRowBackground(brand.surface)
            }
        } footer: {
            if let refused = answer.int("refused"), refused > 0 {
                Text("已拒绝 \(refused) 项公网网段通告。")
            }
        }
    }

    // MARK: Paths

    @ViewBuilder
    private var pathSections: some View {
        let answer = MeshObject(MeshView.paths(status: model.status))
        Section("默认选路") {
            Picker("路径策略", selection: Binding(
                get: { answer.string("policy") == "hops" ? "hops" : "latency" },
                set: { policy in
                    guard policy != answer.string("policy") else {
                        return
                    }
                    Task {
                        await model.setPolicy(policy, tunnel: tunnel)
                    }
                }
            )) {
                Text("延迟最低").tag("latency")
                Text("跳数最低").tag("hops")
            }
            .pickerStyle(.segmented)
            .disabled(model.busy)
            .listRowBackground(brand.surface)
        }
        if answer.raw["links"] is [Any] {
            Section("跨区链路") {
                let links = answer.objects("links")
                if links.isEmpty {
                    Text("暂无跨区链路")
                        .font(.subheadline)
                        .foregroundStyle(brand.text2)
                        .listRowBackground(brand.surface)
                }
                ForEach(Array(links.enumerated()), id: \.offset) { _, link in
                    MeshLinkRow(link: link)
                        .listRowBackground(brand.surface)
                }
            }
        }
        if answer.raw["custom"] is [Any] {
            Section("自定义路径") {
                let paths = answer.objects("custom")
                if paths.isEmpty {
                    Text("暂无自定义路径")
                        .font(.subheadline)
                        .foregroundStyle(brand.text2)
                        .listRowBackground(brand.surface)
                }
                ForEach(Array(paths.enumerated()), id: \.offset) { _, path in
                    MeshCustomPathRow(path: path)
                        .listRowBackground(brand.surface)
                        .swipeActions {
                            Button("移除", role: .destructive) {
                                Task {
                                    await model.removePath(path.string("id") ?? "", tunnel: tunnel)
                                }
                            }
                        }
                }
                if model.removedPaths != nil {
                    HStack {
                        Text("已移除路径。")
                            .font(.subheadline)
                            .foregroundStyle(brand.text2)
                        Spacer()
                        Button("撤销") {
                            Task {
                                await model.undoPathRemoval(tunnel: tunnel)
                            }
                        }
                        .disabled(model.busy)
                    }
                    .listRowBackground(brand.surface)
                }
                Button {
                    addingPath = true
                } label: {
                    Label("添加路径", systemImage: "plus")
                }
                .disabled(model.busy)
                .listRowBackground(brand.surface)
            }
        }
    }
}

// MARK: - Rows

struct MeshNoticeRow: View {
    let text: String
    let detail: String?
    let dismiss: (() -> Void)?
    @Environment(\.brand) private var brand

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(brand.warning)
                    .accessibilityHidden(true)
                Text(text)
                    .font(.subheadline)
                    .foregroundStyle(brand.text)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
                if let dismiss {
                    Button(action: dismiss) {
                        Image(systemName: "xmark")
                            .font(.footnote.weight(.semibold))
                            .foregroundStyle(brand.text2)
                            .frame(width: 28, height: 28)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("关闭提示")
                }
            }
            if let detail {
                DisclosureGroup {
                    Text(detail)
                        .font(.caption.monospaced())
                        .foregroundStyle(brand.text2)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .textSelection(.enabled)
                } label: {
                    Text("技术详情")
                        .font(.footnote.weight(.semibold))
                        .foregroundStyle(brand.text2)
                }
                .tint(brand.text2)
            }
        }
        .listRowBackground(brand.warningSoft)
        .accessibilityIdentifier("mesh-notice")
    }
}

/// A value that can be copied: the internal domain name or the virtual address.
struct MeshCopyRow: View {
    let title: LocalizedStringKey
    let value: String
    @Environment(\.brand) private var brand

    var body: some View {
        LabeledContent(title) {
            HStack(spacing: 8) {
                Text(value)
                    .font(.subheadline.monospaced())
                    .foregroundStyle(brand.text2)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
                Button {
                    Pasteboard.copy(value)
                } label: {
                    Image(systemName: "doc.on.doc")
                        .font(.footnote)
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("复制")
            }
        }
        .listRowBackground(brand.surface)
    }
}

/// A member row (N34 role mark, N30 one level, N31 more than five folded). Phones show two lines: name, role and
/// latency; then the region part of the internal domain name, the connection, the transport and what it contains.
/// The address is in the member detail.
struct MeshMemberRow: View {
    let row: MeshObject
    let showDetail: (String) -> Void
    @Environment(\.brand) private var brand
    @State private var open: Bool?
    @State private var all = false

    var body: some View {
        let children = row.objects("children")
        let isOpen = open ?? row.bool("open")
        VStack(alignment: .leading, spacing: 6) {
            Button {
                if children.isEmpty {
                    showDetail(row.string("id") ?? "")
                } else {
                    open = !isOpen
                }
            } label: {
                summary(children: children)
            }
            .buttonStyle(.borderless)
            .accessibilityValue(children.isEmpty ? Text(verbatim: "") : (isOpen ? Text("已展开") : Text("已折叠")))
            if isOpen, !children.isEmpty {
                let visible = all ? children.count : max(row.int("visibleChildren") ?? children.count, 0)
                ForEach(Array(children.prefix(visible).enumerated()), id: \.offset) { _, child in
                    MeshChildRow(child: child)
                }
                if visible < children.count {
                    Button("另 \(children.count - visible) 项") {
                        all = true
                    }
                    .font(.footnote.weight(.semibold))
                    .buttonStyle(.borderless)
                    .padding(.leading, 18)
                }
            }
            if isOpen, !children.isEmpty {
                Button {
                    showDetail(row.string("id") ?? "")
                } label: {
                    Text("成员详情")
                        .font(.footnote.weight(.semibold))
                        .foregroundStyle(brand.primary)
                }
                .buttonStyle(.borderless)
                .padding(.leading, 18)
            }
        }
        .padding(.vertical, 4)
        .contextMenu {
            if let domain = row.string("domain") {
                Button("复制内网域名") {
                    Pasteboard.copy(domain)
                }
            }
            if let address = row.string("address") {
                Button("复制虚拟地址") {
                    Pasteboard.copy(address)
                }
            }
        }
    }

    private func summary(children: [MeshObject]) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                MeshStateDot(state: row.string("state"))
                Text(row.string("name") ?? "")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(brand.text)
                    .lineLimit(1)
                if let role = MeshText.role(row.string("role")) {
                    MeshTag(text: role, foreground: brand.primary, background: brand.soft)
                }
                if row.bool("self") {
                    MeshTag(text: String(localized: "本机"), foreground: brand.text2, background: brand.raised)
                }
                ForEach(row.strings("marks"), id: \.self) { mark in
                    if let label = MeshText.mark(mark) {
                        MeshTag(text: label, foreground: brand.amberText, background: brand.warningSoft)
                    }
                }
                Spacer(minLength: 4)
                if !row.bool("self") {
                    MeshLatency(milliseconds: row.int("latencyMs"))
                }
            }
            Text(secondLine)
                .font(.caption)
                .foregroundStyle(brand.text2)
                .lineLimit(1)
                .padding(.leading, 14)
        }
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }

    private var secondLine: String {
        var parts: [String] = []
        if let domain = row.string("regionDomain") {
            parts.append(domain)
        }
        parts.append(MeshText.connection(row.string("connection"), hops: row.int("hops") ?? 0))
        if let transport = row.string("transport") {
            parts.append(transport.uppercased())
        }
        if let contains = row.object("contains"), let count = contains.int("count") {
            parts.append(contains.string("noun") == "vms" ? String(localized: "\(count) 台虚拟机") : String(localized: "\(count) 个子网"))
        }
        return parts.joined(separator: " · ")
    }
}

struct MeshChildRow: View {
    let child: MeshObject
    @Environment(\.brand) private var brand

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: symbol)
                .font(.caption)
                .foregroundStyle(brand.text2)
                .frame(width: 16)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 1) {
                Text(child.string("name") ?? "")
                    .font(.footnote.weight(child.bool("match") ? .semibold : .regular))
                    .foregroundStyle(brand.text)
                if let detail = child.string("domain") ?? child.string("address") {
                    Text(detail)
                        .font(.caption2.monospaced())
                        .foregroundStyle(brand.text2)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.leading, 18)
        .accessibilityElement(children: .combine)
    }

    private var symbol: String {
        switch child.string("kind") {
        case "vm":
            return "shippingbox"
        case "device":
            return "printer"
        default:
            return "network"
        }
    }
}

/// One member in full: state, internal domain name and virtual address (both copyable), region and its basis,
/// role, the current path hop by hop, the path state and what the member advertises or hosts.
struct MeshMemberDetail: View {
    let id: String
    var tunnel: TunnelController
    var model: MeshModel
    @Environment(\.brand) private var brand

    var body: some View {
        let status = MeshObject(model.status)
        let own = status.object("self")
        let isSelf = own?.string("id") == id
        let member = isSelf ? own : status.objects("members").first { $0.string("id") == id }
        List {
            if let member {
                Section {
                    LabeledContent("状态") {
                        HStack(spacing: 6) {
                            let state = isSelf ? "online" : member.string("state")
                            MeshStateDot(state: state)
                            Text(state == "unreachable" ? String(localized: "不可达") : String(localized: "在线"))
                                .foregroundStyle(brand.text2)
                        }
                    }
                    .listRowBackground(brand.surface)
                    if let domain = member.string("domain") {
                        MeshCopyRow(title: "内网域名", value: domain)
                    }
                    if let address = member.string("address") {
                        MeshCopyRow(title: "虚拟地址", value: address)
                    }
                    if let address = member.string("addressV6") {
                        MeshCopyRow(title: "虚拟地址（IPv6）", value: address)
                    }
                    if let role = MeshText.role(member.string("role")) {
                        LabeledContent("角色") {
                            Text(role).foregroundStyle(brand.text2)
                        }
                        .listRowBackground(brand.surface)
                    }
                    if let region = member.string("region") {
                        LabeledContent("地区") {
                            Text(regionName(region, status: status)).foregroundStyle(brand.text2)
                        }
                        .listRowBackground(brand.surface)
                    }
                }
                notes(member, status: status)
                if !isSelf {
                    Section("当前路径") {
                        LabeledContent("连接") {
                            Text(MeshText.connection(member.string("connection"), hops: member.int("hops") ?? 0))
                                .foregroundStyle(brand.text2)
                        }
                        .listRowBackground(brand.surface)
                        let hops = member.strings("path")
                        if !hops.isEmpty {
                            Text(hops.map { name($0, status: status) }.joined(separator: " → "))
                                .font(.subheadline)
                                .foregroundStyle(brand.text)
                                .listRowBackground(brand.surface)
                        }
                        LabeledContent("路径") {
                            Text(MeshText.pathState(member.string("pathState")))
                                .foregroundStyle(brand.text2)
                        }
                        .listRowBackground(brand.surface)
                        LabeledContent("延迟") {
                            MeshLatency(milliseconds: member.int("latencyMs"))
                        }
                        .listRowBackground(brand.surface)
                    }
                }
                let subnets = member.strings("subnets")
                if !subnets.isEmpty {
                    Section("通告子网") {
                        ForEach(subnets, id: \.self) { subnet in
                            Text(subnet)
                                .font(.subheadline.monospaced())
                                .listRowBackground(brand.surface)
                        }
                    }
                }
                let vms = member.objects("vms")
                if !vms.isEmpty {
                    Section("虚拟机") {
                        ForEach(Array(vms.enumerated()), id: \.offset) { _, vm in
                            MeshChildRow(child: vm)
                                .listRowBackground(brand.surface)
                        }
                    }
                }
            } else {
                Text("该成员已离开网络。")
                    .foregroundStyle(brand.text2)
                    .listRowBackground(brand.surface)
            }
        }
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle(member?.string("name") ?? "")
        .inlineNavigationTitle()
        .task(id: tunnel.vpnConnection) {
            await model.poll(tunnel)
        }
    }

    @ViewBuilder
    private func notes(_ member: MeshObject, status: MeshObject) -> some View {
        let duplicates = member.strings("duplicateOf")
        let unmanaged = member.raw["addressManaged"] as? Bool == false
            && ["border", "gateway", "host", "server"].contains(member.string("role") ?? "")
        if !duplicates.isEmpty || unmanaged {
            Section {
                if !duplicates.isEmpty {
                    let names = duplicates.map { name($0, status: status) }.formatted(.list(type: .and))
                    Text("与 \(names) 重名。")
                        .font(.subheadline)
                        .listRowBackground(brand.surface)
                }
                if unmanaged {
                    Text("地址未受管：虚拟地址可能变化，内网域名可能随之失效。")
                        .font(.subheadline)
                        .listRowBackground(brand.surface)
                }
            }
        }
    }

    private func name(_ id: String, status: MeshObject) -> String {
        if status.object("self")?.string("id") == id {
            return status.object("self")?.string("name") ?? id
        }
        return status.objects("members").first { $0.string("id") == id }?.string("name") ?? id
    }

    private func regionName(_ code: String, status: MeshObject) -> String {
        status.objects("regions").first { $0.string("code") == code }?.string("name") ?? code
    }
}

/// A subnet: the network, who advertises it (使用中 and 备用 when several do), the path, the latency and the marks.
/// A subnet that overlaps this device's network uses the local network unless switched to the mesh.
struct MeshSubnetRow: View {
    let row: MeshObject
    var tunnel: TunnelController
    var model: MeshModel
    @Environment(\.brand) private var brand

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                MeshStateDot(state: row.string("state") == "ok" ? "online" : "unreachable")
                Text(row.string("cidr") ?? "")
                    .font(.subheadline.weight(.semibold).monospaced())
                    .foregroundStyle(brand.text)
                ForEach(row.strings("marks"), id: \.self) { mark in
                    if let label = MeshText.mark(mark) {
                        MeshTag(text: label, foreground: brand.amberText, background: brand.warningSoft)
                    }
                }
                Spacer(minLength: 4)
                if row.string("state") != "refused" {
                    MeshLatency(milliseconds: row.int("latencyMs"))
                }
            }
            Text(detailLine)
                .font(.caption)
                .foregroundStyle(brand.text2)
                .lineLimit(2)
                .padding(.leading, 14)
            let advertisers = row.objects("advertisers")
            if advertisers.count > 1 {
                ForEach(Array(advertisers.enumerated()), id: \.offset) { _, advertiser in
                    let use = advertiser.bool("inUse") ? String(localized: "使用中") : String(localized: "备用")
                    Text(verbatim: "\(advertiser.string("name") ?? "") · \(use)")
                        .font(.caption)
                        .foregroundStyle(brand.text2)
                        .padding(.leading, 14)
                }
            }
            let overlaps = row.strings("overlaps")
            if !overlaps.isEmpty {
                Text("与 \(overlaps.formatted(.list(type: .and))) 重叠")
                    .font(.caption)
                    .foregroundStyle(brand.amberText)
                    .padding(.leading, 14)
            }
            if row.strings("marks").contains("local_overlap"), let cidr = row.string("cidr") {
                Toggle(
                    "走组网",
                    isOn: Binding(
                        get: { row.string("route") == "mesh" },
                        set: { value in
                            Task {
                                await model.setRoute(value ? "mesh" : "local", cidr: cidr, tunnel: tunnel)
                            }
                        }
                    )
                )
                .font(.footnote)
                .disabled(model.busy)
                .padding(.leading, 14)
            }
        }
        .padding(.vertical, 4)
    }

    private var detailLine: String {
        if row.string("state") == "refused" {
            return String(localized: "公网网段，未安装路由")
        }
        var parts: [String] = []
        if let advertiser = row.string("advertiser") {
            parts.append(advertiser)
        }
        if let region = row.string("regionName") {
            parts.append(region)
        }
        let path = row.strings("path")
        if !path.isEmpty {
            parts.append(path.joined(separator: " → "))
        }
        if row.string("state") == "unreachable" {
            parts.append(String(localized: "不可达"))
        }
        return parts.joined(separator: " · ")
    }
}

/// A pair of regions: the current path (直连 or via a third region), the border members, latency, loss and state.
struct MeshLinkRow: View {
    let link: MeshObject
    @Environment(\.brand) private var brand

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                MeshStateDot(state: link.string("state") == "down" ? "unreachable" : (link.string("state") == "detour" ? "partial" : "online"))
                Text("\(link.string("fromName") ?? "") – \(link.string("toName") ?? "")")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(brand.text)
                Spacer(minLength: 4)
                MeshLatency(milliseconds: link.int("latencyMs"))
            }
            Text(detailLine)
                .font(.caption)
                .foregroundStyle(brand.text2)
                .padding(.leading, 14)
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
    }

    private var detailLine: String {
        var parts: [String] = []
        switch link.string("state") {
        case "detour":
            parts.append(String(localized: "已自动切换，经\(link.string("viaName") ?? "")"))
        case "down":
            parts.append(String(localized: "中断"))
        default:
            parts.append(String(localized: "mesh.connection.direct"))
        }
        let borders = link.strings("borders")
        if !borders.isEmpty {
            parts.append(borders.formatted(.list(type: .and)))
        }
        if let loss = link.double("loss") {
            let percent = Int((loss * 100).rounded())
            parts.append(String(localized: "丢包 \(percent)%"))
        }
        return parts.joined(separator: " · ")
    }
}

/// A custom path: the target (a member or a subnet, with who advertises it), the waypoints and the state.
struct MeshCustomPathRow: View {
    let path: MeshObject
    @Environment(\.brand) private var brand

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                Text(path.string("targetLabel") ?? "")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(brand.text)
                Spacer(minLength: 4)
                Text(stateText)
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(["active", "pending"].contains(path.string("state")) ? brand.text2 : brand.amberText)
            }
            Text(detailLine)
                .font(.caption)
                .foregroundStyle(brand.text2)
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
    }

    private var stateText: String {
        switch path.string("state") {
        case "fallback":
            return String(localized: "已回落")
        case "first_hop_only":
            return String(localized: "仅首跳生效")
        case "pending":
            return String(localized: "待确认")
        default:
            return String(localized: "生效")
        }
    }

    private var detailLine: String {
        var parts: [String] = []
        let via = path.strings("via")
        if !via.isEmpty {
            parts.append(String(localized: "经 \(via.joined(separator: " → "))"))
        }
        if let advertiser = path.string("advertiser") {
            parts.append(String(localized: "由 \(advertiser) 通告"))
        }
        // `reason` (mesh-status.md): a node that does not forward on this path is not an unreachable one.
        let state = path.string("state")
        if state == "fallback" || (state == "first_hop_only" && path.string("reason") != nil) {
            switch path.string("reason") {
            case "no_echo":
                if let node = path.string("unreachable") {
                    parts.append(String(localized: "\(node) 探测无响应"))
                }
            case "unsupported", "not_relay":
                if let node = path.string("unreachable") {
                    parts.append(String(localized: "\(node) 不支持按此路径转发"))
                }
            case "hop_limit":
                parts.append(String(localized: "跳数超出上限"))
            default:
                if let unreachable = path.string("unreachable") {
                    parts.append(String(localized: "\(unreachable) 不可达"))
                }
            }
        }
        return parts.joined(separator: " · ")
    }
}
