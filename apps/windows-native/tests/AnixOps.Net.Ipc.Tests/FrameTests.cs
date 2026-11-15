using System.Text;
using Xunit;

namespace AnixOps.Net.Ipc.Tests;

public class FrameTests
{
    [Fact]
    public async Task AFrameIsItsLengthBigEndianThenThePayload()
    {
        using var stream = new MemoryStream();
        await Frame.WriteAsync(stream, Encoding.UTF8.GetBytes("{}"));
        Assert.Equal(new byte[] { 0, 0, 0, 2, (byte)'{', (byte)'}' }, stream.ToArray());
    }

    [Fact]
    public async Task FramesComeBackInOrderAndTheEndIsClean()
    {
        using var stream = new MemoryStream();
        await Frame.WriteAsync(stream, Encoding.UTF8.GetBytes("first"));
        await Frame.WriteAsync(stream, ReadOnlyMemory<byte>.Empty);
        await Frame.WriteAsync(stream, new byte[300_000]);
        stream.Position = 0;
        Assert.Equal("first", Encoding.UTF8.GetString((await Frame.ReadAsync(stream))!));
        Assert.Empty((await Frame.ReadAsync(stream))!);
        Assert.Equal(300_000, (await Frame.ReadAsync(stream))!.Length);
        Assert.Null(await Frame.ReadAsync(stream));
    }

    [Fact]
    public async Task AChannelThatEndsInsideAFrameIsAnError()
    {
        using var insideLength = new MemoryStream([0, 0]);
        await Assert.ThrowsAsync<EndOfStreamException>(() => Frame.ReadAsync(insideLength));
        using var insidePayload = new MemoryStream([0, 0, 0, 5, 1, 2]);
        await Assert.ThrowsAsync<EndOfStreamException>(() => Frame.ReadAsync(insidePayload));
    }

    [Fact]
    public async Task ALengthOverTheLimitIsRefusedBeforeAnythingIsRead()
    {
        using var stream = new MemoryStream([0xFF, 0xFF, 0xFF, 0xFF]);
        await Assert.ThrowsAsync<InvalidDataException>(() => Frame.ReadAsync(stream));
    }
}
