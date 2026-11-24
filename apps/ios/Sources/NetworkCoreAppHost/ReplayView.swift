import NetworkCoreBridge
import SwiftUI

/// Sends one captured request again, as captured or edited (plan E1), and shows what came back. The request goes
/// out through URLSession, so with the tunnel connected it is captured like any other.
struct ReplayView: View {
    let original: RequestReplay
    /// The captured body was cut off: sending it unchanged would not repeat the request.
    let complete: Bool
    /// "Replay" sends at once; "edit and replay" waits for the user.
    let sendOnAppear: Bool
    @Environment(\.brand) private var brand
    @State private var method: String
    @State private var url: String
    @State private var headerText: String
    @State private var bodyText: String
    @State private var binaryBody: Bool
    @State private var sending = false
    @State private var result: ReplayResult?
    @State private var failure: String?
    @State private var sentOnAppear = false
    @State private var sender = RequestReplaySender()

    init(original: RequestReplay, complete: Bool, sendOnAppear: Bool) {
        self.original = original
        self.complete = complete
        self.sendOnAppear = sendOnAppear
        _method = State(initialValue: original.method)
        _url = State(initialValue: original.url)
        _headerText = State(initialValue: original.headerText)
        let text = String(data: original.body, encoding: .utf8)
        _bodyText = State(initialValue: text ?? "")
        _binaryBody = State(initialValue: text == nil && !original.body.isEmpty)
    }

    var body: some View {
        Form {
            Section {
                TextField("方法", text: $method)
                    .font(.footnote.monospaced())
                    .plainTextEntry()
                TextField("URL", text: $url, axis: .vertical)
                    .font(.footnote.monospaced())
                    .plainTextEntry(url: true)
            } header: {
                Text("请求")
            } footer: {
                if !complete {
                    Text("抓到的请求体不完整，未带上；可在下方补全。")
                }
            }
            .listRowBackground(brand.surface)

            Section {
                TextField("请求头", text: $headerText, prompt: Text(verbatim: "Accept: application/json"), axis: .vertical)
                    .font(.footnote.monospaced())
                    .lineLimit(2...12)
                    .plainTextEntry()
            } header: {
                Text("请求头")
            } footer: {
                Text("每行一个，写“名称: 值”。Host、Content-Length 由系统填写。")
            }
            .listRowBackground(brand.surface)

            Section("请求体") {
                if binaryBody {
                    LabeledContent("请求体", value: String(localized: "二进制内容 · \(TrafficFormat.bytes(UInt64(original.body.count)))"))
                    Button("改为文本") {
                        binaryBody = false
                        bodyText = ""
                    }
                } else {
                    TextEditor(text: $bodyText)
                        .font(.footnote.monospaced())
                        .frame(minHeight: 120)
                        .plainTextEntry()
                }
            }
            .listRowBackground(brand.surface)

            if let failure {
                Section {
                    Text(failure)
                        .foregroundStyle(brand.danger)
                }
                .listRowBackground(brand.surface)
            }

            if let result {
                resultSections(result)
            }
        }
        .formStyle(.grouped)
        .scrollContentBackground(.hidden)
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("重放请求")
        .inlineNavigationTitle()
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                if sending {
                    ProgressView()
                } else {
                    Button("发送") {
                        send()
                    }
                }
            }
        }
        .onAppear {
            if sendOnAppear && !sentOnAppear {
                sentOnAppear = true
                send()
            }
        }
    }

    @ViewBuilder
    private func resultSections(_ result: ReplayResult) -> some View {
        Section {
            LabeledContent("状态", value: String(result.status))
            LabeledContent("耗时", value: "\(Int((result.duration * 1000).rounded())) ms")
            LabeledContent("大小", value: TrafficFormat.bytes(UInt64(result.body.count)))
        } header: {
            Text("响应")
        } footer: {
            Text("连接中时这次请求也会出现在抓包里。重定向不会自动跟随。")
        }
        .listRowBackground(brand.surface)
        Section("响应头") {
            ForEach(Array(result.headers.enumerated()), id: \.offset) { _, header in
                VStack(alignment: .leading, spacing: 2) {
                    Text(header.name)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(brand.text2)
                    Text(header.value)
                        .font(.footnote.monospaced())
                        .textSelection(.enabled)
                }
            }
        }
        .listRowBackground(brand.surface)
        if !result.body.isEmpty {
            Section("响应体") {
                if let text = result.bodyText {
                    Text(verbatim: String(text.prefix(64 * 1024)))
                        .font(.footnote.monospaced())
                        .textSelection(.enabled)
                } else {
                    Text("二进制内容 · \(TrafficFormat.bytes(UInt64(result.body.count)))")
                        .foregroundStyle(brand.text2)
                }
            }
            .listRowBackground(brand.surface)
        }
    }

    private func send() {
        let replay: RequestReplay
        do {
            replay = RequestReplay(
                method: method,
                url: url,
                headers: try RequestReplay.headers(from: headerText),
                body: binaryBody ? original.body : Data(bodyText.utf8)
            )
            _ = try replay.urlRequest()
        } catch let error as RequestReplayError {
            failure = error.message
            return
        } catch {
            failure = error.localizedDescription
            return
        }
        failure = nil
        sending = true
        Task {
            do {
                result = try await sender.send(replay)
            } catch let error as RequestReplayError {
                result = nil
                failure = error.message
            } catch {
                result = nil
                failure = String(localized: "发送失败：\(error.localizedDescription)")
            }
            sending = false
        }
    }
}
