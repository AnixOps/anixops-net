#if os(macOS)
import Darwin
import Foundation

// The local control channel between the `anixops` command-line tool and the running macOS app
// (docs/macos-cli.md).
//
// The app listens on a Unix domain socket in its app group container, mode 0600, and answers only peers of the
// same user (LOCAL_PEERCRED). One connection carries one request and one answer, each a single JSON line. The app
// does the work with its own tunnel controller, so the tool needs no Network Extension entitlement and the
// Keychain, the VPN profile and the system extension stay the app's.

public enum ControlProtocol {
    public static let version = 1
    /// File name of the socket in the app group container.
    public static let socketName = "cli.sock"
    /// Requests and answers are small JSON documents; a node list for import is the largest.
    public static let maxMessageBytes = 4 << 20

    /// `~/Library/Group Containers/<group>/cli.sock` of the user running the tool.
    public static func socketPath(appGroup: String, home: String) -> String {
        (home as NSString)
            .appendingPathComponent("Library/Group Containers")
            .appending("/\(appGroup)/\(socketName)")
    }
}

public struct ControlRequest: Codable, Equatable, Sendable {
    public enum Command: String, Codable, Sendable, CaseIterable {
        case status
        case connect
        case disconnect
        case mode
        case nodes
        case select
        case test
        case importNodes = "import"
    }

    public var version: Int
    public var command: Command
    /// `mode`: the mode to switch to; nil only reads it.
    public var mode: ProxyMode?
    /// `nodes`, `select`, `test`: the subscription group, by id or name.
    public var group: String?
    /// `select`: the node, by id or name.
    public var node: String?
    /// `import`: a subscription link, a node link or node list text.
    public var text: String?

    public init(
        command: Command,
        mode: ProxyMode? = nil,
        group: String? = nil,
        node: String? = nil,
        text: String? = nil,
        version: Int = ControlProtocol.version
    ) {
        self.version = version
        self.command = command
        self.mode = mode
        self.group = group
        self.node = node
        self.text = text
    }
}

/// Why a request failed; each maps to one exit code of the tool (`CommandLineExit`).
public enum ControlErrorCode: String, Codable, Equatable, Sendable {
    /// The request is malformed or not valid in the current state, for example `mode rule` with a single node.
    case usage
    /// No such group or node.
    case notFound
    /// The import text or link holds no usable node.
    case invalidData
    /// The app tried and failed, for example the tunnel did not come up.
    case failed
    /// The app did not finish in time.
    case timeout
    /// The app cannot do it now, for example nothing is imported yet.
    case unavailable
    /// The tool and the app speak different protocol versions.
    case unsupportedVersion
}

public struct ControlStatus: Codable, Equatable, Sendable {
    /// connected, connecting, reasserting, disconnecting, disconnected or unconfigured.
    public var connection: String
    public var node: String?
    public var group: String?
    public var mode: ProxyMode
    public var connectedSince: Date?
    public var appVersion: String

    public init(
        connection: String,
        node: String?,
        group: String?,
        mode: ProxyMode,
        connectedSince: Date?,
        appVersion: String
    ) {
        self.connection = connection
        self.node = node
        self.group = group
        self.mode = mode
        self.connectedSince = connectedSince
        self.appVersion = appVersion
    }

    public var isConnected: Bool {
        connection == "connected"
    }
}

public struct ControlNode: Codable, Equatable, Sendable {
    public var id: String
    public var name: String
    public var `protocol`: String
    public var selected: Bool
    /// The last delay test: milliseconds, or nil with `latency` saying why.
    public var latencyMs: Int?
    /// measuring, timeout, failed or unsupported; nil when untested or measured.
    public var latency: String?

    public init(
        id: String,
        name: String,
        protocol kind: String,
        selected: Bool,
        latencyMs: Int? = nil,
        latency: String? = nil
    ) {
        self.id = id
        self.name = name
        self.protocol = kind
        self.selected = selected
        self.latencyMs = latencyMs
        self.latency = latency
    }
}

public struct ControlGroup: Codable, Equatable, Sendable {
    public var id: String
    public var name: String
    public var active: Bool
    public var nodes: [ControlNode]

    public init(id: String, name: String, active: Bool, nodes: [ControlNode]) {
        self.id = id
        self.name = name
        self.active = active
        self.nodes = nodes
    }
}

public struct ControlResponse: Codable, Equatable, Sendable {
    public var ok: Bool
    public var error: ControlErrorCode?
    /// A sentence for people, in the app's language.
    public var message: String?
    public var status: ControlStatus?
    public var groups: [ControlGroup]?

    public init(
        ok: Bool = true,
        error: ControlErrorCode? = nil,
        message: String? = nil,
        status: ControlStatus? = nil,
        groups: [ControlGroup]? = nil
    ) {
        self.ok = ok
        self.error = error
        self.message = message
        self.status = status
        self.groups = groups
    }

    public static func failure(_ error: ControlErrorCode, _ message: String) -> ControlResponse {
        ControlResponse(ok: false, error: error, message: message)
    }
}

public enum ControlTransportError: Error, Equatable, Sendable {
    /// The socket path does not fit in `sockaddr_un` (very long home folder).
    case pathTooLong
    /// No app is listening.
    case notRunning
    case timeout
    /// The other side sent something that is not an answer, or closed early.
    case protocolError
    case system(Int32)
}

enum ControlWire {
    static func encode<T: Encodable>(_ value: T) throws -> Data {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        // JSON never holds a raw newline outside strings, and strings escape theirs, so one line is one message.
        return try encoder.encode(value) + Data([0x0A])
    }

    static func decode<T: Decodable>(_ type: T.Type, from data: Data) throws -> T {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode(type, from: data)
    }

    static func address(_ path: String) throws -> sockaddr_un {
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        let capacity = MemoryLayout.size(ofValue: address.sun_path)
        guard !bytes.isEmpty, bytes.count < capacity else {
            throw ControlTransportError.pathTooLong
        }
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            raw.copyBytes(from: bytes)
            raw[bytes.count] = 0
        }
        address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        return address
    }

    static func setTimeout(_ fd: Int32, seconds: TimeInterval) {
        let whole = Int(seconds.rounded(.down))
        var value = timeval(tv_sec: whole, tv_usec: Int32((seconds - Double(whole)) * 1_000_000))
        let size = socklen_t(MemoryLayout<timeval>.size)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &value, size)
        setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &value, size)
    }

    static func noSigPipe(_ fd: Int32) {
        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
    }

    static func writeAll(_ fd: Int32, _ data: Data) throws {
        try data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            var offset = 0
            while offset < raw.count {
                let written = Darwin.write(fd, raw.baseAddress! + offset, raw.count - offset)
                if written < 0 {
                    if errno == EINTR {
                        continue
                    }
                    throw errno == EAGAIN || errno == EWOULDBLOCK
                        ? ControlTransportError.timeout
                        : ControlTransportError.system(errno)
                }
                offset += written
            }
        }
    }

    /// Reads up to and including the first newline. Nil when the peer closed before sending a whole line.
    static func readLine(_ fd: Int32) throws -> Data? {
        var line = Data()
        var buffer = [UInt8](repeating: 0, count: 64 * 1024)
        while true {
            let count = buffer.withUnsafeMutableBytes { Darwin.read(fd, $0.baseAddress, $0.count) }
            if count < 0 {
                if errno == EINTR {
                    continue
                }
                throw errno == EAGAIN || errno == EWOULDBLOCK
                    ? ControlTransportError.timeout
                    : ControlTransportError.system(errno)
            }
            if count == 0 {
                return nil
            }
            if let newline = buffer[0..<count].firstIndex(of: 0x0A) {
                line.append(contentsOf: buffer[0..<newline])
                return line
            }
            line.append(contentsOf: buffer[0..<count])
            guard line.count <= ControlProtocol.maxMessageBytes else {
                throw ControlTransportError.protocolError
            }
        }
    }
}

/// The tool's side: one request, one answer.
public enum ControlClient {
    public static func send(_ request: ControlRequest, to path: String, timeout: TimeInterval) throws -> ControlResponse {
        var address = try ControlWire.address(path)
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else {
            throw ControlTransportError.system(errno)
        }
        defer { close(fd) }
        ControlWire.noSigPipe(fd)
        ControlWire.setTimeout(fd, seconds: timeout)
        let connected = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard connected == 0 else {
            switch errno {
            case ENOENT, ECONNREFUSED, ENOTDIR:
                throw ControlTransportError.notRunning
            default:
                throw ControlTransportError.system(errno)
            }
        }
        try ControlWire.writeAll(fd, ControlWire.encode(request))
        guard let line = try ControlWire.readLine(fd),
              let response = try? ControlWire.decode(ControlResponse.self, from: line)
        else {
            throw ControlTransportError.protocolError
        }
        return response
    }
}

/// The app's side: accepts connections on the socket and answers each request with `handler`.
public final class ControlServer: @unchecked Sendable {
    public typealias Handler = @Sendable (ControlRequest) async -> ControlResponse

    public let path: String
    private let handler: Handler
    private let queue = DispatchQueue(label: "net.anixops.control-server")
    private var source: DispatchSourceRead?
    /// How long a peer has to send its request.
    private let requestTimeout: TimeInterval

    public init(path: String, requestTimeout: TimeInterval = 5, handler: @escaping Handler) {
        self.path = path
        self.requestTimeout = requestTimeout
        self.handler = handler
    }

    deinit {
        source?.cancel()
    }

    /// Binds the socket (replacing a stale one from an earlier run), makes it 0600 and starts accepting.
    public func start() throws {
        guard source == nil else {
            return
        }
        var address = try ControlWire.address(path)
        let directory = (path as NSString).deletingLastPathComponent
        try FileManager.default.createDirectory(atPath: directory, withIntermediateDirectories: true)
        unlink(path)
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else {
            throw ControlTransportError.system(errno)
        }
        // Owner only from the moment it exists; LOCAL_PEERCRED below checks every peer again.
        let previous = umask(0o077)
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        umask(previous)
        guard bound == 0, chmod(path, 0o600) == 0, listen(fd, 16) == 0 else {
            let code = errno
            close(fd)
            throw ControlTransportError.system(code)
        }
        _ = fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) | O_NONBLOCK)
        let source = DispatchSource.makeReadSource(fileDescriptor: fd, queue: queue)
        source.setEventHandler { [weak self] in
            self?.acceptPending(fd)
        }
        let path = path
        source.setCancelHandler {
            close(fd)
            unlink(path)
        }
        self.source = source
        source.resume()
    }

    public func stop() {
        source?.cancel()
        source = nil
    }

    private func acceptPending(_ listener: Int32) {
        while true {
            let fd = accept(listener, nil, nil)
            guard fd >= 0 else {
                return
            }
            // Accepted sockets inherit O_NONBLOCK on Darwin; the per-connection work blocks with a timeout.
            _ = fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) & ~O_NONBLOCK)
            ControlWire.noSigPipe(fd)
            ControlWire.setTimeout(fd, seconds: requestTimeout)
            serve(fd)
        }
    }

    private func serve(_ fd: Int32) {
        guard Self.peerIsCurrentUser(fd) else {
            close(fd)
            return
        }
        let request: ControlRequest
        do {
            guard let line = try ControlWire.readLine(fd) else {
                close(fd)
                return
            }
            if let decoded = try? ControlWire.decode(ControlRequest.self, from: line) {
                request = decoded
            } else {
                let version = (try? ControlWire.decode(VersionOnly.self, from: line))?.version
                let answer: ControlResponse = version.map { $0 != ControlProtocol.version } == true
                    ? .failure(.unsupportedVersion, "protocol version \(version ?? 0) is not supported")
                    : .failure(.usage, "malformed request")
                try? ControlWire.writeAll(fd, ControlWire.encode(answer))
                close(fd)
                return
            }
        } catch {
            close(fd)
            return
        }
        guard request.version == ControlProtocol.version else {
            let answer = ControlResponse.failure(
                .unsupportedVersion,
                "protocol version \(request.version) is not supported"
            )
            try? ControlWire.writeAll(fd, ControlWire.encode(answer))
            close(fd)
            return
        }
        let handler = handler
        Task.detached {
            let answer = await handler(request)
            // The answer may come long after the request (a delay test); no send timeout for a slow reader.
            ControlWire.setTimeout(fd, seconds: 30)
            try? ControlWire.writeAll(fd, ControlWire.encode(answer))
            close(fd)
        }
    }

    /// Only processes of the user running the app may drive it.
    static func peerIsCurrentUser(_ fd: Int32) -> Bool {
        var credentials = xucred()
        var length = socklen_t(MemoryLayout<xucred>.size)
        guard getsockopt(fd, SOL_LOCAL, LOCAL_PEERCRED, &credentials, &length) == 0,
              credentials.cr_version == UInt32(XUCRED_VERSION)
        else {
            return false
        }
        return credentials.cr_uid == geteuid()
    }

    private struct VersionOnly: Decodable {
        let version: Int
    }
}
#endif
