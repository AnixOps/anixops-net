import Foundation

/// Large tunnel profile values (plugin texts, scripts) are stored zlib-compressed: plugins and scripts of several
/// MB shrink to a fraction, which keeps the VPN preferences small. Values saved before compression existed are
/// plain JSON and still read.
public enum ProfileCompression {
    private static let magic = Data("NCZ1".utf8)

    public static func pack(_ data: Data) throws -> Data {
        magic + (try (data as NSData).compressed(using: .zlib) as Data)
    }

    /// The original bytes of a packed value, or `value` itself when it was never packed.
    public static func unpack(_ value: Data) -> Data? {
        guard value.starts(with: magic) else {
            return value
        }
        return try? (value.dropFirst(magic.count) as NSData).decompressed(using: .zlib) as Data
    }
}
