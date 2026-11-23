using System.Collections.Concurrent;
using System.IO.Pipes;
using System.Security.Principal;
using System.Text.Json;
using Xunit;

namespace AnixOps.Net.Ipc.Tests;

/// <summary>The window's channel against a core played by the test, over a real named pipe.</summary>
public class ChannelTests
{
    private static readonly TimeSpan Soon = TimeSpan.FromSeconds(10);

    private static JsonElement Json(string text) => JsonDocument.Parse(text).RootElement.Clone();

    /// <summary>The two ends of a pipe nobody else uses.</summary>
    private static async Task<(Stream Core, Stream Window)> PipeAsync()
    {
        var name = "anixops-net-ipc-test-" + Guid.NewGuid().ToString("N");
        var core = new NamedPipeServerStream(name, PipeDirection.InOut, 1, PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
        var window = new NamedPipeClientStream(".", name, PipeDirection.InOut, PipeOptions.Asynchronous);
        await Task.WhenAll(core.WaitForConnectionAsync(), window.ConnectAsync(Soon, CancellationToken.None));
        return (core, window);
    }

    private static Task SendAsync(Stream core, Message message) => Frame.WriteAsync(core, message.ToUtf8());

    private static async Task<Message> ReceiveAsync(Stream core)
    {
        var payload = await Frame.ReadAsync(core).WaitAsync(Soon);
        Assert.NotNull(payload);
        return Message.Parse(payload);
    }

    /// <summary>A window on one end, greeted by the test as the core on the other.</summary>
    private static async Task<(Stream Core, Channel Window, Recorder Handler)> OpenAsync()
    {
        var (core, window) = await PipeAsync();
        var handler = new Recorder();
        var opening = Channel.OpenAsync(window, "0.3.0", handler);
        var hello = Assert.IsType<Hello>(await ReceiveAsync(core));
        Assert.Equal((Channel.Protocol, Hello.Window, "0.3.0"), (hello.Protocol, hello.Role, hello.Version));
        await SendAsync(core, new Hello(Channel.Protocol, Hello.Core, "0.3.1"));
        return (core, await opening.WaitAsync(Soon), handler);
    }

    private sealed class Recorder : IWindowHandler
    {
        public BlockingCollection<(string Name, JsonElement Payload)> Events { get; } = [];

        public Task<JsonElement> RequestAsync(string command, JsonElement args) => command switch
        {
            "session.describe" => Task.FromResult(Json("""{ "desktop": "Windows" }""")),
            "session.set_start_after_login" => Task.FromResult(Message.Null),
            "session.apply_proxy" => throw new RefusedException("the proxy settings are managed by this computer's administrator"),
            _ => throw new InvalidOperationException("an error of the window's own"),
        };

        public void Event(string name, JsonElement payload) => Events.Add((name, payload));
    }

    [Fact]
    public async Task TheGreetingTellsTheCoresVersion()
    {
        var (core, window, _) = await OpenAsync();
        await using (window)
        await using (core)
        {
            Assert.Equal("0.3.1", window.CoreVersion);
        }
    }

    [Theory]
    [InlineData(2u, "core")]
    [InlineData(1u, "window")]
    public async Task AnotherProtocolOrProgramIsRefused(uint protocol, string role)
    {
        var (core, window) = await PipeAsync();
        await using (core)
        await using (window)
        {
            var opening = Channel.OpenAsync(window, "0.3.0", new Recorder());
            await ReceiveAsync(core);
            await SendAsync(core, new Hello(protocol, role, "0.3.0"));
            await Assert.ThrowsAsync<InvalidDataException>(() => opening.WaitAsync(Soon));
        }
    }

    [Fact]
    public async Task ASideThatSkipsTheGreetingIsRefused()
    {
        var (core, window) = await PipeAsync();
        await using (core)
        await using (window)
        {
            var opening = Channel.OpenAsync(window, "0.3.0", new Recorder());
            await ReceiveAsync(core);
            await SendAsync(core, new Event("snapshot_changed", Message.Null));
            await Assert.ThrowsAsync<InvalidDataException>(() => opening.WaitAsync(Soon));
        }
    }

    [Fact]
    public async Task ARequestGetsItsOwnAnswer()
    {
        var (core, window, _) = await OpenAsync();
        await using (window)
        await using (core)
        {
            var first = window.AskAsync("select_node", Json("""{ "nodeId": "node-hk" }"""), Soon);
            var second = window.AskAsync("connect", patience: Soon);
            var one = Assert.IsType<Request>(await ReceiveAsync(core));
            var two = Assert.IsType<Request>(await ReceiveAsync(core));
            Assert.Equal(("select_node", "node-hk"), (one.Command, one.Args.GetProperty("nodeId").GetString()));
            Assert.Equal("connect", two.Command);
            Assert.Empty(two.Args.EnumerateObject());
            Assert.NotEqual(one.Id, two.Id);

            // Answered in the other order: each answer still reaches the request it belongs to.
            await SendAsync(core, new Response(two.Id, Message.Null, null));
            await SendAsync(core, new Response(one.Id, Json("""{ "message": "Node selected" }"""), null));
            Assert.Equal("Node selected", (await first).GetProperty("message").GetString());
            Assert.Equal(JsonValueKind.Null, (await second).ValueKind);
        }
    }

    [Fact]
    public async Task ARefusalCarriesTheCoresSentence()
    {
        var (core, window, _) = await OpenAsync();
        await using (window)
        await using (core)
        {
            var asking = window.AskAsync("select_node", patience: Soon);
            var request = Assert.IsType<Request>(await ReceiveAsync(core));
            await SendAsync(core, new Response(request.Id, Message.Null, "the node is not in any subscription"));
            var refused = await Assert.ThrowsAsync<RefusedException>(() => asking);
            Assert.Equal("the node is not in any subscription", refused.Message);
        }
    }

    [Fact]
    public async Task TheWindowAnswersTheCoreWhileItWaitsForTheCore()
    {
        var (core, window, _) = await OpenAsync();
        await using (window)
        await using (core)
        {
            var asking = window.AskAsync("connect", patience: Soon);
            var request = Assert.IsType<Request>(await ReceiveAsync(core));

            // The core, answering "connect", asks the window about the user's session first.
            await SendAsync(core, new Request(41, "session.describe", Message.EmptyObject));
            var described = Assert.IsType<Response>(await ReceiveAsync(core));
            Assert.Equal(41ul, described.Id);
            Assert.Equal("Windows", described.Ok.GetProperty("desktop").GetString());

            await SendAsync(core, new Request(42, "session.set_start_after_login", Json("""{ "enabled": true }""")));
            var done = Assert.IsType<Response>(await ReceiveAsync(core));
            Assert.Equal((42ul, JsonValueKind.Null, null), (done.Id, done.Ok.ValueKind, done.Error));

            await SendAsync(core, new Response(request.Id, Json("""{ "message": "Connected" }"""), null));
            Assert.Equal("Connected", (await asking).GetProperty("message").GetString());
        }
    }

    [Fact]
    public async Task WhatTheWindowRefusesReachesTheCoreAsTheReason()
    {
        var (core, window, _) = await OpenAsync();
        await using (window)
        await using (core)
        {
            await SendAsync(core, new Request(1, "session.apply_proxy", Json("""{ "port": 7890 }""")));
            var refused = Assert.IsType<Response>(await ReceiveAsync(core));
            Assert.Equal((1ul, "the proxy settings are managed by this computer's administrator"), (refused.Id, refused.Error));

            // An error of the window's own is not passed on in its words.
            await SendAsync(core, new Request(2, "session.unknown", Message.EmptyObject));
            var failed = Assert.IsType<Response>(await ReceiveAsync(core));
            Assert.Equal((2ul, "the command stopped unexpectedly"), (failed.Id, failed.Error));
        }
    }

    [Fact]
    public async Task EventsArriveInOrder()
    {
        var (core, window, handler) = await OpenAsync();
        await using (window)
        await using (core)
        {
            for (var number = 0; number < 50; number++)
            {
                await SendAsync(core, new Event("tick", Json($$"""{ "number": {{number}} }""")));
            }

            await SendAsync(core, new Event("done", Message.Null));
            for (var number = 0; number < 50; number++)
            {
                Assert.True(handler.Events.TryTake(out var told, Soon));
                Assert.Equal(("tick", number), (told.Name, told.Payload.GetProperty("number").GetInt32()));
            }

            Assert.True(handler.Events.TryTake(out var last, Soon));
            Assert.Equal(("done", JsonValueKind.Null), (last.Name, last.Payload.ValueKind));
        }
    }

    [Fact]
    public async Task AnAnswerThatTakesTooLongIsGivenUpOn()
    {
        var (core, window, _) = await OpenAsync();
        await using (window)
        await using (core)
        {
            var asking = window.AskAsync("connect", patience: TimeSpan.FromMilliseconds(100));
            var request = Assert.IsType<Request>(await ReceiveAsync(core));
            await Assert.ThrowsAsync<TimeoutException>(() => asking);

            // The late answer is dropped, and the channel still works.
            await SendAsync(core, new Response(request.Id, Message.Null, null));
            var again = window.AskAsync("disconnect", patience: Soon);
            var next = Assert.IsType<Request>(await ReceiveAsync(core));
            await SendAsync(core, new Response(next.Id, Json("true"), null));
            Assert.True((await again).GetBoolean());
        }
    }

    [Fact]
    public async Task ClosingTheChannelFailsWhatIsStillWaiting()
    {
        var (core, window, _) = await OpenAsync();
        await using (window)
        {
            var asking = window.AskAsync("connect", patience: Soon);
            await ReceiveAsync(core);
            await core.DisposeAsync();
            await Assert.ThrowsAsync<ChannelClosedException>(() => asking);
            await window.Closed.WaitAsync(Soon);
            await Assert.ThrowsAsync<ChannelClosedException>(() => window.AskAsync("connect", patience: Soon));
        }
    }

    [Fact]
    public async Task SomethingThatIsNotAMessageBreaksTheChannel()
    {
        var (core, window, _) = await OpenAsync();
        await using (window)
        await using (core)
        {
            var asking = window.AskAsync("connect", patience: Soon);
            await ReceiveAsync(core);
            await Frame.WriteAsync(core, "{ \"type\": \"unknown\" }"u8.ToArray());
            await Assert.ThrowsAsync<ChannelClosedException>(() => asking);
            await Assert.ThrowsAsync<InvalidDataException>(() => window.Closed.WaitAsync(Soon));
        }
    }

    [Fact]
    public async Task OnlyTheSystemOrAdministratorsServeTheDefaultPipe()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }
        var user = WindowsIdentity.GetCurrent().User;
        Assert.NotNull(user);
        var other = new SecurityIdentifier("S-1-5-21-1-2-3-1002");
        var system = new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null);
        var administrators = new SecurityIdentifier(WellKnownSidType.BuiltinAdministratorsSid, null);
        Assert.True(Channel.TrustedOwner(system, Channel.DefaultPipeName, user));
        Assert.True(Channel.TrustedOwner(administrators, Channel.DefaultPipeName, user));
        // Another user's pipe, or even this user's, is not the installed core.
        Assert.False(Channel.TrustedOwner(other, Channel.DefaultPipeName, user));
        Assert.False(Channel.TrustedOwner(user, Channel.DefaultPipeName, user));
        // A core this user runs in a console on a pipe of its own.
        Assert.True(Channel.TrustedOwner(user, "anixops-net-test", user));
        Assert.False(Channel.TrustedOwner(other, "anixops-net-test", user));

        // A pipe this process serves belongs to this user (or, elevated, the administrators).
        var (core, window) = await PipeAsync();
        await using (core)
        await using (window)
        {
            Channel.VerifyServer((PipeStream)window, "anixops-net-test");
        }
    }
}
