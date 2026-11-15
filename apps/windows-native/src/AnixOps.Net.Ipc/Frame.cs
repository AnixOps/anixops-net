using System.Buffers.Binary;

namespace AnixOps.Net.Ipc;

/// <summary>
/// One message on the channel: a four-byte big-endian length followed by that many bytes of UTF-8 JSON.
/// The same format as <c>crates/client-ipc/src/frame.rs</c>.
/// </summary>
public static class Frame
{
    /// <summary>The largest payload either side accepts, so a broken length cannot ask for gigabytes.</summary>
    public const int MaxLength = 64 * 1024 * 1024;

    /// <summary>Writes <paramref name="payload"/> as one frame. The caller keeps frames from interleaving.</summary>
    public static async Task WriteAsync(Stream stream, ReadOnlyMemory<byte> payload, CancellationToken cancellation = default)
    {
        if (payload.Length > MaxLength)
        {
            throw new InvalidDataException($"a frame of {payload.Length} bytes is over the limit of {MaxLength}");
        }

        // Length and payload go out in one write, so the other side never sees half a frame from this call.
        var frame = new byte[4 + payload.Length];
        BinaryPrimitives.WriteUInt32BigEndian(frame, (uint)payload.Length);
        payload.CopyTo(frame.AsMemory(4));
        await stream.WriteAsync(frame, cancellation).ConfigureAwait(false);
        await stream.FlushAsync(cancellation).ConfigureAwait(false);
    }

    /// <summary>
    /// The next frame's payload, or <c>null</c> when the other side closed the channel between two frames.
    /// A channel that ends inside a frame, or a length over the limit, is an error.
    /// </summary>
    public static async Task<byte[]?> ReadAsync(Stream stream, CancellationToken cancellation = default)
    {
        var header = new byte[4];
        var read = await stream.ReadAtLeastAsync(header, 4, throwOnEndOfStream: false, cancellation).ConfigureAwait(false);
        if (read == 0)
        {
            return null;
        }

        if (read < 4)
        {
            throw new EndOfStreamException("the channel closed inside a frame's length");
        }

        var length = BinaryPrimitives.ReadUInt32BigEndian(header);
        if (length > MaxLength)
        {
            throw new InvalidDataException($"a frame of {length} bytes is over the limit of {MaxLength}");
        }

        var payload = new byte[length];
        await stream.ReadExactlyAsync(payload, cancellation).ConfigureAwait(false);
        return payload;
    }
}
