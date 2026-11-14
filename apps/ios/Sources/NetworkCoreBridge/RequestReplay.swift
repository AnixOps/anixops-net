import Foundation

/// One captured HTTP request to send again, as captured or edited (plan E1, free). The app sends it with
/// URLSession, so it goes through the tunnel like any other request and shows up in the capture itself.
public struct RequestReplay: Equatable, Sendable {
    public struct Header: Equatable, Hashable, Sendable {
        public var name: String
        public var value: String

        public init(name: String, value: String) {
            self.name = name
            self.value = value
        }
    }

    /// Request bodies sent again are kept to this size; the capture keeps no more per direction either.
    public static let maxBodyBytes = 1024 * 1024

    /// Headers URLSession writes itself or that would not match an edited body: they are left out when a request is
    /// taken from the capture, and ignored when building one.
    static let managedHeaders: Set<String> = [
        "host", "content-length", "connection", "keep-alive", "transfer-encoding", "upgrade",
        "proxy-connection", "te", "trailer",
    ]

    public var method: String
    public var url: String
    public var headers: [Header]
    public var body: Data

    public init(method: String = "GET", url: String = "", headers: [Header] = [], body: Data = Data()) {
        self.method = method
        self.url = url
        self.headers = headers
        self.body = body
    }

    /// The request as captured. HTTP/2 pseudo-headers (`:authority` and the like) and the headers URLSession sets
    /// are dropped. `complete` is false when the captured body was cut off, so sending it unchanged would not
    /// repeat the request.
    public static func captured(
        method: String,
        url: String,
        headers: [(name: String, value: String)],
        body: Data,
        bodyTruncated: Bool
    ) -> (replay: RequestReplay, complete: Bool) {
        let kept = headers
            .filter { !$0.name.hasPrefix(":") && !managedHeaders.contains($0.name.lowercased()) }
            .map { Header(name: $0.name, value: $0.value) }
        let replay = RequestReplay(method: method, url: url, headers: kept, body: bodyTruncated ? Data() : body)
        return (replay, !bodyTruncated)
    }

    /// The headers as editable text, one `Name: value` per line.
    public var headerText: String {
        headers.map { "\($0.name): \($0.value)" }.joined(separator: "\n")
    }

    /// Reads headers written one `Name: value` per line; blank lines are skipped.
    public static func headers(from text: String) throws -> [Header] {
        var headers: [Header] = []
        for line in text.split(whereSeparator: \.isNewline) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            guard !trimmed.isEmpty else {
                continue
            }
            guard let colon = trimmed.firstIndex(of: ":"), colon != trimmed.startIndex else {
                throw RequestReplayError.invalidHeader(trimmed)
            }
            let name = trimmed[..<colon].trimmingCharacters(in: .whitespaces)
            guard name.allSatisfy({ $0.isASCII && !$0.isWhitespace }) else {
                throw RequestReplayError.invalidHeader(trimmed)
            }
            headers.append(Header(name: name, value: trimmed[trimmed.index(after: colon)...].trimmingCharacters(in: .whitespaces)))
        }
        return headers
    }

    /// The URLSession request, or why it cannot be sent.
    public func urlRequest(timeout: TimeInterval = 30) throws -> URLRequest {
        let method = method.trimmingCharacters(in: .whitespaces).uppercased()
        guard !method.isEmpty, method.allSatisfy({ $0.isASCII && ($0.isLetter || $0 == "-") }) else {
            throw RequestReplayError.invalidMethod
        }
        guard let url = URL(string: url.trimmingCharacters(in: .whitespacesAndNewlines)),
              let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https",
              url.host?.isEmpty == false
        else {
            throw RequestReplayError.invalidURL
        }
        guard body.count <= Self.maxBodyBytes else {
            throw RequestReplayError.bodyTooLarge
        }
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalAndRemoteCacheData, timeoutInterval: timeout)
        request.httpMethod = method
        request.httpShouldHandleCookies = false
        for header in headers where !Self.managedHeaders.contains(header.name.lowercased()) {
            request.addValue(header.value, forHTTPHeaderField: header.name)
        }
        if !body.isEmpty {
            request.httpBody = body
        }
        return request
    }
}

/// What came back from a replay.
public struct ReplayResult: Equatable, Sendable {
    public let status: Int
    public let headers: [RequestReplay.Header]
    /// Already decompressed by URLSession.
    public let body: Data
    public let duration: TimeInterval

    public init(status: Int, headers: [RequestReplay.Header], body: Data, duration: TimeInterval) {
        self.status = status
        self.headers = headers
        self.body = body
        self.duration = duration
    }

    public init(response: HTTPURLResponse, body: Data, duration: TimeInterval) {
        let headers = response.allHeaderFields
            .compactMap { key, value -> RequestReplay.Header? in
                guard let name = key as? String else {
                    return nil
                }
                return RequestReplay.Header(name: name, value: "\(value)")
            }
            .sorted { $0.name.lowercased() < $1.name.lowercased() }
        self.init(status: response.statusCode, headers: headers, body: body, duration: duration)
    }

    /// The body as text when it is UTF-8.
    public var bodyText: String? {
        String(data: body, encoding: .utf8)
    }
}

/// Sends replays: no cookies or cache, redirects not followed so the answer is the server's own.
public final class RequestReplaySender: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    private lazy var session: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpCookieStorage = nil
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalAndRemoteCacheData
        return URLSession(configuration: configuration, delegate: self, delegateQueue: nil)
    }()

    override public init() {
        super.init()
    }

    public func send(_ replay: RequestReplay) async throws -> ReplayResult {
        let request = try replay.urlRequest()
        let started = Date()
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse else {
            throw RequestReplayError.notHTTP
        }
        return ReplayResult(response: http, body: data, duration: Date().timeIntervalSince(started))
    }

    public func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest
    ) async -> URLRequest? {
        nil
    }
}

public enum RequestReplayError: Error, Equatable {
    case invalidMethod
    case invalidURL
    case invalidHeader(String)
    case bodyTooLarge
    case notHTTP

    public var message: String {
        switch self {
        case .invalidMethod:
            return String(localized: "请求方法只能包含字母，例如 GET、POST。")
        case .invalidURL:
            return String(localized: "请填写 http:// 或 https:// 开头的完整 URL。")
        case let .invalidHeader(line):
            return String(localized: "请求头“\(line)”格式不对，每行写“名称: 值”。")
        case .bodyTooLarge:
            return String(localized: "请求体不能超过 1 MB。")
        case .notHTTP:
            return String(localized: "服务器没有返回 HTTP 响应。")
        }
    }
}
