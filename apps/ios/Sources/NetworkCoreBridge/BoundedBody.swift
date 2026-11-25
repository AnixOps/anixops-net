import Foundation

/// Collects a download body without ever holding more than a limit (audit 2026-11-25 NET-02, the Apple side of
/// the client-core bounded read). An answer whose declared length is over the limit is refused before a byte is
/// read; one without a length, or one that sends more than it declared, is refused as soon as it passes the limit.
public enum BoundedBody {
    public struct TooLarge: Error, Equatable {
        public let limit: Int
    }

    /// The bytes of `bytes`, at most `limit`. `expectedLength` is `URLResponse.expectedContentLength` (negative when
    /// unknown).
    public static func collect<Bytes: AsyncSequence>(
        _ bytes: Bytes,
        expectedLength: Int64,
        limit: Int
    ) async throws -> Data where Bytes.Element == UInt8 {
        if expectedLength > Int64(limit) {
            throw TooLarge(limit: limit)
        }
        var data = Data()
        data.reserveCapacity(expectedLength > 0 ? Int(expectedLength) : 0)
        for try await byte in bytes {
            if data.count >= limit {
                throw TooLarge(limit: limit)
            }
            data.append(byte)
        }
        return data
    }
}
