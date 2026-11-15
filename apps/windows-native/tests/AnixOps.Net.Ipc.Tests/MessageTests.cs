using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Xunit;

namespace AnixOps.Net.Ipc.Tests;

/// <summary>The samples the Rust side reads too (<c>crates/client-ipc/fixtures</c>).</summary>
public class MessageTests
{
    private static byte[] Sample(string name) =>
        File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "fixtures", "ipc", name + ".json"));

    private static void AssertSameJson(string expected, byte[] actual) =>
        Assert.True(
            JsonNode.DeepEquals(JsonNode.Parse(expected), JsonNode.Parse(actual)),
            $"expected {expected}, got {Encoding.UTF8.GetString(actual)}");

    [Fact]
    public void SamplesMeanWhatTheySay()
    {
        var hello = Assert.IsType<Hello>(Message.Parse(Sample("hello")));
        Assert.Equal((1u, "window", "0.3.0"), (hello.Protocol, hello.Role, hello.Version));

        var request = Assert.IsType<Request>(Message.Parse(Sample("request")));
        Assert.Equal((7ul, "select_node"), (request.Id, request.Command));
        Assert.Equal("node-hk", request.Args.GetProperty("nodeId").GetString());

        var bare = Assert.IsType<Request>(Message.Parse(Sample("request-without-args")));
        Assert.Equal((8ul, "connect"), (bare.Id, bare.Command));
        Assert.Equal(JsonValueKind.Object, bare.Args.ValueKind);
        Assert.Empty(bare.Args.EnumerateObject());

        var ok = Assert.IsType<Response>(Message.Parse(Sample("response-ok")));
        Assert.Equal(7ul, ok.Id);
        Assert.Null(ok.Error);
        Assert.Equal("Node selected", ok.Ok.GetProperty("message").GetString());

        var empty = Assert.IsType<Response>(Message.Parse(Sample("response-empty")));
        Assert.Equal(8ul, empty.Id);
        Assert.Null(empty.Error);
        Assert.Equal(JsonValueKind.Null, empty.Ok.ValueKind);

        var refused = Assert.IsType<Response>(Message.Parse(Sample("response-error")));
        Assert.Equal((9ul, "the node is not in any subscription"), (refused.Id, refused.Error));

        var told = Assert.IsType<Event>(Message.Parse(Sample("event")));
        Assert.Equal("snapshot_changed", told.Name);
        Assert.Equal("connected", told.Payload.GetProperty("connection").GetString());
    }

    [Theory]
    [InlineData("hello")]
    [InlineData("request")]
    [InlineData("response-ok")]
    [InlineData("response-empty")]
    [InlineData("response-error")]
    [InlineData("event")]
    public void WhatIsWrittenIsWhatTheSampleSays(string name)
    {
        var sample = Sample(name);
        AssertSameJson(Encoding.UTF8.GetString(sample), Message.Parse(sample).ToUtf8());
    }

    [Fact]
    public void ARequestIsAlwaysWrittenWithItsArguments()
    {
        AssertSameJson(
            """{ "type": "request", "id": 8, "command": "connect", "args": {} }""",
            Message.Parse(Sample("request-without-args")).ToUtf8());
    }

    [Fact]
    public void AnEventWithoutAPayloadIsWrittenWithoutOne()
    {
        AssertSameJson("""{ "type": "event", "name": "tick" }""", new Event("tick", Message.Null).ToUtf8());
    }

    [Theory]
    [InlineData("not json")]
    [InlineData("[]")]
    [InlineData("""{ "type": "unknown" }""")]
    [InlineData("""{ "type": 7 }""")]
    [InlineData("""{ "type": "request", "command": "connect" }""")]
    [InlineData("""{ "type": "request", "id": "seven", "command": "connect" }""")]
    [InlineData("""{ "type": "hello", "protocol": 1 }""")]
    public void WhatIsNotAMessageIsAnError(string text)
    {
        Assert.Throws<InvalidDataException>(() => Message.Parse(Encoding.UTF8.GetBytes(text)));
    }
}
