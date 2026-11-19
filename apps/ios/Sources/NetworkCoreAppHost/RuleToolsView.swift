import NetworkCoreBridge
import SwiftUI

/// The `GEOIP` and `IP-ASN` data lists. Only the mainland China list is built in, so `GEOIP,US` or `IP-ASN,13335`
/// need a list the user adds by URL. While connected, the lists the running rules use and nobody provides come first,
/// so a rule that matches nothing is never silent. Saving reconnects a running tunnel.
struct DataListsSection: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var missing: [String] = []
    @State private var name = ""
    @State private var url = ""
    @State private var failure: String?

    private var sources: [LocalRules.DataSource] {
        tunnel.localRules.dataSources
    }

    private var lacking: [String] {
        let added = Set(sources.map(\.name))
        return missing.filter { !added.contains($0) }
    }

    private var target: String? {
        LocalRules.DataSource.name(from: name)
    }

    var body: some View {
        Section {
            ForEach(lacking, id: \.self) { item in
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("缺少 \(LocalRules.DataSource.rule(for: item)) 的数据")
                            .foregroundStyle(brand.warning)
                        Text("这条规则现在不会匹配任何连接。")
                            .font(.caption)
                            .foregroundStyle(brand.text2)
                    }
                    Spacer()
                    Button("添加列表") {
                        name = item
                    }
                    .buttonStyle(.borderless)
                }
                .listRowBackground(brand.surface)
            }
            ForEach(sources) { source in
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: source.rule)
                        .foregroundStyle(brand.text)
                    Text(verbatim: source.url)
                        .font(.caption)
                        .foregroundStyle(brand.text2)
                        .lineLimit(2)
                        .textSelection(.enabled)
                }
                .padding(.vertical, 2)
                .listRowBackground(brand.surface)
                .swipeActions {
                    Button("删除", role: .destructive) {
                        tunnel.updateLocalRules(tunnel.localRules.removing(dataSource: source.name))
                    }
                }
                .contextMenu {
                    Button("删除", role: .destructive) {
                        tunnel.updateLocalRules(tunnel.localRules.removing(dataSource: source.name))
                    }
                }
            }
            TextField("国家代码或 AS 号", text: $name, prompt: Text(verbatim: "US, AS13335"))
                .plainTextEntry(url: false)
                .listRowBackground(brand.surface)
            TextField("列表地址", text: $url, prompt: Text(verbatim: "https://"))
                .plainTextEntry(url: true)
                .listRowBackground(brand.surface)
            if !name.trimmingCharacters(in: .whitespaces).isEmpty, target == nil {
                Text("写两个字母的国家代码（如 US）或 AS 号（如 AS13335）。")
                    .font(.caption)
                    .foregroundStyle(brand.warning)
                    .listRowBackground(brand.surface)
            }
            if let failure {
                Text(failure)
                    .font(.caption)
                    .foregroundStyle(brand.danger)
                    .listRowBackground(brand.surface)
            }
            Button("添加") {
                add()
            }
            .disabled(target == nil || !url.trimmingCharacters(in: .whitespaces).hasPrefix("https://"))
            .listRowBackground(brand.surface)
        } header: {
            Text("GeoIP 与 ASN 数据")
        } footer: {
            Text("客户端只内置中国大陆的 GeoIP 列表。GEOIP 的其他国家和 IP-ASN 规则需要你按 URL 添加开源列表（每行一个网段，或 IP-CIDR 规则），经代理每天下载一次，并替换同名的内置或订阅列表。")
        }
        .task(id: tunnel.connection) {
            await load()
        }
    }

    private func add() {
        guard let target else {
            return
        }
        do {
            tunnel.updateLocalRules(try tunnel.localRules.saving(dataSource: target, url: url))
            name = ""
            url = ""
            failure = nil
        } catch LocalRules.Failure.tooMany {
            failure = String(localized: "最多 16 个数据列表。")
        } catch {
            failure = String(localized: "列表必须是 https:// 地址。")
        }
    }

    private func load() async {
        guard tunnel.connection == .connected,
              let answer = RuntimeControl.decode(RuntimeControl.RuleSets.self, from: await tunnel.control(.ruleSets))
        else {
            missing = []
            return
        }
        missing = answer.missing
    }
}

/// Names for the devices that use the Mac as their gateway, by the address their connections come from, for
/// `DEVICE-NAME` rules. Saving reconnects a running tunnel.
struct DeviceNamesSection: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var address = ""
    @State private var name = ""
    @State private var failure: String?

    private var devices: [(address: String, name: String)] {
        tunnel.localRules.devices.map { (address: $0.key, name: $0.value) }.sorted { $0.address < $1.address }
    }

    var body: some View {
        Section {
            ForEach(devices, id: \.address) { device in
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: device.name)
                        .foregroundStyle(brand.text)
                    Text(verbatim: device.address)
                        .font(.caption.monospaced())
                        .foregroundStyle(brand.text2)
                }
                .padding(.vertical, 2)
                .listRowBackground(brand.surface)
                .contextMenu {
                    Button("删除", role: .destructive) {
                        tunnel.updateLocalRules(tunnel.localRules.removing(device: device.address))
                    }
                }
            }
            TextField("设备 IP", text: $address, prompt: Text(verbatim: "192.168.1.30"))
                .plainTextEntry(url: true)
                .listRowBackground(brand.surface)
            TextField("名字", text: $name, prompt: Text(verbatim: "Apple TV"))
                .plainTextEntry(url: false)
                .listRowBackground(brand.surface)
            if let failure {
                Text(failure)
                    .font(.caption)
                    .foregroundStyle(brand.danger)
                    .listRowBackground(brand.surface)
            }
            Button("添加") {
                add()
            }
            .disabled(address.trimmingCharacters(in: .whitespaces).isEmpty || name.trimmingCharacters(in: .whitespaces).isEmpty)
            .listRowBackground(brand.surface)
        } header: {
            Text("设备名")
        } footer: {
            Text("给把 Mac 当作网关的设备起名字，DEVICE-NAME 规则按名字匹配（不区分大小写）。设备按连接来源的 IP 认出，最好在路由器里给它固定地址。右键可删除。")
        }
    }

    private func add() {
        do {
            tunnel.updateLocalRules(try tunnel.localRules.saving(device: address, name: name))
            address = ""
            name = ""
            failure = nil
        } catch LocalRules.Failure.tooMany {
            failure = String(localized: "最多 \(LocalRules.maxDevices) 个设备名。")
        } catch {
            failure = String(localized: "写设备的 IP 地址和 1 到 64 个字符的名字（不含逗号）。")
        }
    }
}

/// Asks the running engine which rule decides a connection and where it goes, without opening or counting one.
struct RuleTestSection: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var host = ""
    @State private var port = "443"
    @State private var udp = false
    @State private var address = ""
    @State private var answer: RuntimeControl.RouteTestAnswer?
    @State private var failed = false
    @State private var testing = false

    var body: some View {
        Section {
            TextField("域名或 IP", text: $host, prompt: Text(verbatim: "example.com"))
                .plainTextEntry(url: true)
                .listRowBackground(brand.surface)
            TextField("端口", text: $port, prompt: Text(verbatim: "443"))
                .plainTextEntry(url: false)
                .listRowBackground(brand.surface)
            Toggle(isOn: $udp) {
                Text(verbatim: "UDP")
            }
            .listRowBackground(brand.surface)
            TextField("域名解析到的 IP（可选）", text: $address)
                .plainTextEntry(url: true)
                .listRowBackground(brand.surface)
            Button {
                Task {
                    await test()
                }
            } label: {
                if testing {
                    ProgressView()
                } else {
                    Label("测试", systemImage: "point.topleft.down.to.point.bottomright.curvepath")
                }
            }
            .disabled(testing || host.trimmingCharacters(in: .whitespaces).isEmpty)
            .listRowBackground(brand.surface)
            if failed {
                Text("内核没有回答，请确认已连接。")
                    .font(.caption)
                    .foregroundStyle(brand.danger)
                    .listRowBackground(brand.surface)
            }
            if let answer {
                LabeledContent("匹配的规则") {
                    Text(verbatim: answer.test.rule)
                        .font(.footnote.monospaced())
                        .textSelection(.enabled)
                }
                .listRowBackground(brand.surface)
                LabeledContent("出口") {
                    Text(verbatim: answer.test.path)
                }
                .listRowBackground(brand.surface)
                if answer.test.needsAddress {
                    Text("这条规则要看目标 IP。填上域名解析到的 IP 再测；解析失败时连接会被拒绝。")
                        .font(.caption)
                        .foregroundStyle(brand.warning)
                        .listRowBackground(brand.surface)
                }
                if answer.mode != "rule" {
                    Text(answer.mode == "global" ? "当前是全局模式，实际连接不按规则分流。" : "当前是直连模式，实际连接不按规则分流。")
                        .font(.caption)
                        .foregroundStyle(brand.warning)
                        .listRowBackground(brand.surface)
                }
            }
        } header: {
            Text("规则测试")
        } footer: {
            Text("查看当前规则会把这样的连接交给哪条规则、走哪个出口。不会真的发起连接，也不计入命中次数。")
        }
    }

    private func test() async {
        testing = true
        defer {
            testing = false
        }
        let request = RuntimeControl.testRoute(
            host: host.trimmingCharacters(in: .whitespaces),
            port: UInt16(port.trimmingCharacters(in: .whitespaces)) ?? 443,
            udp: udp,
            address: address
        )
        answer = RuntimeControl.decode(RuntimeControl.RouteTestAnswer.self, from: await tunnel.control(request))
        failed = answer == nil
    }
}

/// How often each rule decided a connection since the rules were loaded, busiest first, while connected.
struct RuleHitsSection: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @State private var hits: RuntimeControl.RuleHits?

    var body: some View {
        if tunnel.connection == .connected {
            Section {
                if let hits {
                    let busiest = hits.busiest()
                    if busiest.isEmpty, hits.finalRule.hits == 0 {
                        Text("还没有连接经过规则。")
                            .foregroundStyle(brand.text2)
                            .listRowBackground(brand.surface)
                    }
                    ForEach(busiest, id: \.index) { item in
                        row(item.hit)
                    }
                    row(hits.finalRule)
                    if hits.unused > 0 {
                        Text("另有 \(hits.unused) 条规则还没有命中。")
                            .font(.caption)
                            .foregroundStyle(brand.text2)
                            .listRowBackground(brand.surface)
                    }
                    Button("清零") {
                        Task {
                            self.hits = RuntimeControl.decode(
                                RuntimeControl.RuleHits.self, from: await tunnel.control(.clearRuleHits)
                            ) ?? self.hits
                        }
                    }
                    .listRowBackground(brand.surface)
                } else {
                    ProgressView()
                        .listRowBackground(brand.surface)
                }
            } header: {
                Text("规则命中")
            } footer: {
                Text("规则加载以来每条规则决定了多少个连接。修改规则后重新计数。")
            }
            .task {
                while !Task.isCancelled, tunnel.connection == .connected {
                    hits = RuntimeControl.decode(RuntimeControl.RuleHits.self, from: await tunnel.control(.ruleHits))
                    try? await Task.sleep(for: .seconds(5))
                }
                hits = nil
            }
        }
    }

    private func row(_ hit: RuntimeControl.RuleHits.Hit) -> some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 2) {
                Text(verbatim: hit.rule)
                    .font(.footnote.monospaced())
                    .foregroundStyle(brand.text)
                    .lineLimit(2)
                Text(verbatim: hit.target)
                    .font(.caption)
                    .foregroundStyle(brand.text2)
            }
            Spacer()
            Text(verbatim: "\(hit.hits)")
                .font(.footnote.monospacedDigit())
                .foregroundStyle(brand.text2)
        }
        .listRowBackground(brand.surface)
    }
}

/// A warning about subscription rules that match nothing on this device.
struct RuleNote: View {
    let text: String
    @Environment(\.brand) private var brand

    var body: some View {
        Section {
            Text(text)
                .font(.caption)
                .foregroundStyle(brand.warning)
                .listRowBackground(brand.surface)
        }
    }
}

/// `PROCESS-NAME` rules match nothing here: the tunnel cannot tell which app opened a connection on iOS and macOS.
struct ProcessRulesNote: View {
    @Environment(\.brand) private var brand

    var body: some View {
        Section {
            Text("订阅里有 PROCESS-NAME 规则。iOS 和 macOS 的隧道分不出连接来自哪个 App，这些规则不会生效。")
                .font(.caption)
                .foregroundStyle(brand.warning)
                .listRowBackground(brand.surface)
        }
    }
}
