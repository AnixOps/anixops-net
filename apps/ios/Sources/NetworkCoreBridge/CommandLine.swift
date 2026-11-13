#if os(macOS)
import Foundation

// Argument parsing, exit codes and text output of the `anixops` command-line tool (docs/macos-cli.md). The tool's
// main.swift only does I/O; everything here is covered by unit tests.

/// Exit codes, following sysexits(3) where one fits.
public enum CommandLineExit: Int32, Equatable, Sendable {
    case ok = 0
    /// The app tried and failed (the tunnel did not come up, an import was refused).
    case failure = 1
    /// `status --quiet`: the app answered, but the tunnel is not connected (like `systemctl is-active`).
    case notConnected = 3
    /// EX_USAGE: wrong arguments, or a request not valid in the current state.
    case usage = 64
    /// EX_DATAERR: no such group or node, or import text with no usable node.
    case dataError = 65
    /// EX_NOINPUT: the import file cannot be read.
    case noInput = 66
    /// EX_UNAVAILABLE: the app is not running, or cannot do it now.
    case unavailable = 69
    /// EX_SOFTWARE: an internal error of the tool.
    case software = 70
    /// EX_OSERR: a system call failed.
    case osError = 71
    /// EX_TEMPFAIL: the app did not answer in time.
    case timeout = 75
    /// EX_PROTOCOL: the tool and the app speak different protocol versions, or the answer was garbled.
    case protocolError = 76

    public init(_ error: ControlErrorCode) {
        switch error {
        case .usage:
            self = .usage
        case .notFound, .invalidData:
            self = .dataError
        case .failed:
            self = .failure
        case .timeout:
            self = .timeout
        case .unavailable:
            self = .unavailable
        case .unsupportedVersion:
            self = .protocolError
        }
    }

    public init(_ error: ControlTransportError) {
        switch error {
        case .notRunning:
            self = .unavailable
        case .timeout:
            self = .timeout
        case .protocolError:
            self = .protocolError
        case .pathTooLong:
            self = .software
        case .system:
            self = .osError
        }
    }
}

public struct CommandLineInvocation: Equatable, Sendable {
    public enum ImportSource: Equatable, Sendable {
        /// A link or node text given on the command line.
        case text(String)
        case file(String)
        case standardInput
    }

    public enum Action: Equatable, Sendable {
        case help
        case version
        case request(ControlRequest)
        /// `import`: the tool reads the source, then sends an import request with its text.
        case importNodes(ImportSource)
    }

    public var action: Action
    public var json = false
    public var quiet = false
    /// Overrides the per-command default (`defaultTimeout`).
    public var timeout: TimeInterval?
    /// Overrides the socket path, for tests.
    public var socket: String?

    public init(action: Action, json: Bool = false, quiet: Bool = false, timeout: TimeInterval? = nil, socket: String? = nil) {
        self.action = action
        self.json = json
        self.quiet = quiet
        self.timeout = timeout
        self.socket = socket
    }

    /// How long to wait for the app: a delay test of every node or a subscription download takes a while.
    public var effectiveTimeout: TimeInterval {
        if let timeout {
            return timeout
        }
        switch action {
        case let .request(request):
            switch request.command {
            case .test:
                return 180
            case .connect, .disconnect, .select, .mode:
                return 45
            case .importNodes:
                return 90
            case .status, .nodes:
                return 10
            }
        case .importNodes:
            return 90
        case .help, .version:
            return 0
        }
    }
}

public struct CommandLineUsageError: Error, Equatable, Sendable {
    public let message: String
}

public enum CommandLineParser {
    public static let usage = """
    用法: anixops <命令> [选项]

    命令:
      status                        连接状态、节点和代理模式
      connect                       连接，等隧道连上或失败后返回
      disconnect                    断开
      mode [rule|global|direct]     查看或切换代理模式（规则 / 全局 / 直连）
      nodes [--group 订阅]          列出订阅组和节点，* 为当前节点
      select <节点> [--group 订阅]  切换到这个节点（名称或 ID）
      test [--group 订阅]           测速（经节点的 HTTP 延迟）并列出结果
      import <链接|文件|->          导入订阅链接、节点链接或节点列表（- 从标准输入读）
      version                       工具版本

    选项:
      --json            输出 JSON
      -q, --quiet       不输出；status 只用退出码表示连接状态
      --timeout <秒>    等待 App 的时间
      -h, --help        显示本说明

    退出码:
      0 成功          1 操作失败          3 未连接（status -q）
      64 用法错误     65 找不到订阅或节点，或导入内容无效
      66 读不到文件   69 App 没有运行     71 系统错误
      75 超时         76 App 与工具版本不符

    AnixOps Net 要在运行；命令行只控制当前用户的 App。
    """

    public static func parse(_ arguments: [String]) -> Result<CommandLineInvocation, CommandLineUsageError> {
        var json = false
        var quiet = false
        var timeout: TimeInterval?
        var socket: String?
        var group: String?
        var positional: [String] = []
        var index = 0
        func value(_ flag: String) throws -> String {
            index += 1
            guard index < arguments.count else {
                throw CommandLineUsageError(message: "\(flag) 需要一个值")
            }
            return arguments[index]
        }
        do {
            var literal = false
            while index < arguments.count {
                let argument = arguments[index]
                if literal || argument == "-" || !argument.hasPrefix("-") {
                    positional.append(argument)
                } else {
                    switch argument {
                    case "--":
                        literal = true
                    case "--json":
                        json = true
                    case "-q", "--quiet":
                        quiet = true
                    case "-h", "--help":
                        return .success(CommandLineInvocation(action: .help))
                    case "--group", "-g":
                        group = try value(argument)
                    case "--timeout":
                        guard let seconds = TimeInterval(try value(argument)), seconds > 0, seconds <= 3600 else {
                            throw CommandLineUsageError(message: "--timeout 要 1 到 3600 秒")
                        }
                        timeout = seconds
                    case "--socket":
                        socket = try value(argument)
                    default:
                        throw CommandLineUsageError(message: "不认识的选项 \(argument)")
                    }
                }
                index += 1
            }
        } catch let error as CommandLineUsageError {
            return .failure(error)
        } catch {
            return .failure(CommandLineUsageError(message: "\(error)"))
        }

        guard let name = positional.first else {
            return .failure(CommandLineUsageError(message: "缺少命令"))
        }
        let rest = Array(positional.dropFirst())
        func make(_ action: CommandLineInvocation.Action) -> Result<CommandLineInvocation, CommandLineUsageError> {
            .success(CommandLineInvocation(action: action, json: json, quiet: quiet, timeout: timeout, socket: socket))
        }
        func noArguments(_ command: ControlRequest.Command) -> Result<CommandLineInvocation, CommandLineUsageError> {
            guard rest.isEmpty else {
                return .failure(CommandLineUsageError(message: "\(name) 不接受参数 \(rest[0])"))
            }
            guard group == nil || [.nodes, .test].contains(command) else {
                return .failure(CommandLineUsageError(message: "\(name) 不接受 --group"))
            }
            return make(.request(ControlRequest(command: command, group: group)))
        }

        switch name {
        case "help":
            return make(.help)
        case "version":
            return rest.isEmpty ? make(.version) : .failure(CommandLineUsageError(message: "version 不接受参数"))
        case "status":
            return noArguments(.status)
        case "connect", "up":
            return noArguments(.connect)
        case "disconnect", "down":
            return noArguments(.disconnect)
        case "nodes", "ls":
            return noArguments(.nodes)
        case "test":
            return noArguments(.test)
        case "mode":
            guard rest.count <= 1, group == nil else {
                return .failure(CommandLineUsageError(message: "用法: anixops mode [rule|global|direct]"))
            }
            guard let text = rest.first else {
                return make(.request(ControlRequest(command: .mode)))
            }
            guard let mode = ProxyMode(rawValue: text.lowercased()) else {
                return .failure(CommandLineUsageError(message: "模式只能是 rule、global 或 direct"))
            }
            return make(.request(ControlRequest(command: .mode, mode: mode)))
        case "select", "use":
            guard rest.count == 1, !rest[0].isEmpty else {
                return .failure(CommandLineUsageError(message: "用法: anixops select <节点> [--group 订阅]"))
            }
            return make(.request(ControlRequest(command: .select, group: group, node: rest[0])))
        case "import":
            guard rest.count == 1, group == nil else {
                return .failure(CommandLineUsageError(message: "用法: anixops import <链接|文件|->"))
            }
            let source = rest[0]
            if source == "-" {
                return make(.importNodes(.standardInput))
            }
            if source.contains("://") {
                return make(.importNodes(.text(source)))
            }
            return make(.importNodes(.file(source)))
        default:
            return .failure(CommandLineUsageError(message: "不认识的命令 \(name)"))
        }
    }
}

/// Text output for people; `--json` prints the answer as is.
public enum CommandLineOutput {
    public static func render(_ response: ControlResponse, for command: ControlRequest.Command) -> String {
        guard response.ok else {
            return "错误: " + (response.message ?? response.error?.rawValue ?? "未知错误")
        }
        var lines: [String] = []
        switch command {
        case .status:
            if let status = response.status {
                lines += statusLines(status)
            }
        case .mode:
            if let status = response.status {
                lines.append("模式: \(modeName(status.mode)) (\(status.mode.rawValue))")
            }
        case .nodes, .test:
            for group in response.groups ?? [] {
                lines.append(group.active ? "[\(group.name)] 当前使用" : "[\(group.name)]")
                for node in group.nodes {
                    let mark = node.selected ? "*" : " "
                    lines.append("  \(mark) \(node.name)  \(node.protocol)\(latencyText(node).map { "  \($0)" } ?? "")")
                }
            }
            if lines.isEmpty {
                lines.append("还没有导入节点")
            }
        case .connect, .disconnect, .select, .importNodes:
            if let message = response.message {
                lines.append(message)
            }
            if let status = response.status {
                lines += statusLines(status)
            }
        }
        return lines.joined(separator: "\n")
    }

    public static func json(_ response: ControlResponse) -> String {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        guard let data = try? encoder.encode(response) else {
            return "{}"
        }
        return String(decoding: data, as: UTF8.self)
    }

    static func statusLines(_ status: ControlStatus) -> [String] {
        var lines = ["状态: \(connectionName(status.connection)) (\(status.connection))"]
        if let node = status.node {
            lines.append("节点: \(node)")
        }
        if let group = status.group {
            lines.append("订阅: \(group)")
        }
        lines.append("模式: \(modeName(status.mode)) (\(status.mode.rawValue))")
        if let since = status.connectedSince, status.isConnected {
            lines.append("连接于: \(ISO8601DateFormatter().string(from: since))")
        }
        return lines
    }

    static func latencyText(_ node: ControlNode) -> String? {
        if let millis = node.latencyMs {
            return "\(millis) ms"
        }
        switch node.latency {
        case "timeout":
            return "超时"
        case "failed":
            return "失败"
        case "unsupported":
            return "不支持测速"
        case "measuring":
            return "测速中"
        default:
            return nil
        }
    }

    static func connectionName(_ connection: String) -> String {
        switch connection {
        case "connected":
            return "已连接"
        case "connecting":
            return "正在连接"
        case "reasserting":
            return "正在重连"
        case "disconnecting":
            return "正在断开"
        case "unconfigured":
            return "未配置"
        default:
            return "未连接"
        }
    }

    static func modeName(_ mode: ProxyMode) -> String {
        switch mode {
        case .rule:
            return "规则"
        case .global:
            return "全局"
        case .direct:
            return "直连"
        }
    }
}
#endif
