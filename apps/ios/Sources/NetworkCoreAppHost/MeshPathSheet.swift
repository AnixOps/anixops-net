import NetworkCoreBridge
import SwiftUI

/// 添加路径 on phones (docs/mesh-ui.md, 路径): target kind, target, waypoints in order, and the path it gives.
/// A path to a subnet names who advertises it. Kernel 0.5.
struct MeshPathSheet: View {
    let status: [String: Any]?
    let save: (MeshPaths.Draft) async -> Bool
    @Environment(\.brand) private var brand
    @Environment(\.dismiss) private var dismiss
    @State private var isMember = true
    @State private var target = ""
    @State private var via: [String] = []
    @State private var error: String?
    @State private var saving = false

    /// Members other than this device, by id: name.
    private var members: [(id: String, name: String)] {
        (status?["members"] as? [[String: Any]] ?? []).compactMap { row in
            guard let id = row["id"] as? String else {
                return nil
            }
            return (id, row["name"] as? String ?? id)
        }
    }

    /// Subnets with a route, with who advertises them.
    private var subnets: [(cidr: String, advertiser: String?)] {
        let rows = MeshObject(MeshView.subnets(status: status, query: "")).objects("rows")
        return rows.filter { $0.string("state") != "refused" }.compactMap { row in
            row.string("cidr").map { ($0, row.string("advertiser")) }
        }
    }

    private func name(_ id: String) -> String {
        members.first { $0.id == id }?.name ?? id
    }

    private var preview: String {
        let end = isMember ? name(target) : target
        return ([String(localized: "本机")] + via.map(name) + (end.isEmpty ? [] : [end])).joined(separator: " → ")
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Picker("目标类型", selection: $isMember) {
                        Text("成员").tag(true)
                        Text("子网").tag(false)
                    }
                    .pickerStyle(.segmented)
                    .onChange(of: isMember) {
                        target = ""
                    }
                    Picker("目标", selection: $target) {
                        Text("选择目标").tag("")
                        if isMember {
                            ForEach(members, id: \.id) { member in
                                Text(verbatim: member.name).tag(member.id)
                            }
                        } else {
                            ForEach(subnets, id: \.cidr) { subnet in
                                Text(verbatim: subnet.cidr).tag(subnet.cidr)
                            }
                        }
                    }
                    .onChange(of: target) {
                        via.removeAll { $0 == target }
                    }
                } footer: {
                    if !isMember, let advertiser = subnets.first(where: { $0.cidr == target })?.advertiser {
                        Text("由 \(advertiser) 通告")
                    }
                }
                .listRowBackground(brand.surface)
                Section {
                    ForEach(Array(via.enumerated()), id: \.offset) { _, id in
                        Text(verbatim: name(id))
                    }
                    .onMove { from, to in
                        via.move(fromOffsets: from, toOffset: to)
                    }
                    .onDelete { offsets in
                        via.remove(atOffsets: offsets)
                    }
                    Menu {
                        ForEach(members.filter { $0.id != target }, id: \.id) { member in
                            Button {
                                via.append(member.id)
                            } label: {
                                Text(verbatim: member.name)
                            }
                        }
                    } label: {
                        Label("添加途经点", systemImage: "plus")
                    }
                    .disabled(via.count >= MeshPaths.maxWaypoints)
                } header: {
                    Text("途经点")
                } footer: {
                    Text(verbatim: preview)
                }
                .listRowBackground(brand.surface)
                if let error {
                    Section {
                        Label(error, systemImage: "exclamationmark.circle.fill")
                            .foregroundStyle(brand.danger)
                    }
                    .listRowBackground(brand.surface)
                }
            }
            .scrollContentBackground(.hidden)
            .background(brand.bg.ignoresSafeArea())
            .safeAreaInset(edge: .bottom, spacing: 0) {
                Button {
                    submit()
                } label: {
                    Text("保存")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(BrandButtonStyle())
                .disabled(saving)
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .background(brand.bg)
            }
            .navigationTitle("添加路径")
            .inlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("取消") {
                        dismiss()
                    }
                }
            }
        }
    }

    private func submit() {
        if target.isEmpty {
            error = String(localized: "选择目标。")
            return
        }
        if via.isEmpty {
            error = String(localized: "至少选择一个途经点。")
            return
        }
        error = nil
        saving = true
        Task {
            let saved = await save(MeshPaths.Draft(isMember: isMember, target: target, via: via))
            saving = false
            if saved {
                dismiss()
            } else {
                error = String(localized: "路径中的成员缺少虚拟地址。")
            }
        }
    }
}
