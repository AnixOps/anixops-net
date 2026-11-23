// `anixops`: the command-line tool of the macOS app (docs/macos-cli.md). Ships inside the app bundle at
// Contents/MacOS/anixops; the user links it into their PATH. It sends one request to the running app over the
// app's local socket and prints the answer. Parsing, exit codes and output live in NetworkCoreBridge
// (CommandLine.swift), where they are unit tested.

import AppKit
import Darwin
import Foundation
import MachO
import NetworkCoreBridge

func printError(_ text: String) {
    FileHandle.standardError.write(Data((text + "\n").utf8))
}

func finish(_ code: CommandLineExit) -> Never {
    exit(code.rawValue)
}

func toolVersion() -> String {
    Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "dev"
}

/// The home folder of the user running the tool, also when HOME is changed.
func homeFolder() -> String {
    if let entry = getpwuid(getuid()), let directory = entry.pointee.pw_dir {
        return String(cString: directory)
    }
    return NSHomeDirectory()
}

/// This executable's path as the system started it (the PATH entry when run through a symlink). `argv[0]` is only
/// the bare name then, which would be read against the current folder.
func executablePath() -> URL? {
    var size: UInt32 = 0
    _ = _NSGetExecutablePath(nil, &size)
    if size > 0 {
        var buffer = [CChar](repeating: 0, count: Int(size) + 1)
        if _NSGetExecutablePath(&buffer, &size) == 0 {
            return URL(fileURLWithPath: String(cString: buffer))
        }
    }
    return Bundle.main.executableURL
}

/// The app this tool belongs to: the bundle around Contents/MacOS/anixops, through a symlink in PATH.
func enclosingApp() -> URL? {
    executablePath().flatMap { CommandLineTool.enclosingApp(ofExecutable: $0) }
}

final class Flag: @unchecked Sendable {
    private let lock = NSLock()
    private var stored = false

    var value: Bool {
        lock.lock()
        defer { lock.unlock() }
        return stored
    }

    func set(_ value: Bool) {
        lock.lock()
        stored = value
        lock.unlock()
    }
}

/// Opens the app in the background and waits until it accepts connections. Not until the socket file exists: one
/// left by an app that quit or crashed is there before the new app has read its configuration and listens.
func launchApp(socket: String) -> Bool {
    guard let app = enclosingApp() else {
        return false
    }
    let configuration = NSWorkspace.OpenConfiguration()
    configuration.activates = false
    let opened = DispatchSemaphore(value: 0)
    let launched = Flag()
    NSWorkspace.shared.openApplication(at: app, configuration: configuration) { application, _ in
        launched.set(application != nil)
        opened.signal()
    }
    _ = opened.wait(timeout: .now() + 10)
    guard launched.value else {
        return false
    }
    return ControlClient.waitUntilListening(at: socket, timeout: 30)
}

func readImport(_ source: CommandLineInvocation.ImportSource) -> String {
    let data: Data
    switch source {
    case let .text(text):
        return text
    case let .file(path):
        guard let contents = FileManager.default.contents(atPath: (path as NSString).expandingTildeInPath) else {
            printError("anixops: 读不到文件 \(path)")
            finish(.noInput)
        }
        data = contents
    case .standardInput:
        data = FileHandle.standardInput.readDataToEndOfFile()
    }
    guard data.count < ControlProtocol.maxMessageBytes / 2, let text = String(data: data, encoding: .utf8) else {
        printError("anixops: 导入内容太大或不是 UTF-8 文本")
        finish(.dataError)
    }
    return text
}

let invocation: CommandLineInvocation
switch CommandLineParser.parse(Array(CommandLine.arguments.dropFirst())) {
case let .success(parsed):
    invocation = parsed
case let .failure(error):
    printError("anixops: \(error.message)")
    printError("运行 anixops --help 查看用法")
    finish(.usage)
}

let request: ControlRequest
switch invocation.action {
case .help:
    print(CommandLineParser.usage)
    finish(.ok)
case .version:
    print("anixops \(toolVersion())")
    finish(.ok)
case let .request(parsed):
    request = parsed
case let .importNodes(source):
    request = ControlRequest(command: .importNodes, text: readImport(source))
}

let socket: String
if let override = invocation.socket {
    socket = override
} else if let group = Bundle.main.object(forInfoDictionaryKey: "AnixOpsAppGroup") as? String, !group.isEmpty {
    socket = ControlProtocol.socketPath(appGroup: group, home: homeFolder())
} else {
    printError("anixops: 这个构建没有 App 组标识，找不到 AnixOps Net")
    finish(.software)
}

func send() throws -> ControlResponse {
    try ControlClient.send(request, to: socket, timeout: invocation.effectiveTimeout)
}

/// Every command but status opens the app first when it is not running; status reports that it is not running.
func exchange() throws -> ControlResponse {
    do {
        return try send()
    } catch ControlTransportError.notRunning where request.command != .status && invocation.socket == nil {
        guard launchApp(socket: socket) else {
            throw ControlTransportError.notRunning
        }
        return try send()
    }
}

let response: ControlResponse
do {
    response = try exchange()
} catch let error as ControlTransportError {
    if !invocation.quiet {
        switch error {
        case .notRunning:
            printError("anixops: AnixOps Net 没有运行")
        case .timeout:
            printError("anixops: 等待 AnixOps Net 超时")
        case .protocolError:
            printError("anixops: AnixOps Net 的回答无法识别，请确认工具和 App 是同一版本")
        case .pathTooLong:
            printError("anixops: 套接字路径太长: \(socket)")
        case let .system(code):
            printError("anixops: 系统错误: \(String(cString: strerror(code)))")
        }
    }
    finish(CommandLineExit(error))
} catch {
    printError("anixops: \(error)")
    finish(.software)
}

if invocation.json {
    print(CommandLineOutput.json(response))
} else if !invocation.quiet {
    let text = CommandLineOutput.render(response, for: request.command)
    if !text.isEmpty {
        if response.ok {
            print(text)
        } else {
            printError(text)
        }
    }
}

guard response.ok else {
    finish(CommandLineExit(response.error ?? .failed))
}
if invocation.quiet, request.command == .status, response.status?.isConnected != true {
    finish(.notConnected)
}
finish(.ok)
