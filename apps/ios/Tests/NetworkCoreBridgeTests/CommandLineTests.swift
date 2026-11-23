#if os(macOS)
import Darwin
import Foundation
import NetworkCoreBridge
import XCTest

final class CommandLineTests: XCTestCase {
    // MARK: Parsing

    func testCommandsBecomeRequests() throws {
        XCTAssertEqual(try parse(["status"]).action, .request(ControlRequest(command: .status)))
        XCTAssertEqual(try parse(["connect"]).action, .request(ControlRequest(command: .connect)))
        XCTAssertEqual(try parse(["down"]).action, .request(ControlRequest(command: .disconnect)))
        XCTAssertEqual(try parse(["mode"]).action, .request(ControlRequest(command: .mode)))
        XCTAssertEqual(try parse(["mode", "Rule"]).action, .request(ControlRequest(command: .mode, mode: .rule)))
        XCTAssertEqual(
            try parse(["nodes", "--group", "服务商"]).action,
            .request(ControlRequest(command: .nodes, group: "服务商"))
        )
        XCTAssertEqual(
            try parse(["select", "香港 01", "-g", "服务商"]).action,
            .request(ControlRequest(command: .select, group: "服务商", node: "香港 01"))
        )
        XCTAssertEqual(try parse(["test"]).action, .request(ControlRequest(command: .test)))
        XCTAssertEqual(try parse(["version"]).action, .version)
        XCTAssertEqual(try parse(["--help"]).action, .help)
        XCTAssertEqual(try parse(["status", "-h"]).action, .help)
    }

    func testImportSources() throws {
        XCTAssertEqual(try parse(["import", "-"]).action, .importNodes(.standardInput))
        XCTAssertEqual(
            try parse(["import", "https://example.com/sub?token=x"]).action,
            .importNodes(.text("https://example.com/sub?token=x"))
        )
        XCTAssertEqual(try parse(["import", "ss://YWVz@h:1#n"]).action, .importNodes(.text("ss://YWVz@h:1#n")))
        XCTAssertEqual(try parse(["import", "nodes.txt"]).action, .importNodes(.file("nodes.txt")))
    }

    func testOptions() throws {
        let invocation = try parse(["--json", "status", "-q", "--timeout", "2.5", "--socket", "/tmp/x.sock"])
        XCTAssertTrue(invocation.json)
        XCTAssertTrue(invocation.quiet)
        XCTAssertEqual(invocation.timeout, 2.5)
        XCTAssertEqual(invocation.socket, "/tmp/x.sock")
        XCTAssertEqual(invocation.effectiveTimeout, 2.5)
        // A node whose name starts with a dash.
        XCTAssertEqual(
            try parse(["select", "--", "-node-"]).action,
            .request(ControlRequest(command: .select, node: "-node-"))
        )
    }

    func testDefaultTimeoutsFollowTheCommand() throws {
        XCTAssertEqual(try parse(["status"]).effectiveTimeout, 10)
        XCTAssertEqual(try parse(["test"]).effectiveTimeout, 180)
        XCTAssertEqual(try parse(["connect"]).effectiveTimeout, 45)
        XCTAssertEqual(try parse(["import", "-"]).effectiveTimeout, 90)
    }

    func testUsageErrors() {
        for arguments in [
            [],
            ["frobnicate"],
            ["status", "extra"],
            ["status", "--group", "g"],
            ["mode", "fast"],
            ["mode", "rule", "global"],
            ["select"],
            ["import"],
            ["import", "a", "b"],
            ["status", "--timeout", "0"],
            ["status", "--timeout"],
            ["status", "--verbose"],
            ["version", "now"],
        ] {
            if case .success = CommandLineParser.parse(arguments) {
                XCTFail("expected a usage error for \(arguments)")
            }
        }
    }

    // MARK: Exit codes

    func testExitCodes() {
        XCTAssertEqual(CommandLineExit(ControlErrorCode.usage).rawValue, 64)
        XCTAssertEqual(CommandLineExit(ControlErrorCode.notFound).rawValue, 65)
        XCTAssertEqual(CommandLineExit(ControlErrorCode.invalidData).rawValue, 65)
        XCTAssertEqual(CommandLineExit(ControlErrorCode.failed).rawValue, 1)
        XCTAssertEqual(CommandLineExit(ControlErrorCode.timeout).rawValue, 75)
        XCTAssertEqual(CommandLineExit(ControlErrorCode.unavailable).rawValue, 69)
        XCTAssertEqual(CommandLineExit(ControlErrorCode.unsupportedVersion).rawValue, 76)
        XCTAssertEqual(CommandLineExit(ControlTransportError.notRunning).rawValue, 69)
        XCTAssertEqual(CommandLineExit(ControlTransportError.timeout).rawValue, 75)
        XCTAssertEqual(CommandLineExit(ControlTransportError.protocolError).rawValue, 76)
        XCTAssertEqual(CommandLineExit(ControlTransportError.system(EIO)).rawValue, 71)
        XCTAssertEqual(CommandLineExit.notConnected.rawValue, 3)
        XCTAssertEqual(CommandLineExit.noInput.rawValue, 66)
    }

    // MARK: Output

    func testTextOutput() {
        let status = ControlStatus(
            connection: "connected",
            node: "香港 01",
            group: "服务商",
            mode: .rule,
            connectedSince: nil,
            appVersion: "0.3.4"
        )
        let text = CommandLineOutput.render(ControlResponse(status: status), for: .status)
        XCTAssertTrue(text.contains("已连接 (connected)"))
        XCTAssertTrue(text.contains("节点: 香港 01"))
        XCTAssertTrue(text.contains("模式: 规则 (rule)"))

        let groups = [ControlGroup(id: "g1", name: "服务商", active: true, nodes: [
            ControlNode(id: "a", name: "香港 01", protocol: "trojan", selected: true, latencyMs: 48),
            ControlNode(id: "b", name: "日本 02", protocol: "vless", selected: false, latency: "timeout"),
        ])]
        let list = CommandLineOutput.render(ControlResponse(groups: groups), for: .test)
        XCTAssertEqual(list, "[服务商] 当前使用\n  * 香港 01  trojan  48 ms\n    日本 02  vless  超时")

        XCTAssertEqual(
            CommandLineOutput.render(.failure(.notFound, "没有叫 x 的节点"), for: .select),
            "错误: 没有叫 x 的节点"
        )
    }

    // MARK: Socket

    func testRequestAndAnswerOverTheSocket() throws {
        let path = socketPath()
        let server = ControlServer(path: path) { request in
            ControlResponse(message: "got \(request.command.rawValue) \(request.node ?? "")")
        }
        try server.start()
        defer { server.stop() }

        var info = stat()
        XCTAssertEqual(stat(path, &info), 0)
        XCTAssertEqual(info.st_mode & 0o777, 0o600, "the socket must be owner-only")

        let answer = try ControlClient.send(ControlRequest(command: .select, node: "香港 01"), to: path, timeout: 5)
        XCTAssertEqual(answer.message, "got select 香港 01")

        // A large import text arrives whole.
        let big = String(repeating: "ss://YWVzLTEyOC1nY206eA@example.com:8388#n\n", count: 20_000)
        let echo = ControlServer(path: socketPath()) { request in
            ControlResponse(message: "\(request.text?.utf8.count ?? 0)")
        }
        try echo.start()
        defer { echo.stop() }
        let sized = try ControlClient.send(ControlRequest(command: .importNodes, text: big), to: echo.path, timeout: 5)
        XCTAssertEqual(sized.message, "\(big.utf8.count)")
    }

    func testNoAppMeansNotRunning() {
        XCTAssertThrowsError(try ControlClient.send(ControlRequest(command: .status), to: socketPath(), timeout: 1)) { error in
            XCTAssertEqual(error as? ControlTransportError, .notRunning)
        }
    }

    func testSlowAppTimesOut() throws {
        let server = ControlServer(path: socketPath()) { _ in
            try? await Task.sleep(nanoseconds: 3_000_000_000)
            return ControlResponse()
        }
        try server.start()
        defer { server.stop() }

        XCTAssertThrowsError(try ControlClient.send(ControlRequest(command: .test), to: server.path, timeout: 0.5)) { error in
            XCTAssertEqual(error as? ControlTransportError, .timeout)
        }
    }

    func testOtherProtocolVersionIsRefused() throws {
        let server = ControlServer(path: socketPath()) { _ in ControlResponse() }
        try server.start()
        defer { server.stop() }

        let answer = try ControlClient.send(ControlRequest(command: .status, version: 99), to: server.path, timeout: 5)
        XCTAssertFalse(answer.ok)
        XCTAssertEqual(answer.error, .unsupportedVersion)
    }

    func testStoppedServerRemovesItsSocket() throws {
        let server = ControlServer(path: socketPath()) { _ in ControlResponse() }
        try server.start()
        server.stop()
        // The cancel handler runs on the server's queue.
        let deadline = Date().addingTimeInterval(2)
        while FileManager.default.fileExists(atPath: server.path), Date() < deadline {
            Thread.sleep(forTimeInterval: 0.05)
        }
        XCTAssertFalse(FileManager.default.fileExists(atPath: server.path))
    }

    func testStoppedServerRemovesItsSocketAtOnce() throws {
        // Before `stop()` returns: the app calls it when it quits, and a cancel handler may never run then.
        let server = ControlServer(path: socketPath()) { _ in ControlResponse() }
        try server.start()
        XCTAssertTrue(ControlClient.isListening(at: server.path))
        server.stop()
        XCTAssertFalse(FileManager.default.fileExists(atPath: server.path))
    }

    /// A socket file left by an app that quit or crashed is not an app that listens: the tool keeps waiting for the
    /// app it opened instead of sending at once and reporting that nothing runs.
    func testLeftoverSocketFileIsNotARunningApp() {
        let path = socketPath()
        XCTAssertTrue(FileManager.default.createFile(atPath: path, contents: Data()))
        XCTAssertFalse(ControlClient.isListening(at: path))
        XCTAssertFalse(ControlClient.waitUntilListening(at: path, timeout: 0.3, interval: 0.05))
        XCTAssertFalse(ControlClient.isListening(at: socketPath()))
    }

    func testWaitsForAnAppThatStartsListeningLater() throws {
        let path = socketPath()
        XCTAssertTrue(FileManager.default.createFile(atPath: path, contents: Data()))
        let server = ControlServer(path: path) { _ in ControlResponse() }
        defer { server.stop() }
        DispatchQueue.global().asyncAfter(deadline: .now() + 0.5) {
            try? server.start()
        }
        XCTAssertTrue(ControlClient.waitUntilListening(at: path, timeout: 10, interval: 0.05))
        // The probe sent nothing: the app still answers a real request afterwards.
        XCTAssertTrue(try ControlClient.send(ControlRequest(command: .status), to: path, timeout: 5).ok)
    }

    /// The tool finds its app from its own path, also through the symlink in PATH that docs/macos-cli.md installs.
    func testEnclosingAppThroughASymlink() throws {
        let root = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("anx-\(UUID().uuidString.prefix(8))")
        addTeardownBlock {
            try? FileManager.default.removeItem(at: root)
        }
        let app = root.appendingPathComponent("AnixOps Net.app")
        let macOS = app.appendingPathComponent("Contents/MacOS")
        try FileManager.default.createDirectory(at: macOS, withIntermediateDirectories: true)
        let tool = macOS.appendingPathComponent("anixops")
        XCTAssertTrue(FileManager.default.createFile(atPath: tool.path, contents: Data()))
        let bin = root.appendingPathComponent("bin")
        try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
        let link = bin.appendingPathComponent("anixops")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: tool)

        let expected = app.resolvingSymlinksInPath().path
        XCTAssertEqual(CommandLineTool.enclosingApp(ofExecutable: tool)?.path, expected)
        XCTAssertEqual(CommandLineTool.enclosingApp(ofExecutable: link)?.path, expected)
        // A copy outside an app has none.
        XCTAssertNil(CommandLineTool.enclosingApp(ofExecutable: bin.appendingPathComponent("other")))
    }

    func testSocketPathLivesInTheGroupContainer() {
        XCTAssertEqual(
            ControlProtocol.socketPath(appGroup: "ABCDE12345.net.anixops.networkcore", home: "/Users/me"),
            "/Users/me/Library/Group Containers/ABCDE12345.net.anixops.networkcore/cli.sock"
        )
    }

    // MARK: Helpers

    private func parse(_ arguments: [String]) throws -> CommandLineInvocation {
        try CommandLineParser.parse(arguments).get()
    }

    /// Short paths: sockaddr_un holds 104 bytes, and the test temporary folder is long.
    private func socketPath() -> String {
        let path = "/tmp/anx-\(UUID().uuidString.prefix(8)).sock"
        addTeardownBlock {
            unlink(path)
        }
        return path
    }
}
#endif
