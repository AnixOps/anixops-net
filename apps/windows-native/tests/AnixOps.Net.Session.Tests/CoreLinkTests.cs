using System.IO.Pipes;
using System.Text.Json;
using AnixOps.Net.Ipc;
using AnixOps.Net.Testing;
using Microsoft.Win32;
using Xunit;

namespace AnixOps.Net.Session.Tests;

internal static class Until
{
    public static readonly TimeSpan Soon = TimeSpan.FromSeconds(30);

    /// <summary>Waits for something that happens on another thread or in another process.</summary>
    public static async Task TrueAsync(Func<bool> done, Func<string> failure)
    {
        var deadline = DateTime.UtcNow + Soon;
        while (!done())
        {
            Assert.True(DateTime.UtcNow < deadline, failure());
            await Task.Delay(20);
        }
    }

    public static Task StateAsync(CoreLink link, LinkState state) =>
        TrueAsync(() => link.Status.State == state, () => $"the link is {link.Status}, expected {state}");
}

/// <summary>The link against a core played by the test, over a real named pipe.</summary>
public sealed class CoreLinkTests
{
    private static readonly TimeSpan Quick = TimeSpan.FromMilliseconds(50);

    private static string NewPipe() => "anixops-net-link-test-" + Guid.NewGuid().ToString("N");

    private static NamedPipeServerStream Listen(string pipe) =>
        new(pipe, PipeDirection.InOut, NamedPipeServerStream.MaxAllowedServerInstances, PipeTransmissionMode.Byte, PipeOptions.Asynchronous);

    /// <summary>Waits for a window on <paramref name="server"/> and greets it as the core.</summary>
    private static async Task GreetAsync(NamedPipeServerStream server, string version)
    {
        await server.WaitForConnectionAsync().WaitAsync(Until.Soon);
        var payload = await Frame.ReadAsync(server).WaitAsync(Until.Soon);
        Assert.NotNull(payload);
        var hello = Assert.IsType<Hello>(Message.Parse(payload));
        Assert.Equal(Hello.Window, hello.Role);
        await Frame.WriteAsync(server, new Hello(Channel.Protocol, Hello.Core, version).ToUtf8());
    }

    private static SessionHandler Handler() => new(new FakeProxyStore(new ProxySettings(false, "", "")), new FakeLoginEntry());

    [Fact]
    public async Task WithoutAServiceTheLinkSaysSoAndAsksFailAtOnce()
    {
        await using var link = new CoreLink("0.3.0", Handler(), NewPipe(), Quick);
        Assert.Equal(LinkState.Connecting, link.Status.State);
        link.Start();
        await Until.StateAsync(link, LinkState.Unavailable);
        Assert.NotNull(link.Status.Problem);
        await Assert.ThrowsAsync<ChannelClosedException>(() => link.AskAsync("runtime_snapshot"));
    }

    [Fact]
    public async Task TheLinkConnectsAsksAndConnectsAgainAfterTheServiceWentAway()
    {
        var pipe = NewPipe();
        var states = new List<LinkState>();
        await using var link = new CoreLink("0.3.0", Handler(), pipe, Quick);
        link.Changed += status =>
        {
            lock (states)
            {
                // Why the service cannot be reached may change while it is away; the state is what counts.
                if (states.Count == 0 || states[^1] != status.State)
                {
                    states.Add(status.State);
                }
            }
        };

        var first = Listen(pipe);
        link.Start();
        await GreetAsync(first, "1.0.0");
        await Until.StateAsync(link, LinkState.Connected);
        Assert.Equal("1.0.0", link.Status.CoreVersion);

        // A question goes to the core and its answer comes back.
        var asking = link.AskAsync("list_subscriptions");
        var request = Assert.IsType<Request>(Message.Parse((await Frame.ReadAsync(first).WaitAsync(Until.Soon))!));
        Assert.Equal("list_subscriptions", request.Command);
        await Frame.WriteAsync(first, new Response(request.Id, JsonDocument.Parse("[]").RootElement.Clone(), null).ToUtf8());
        Assert.Equal(JsonValueKind.Array, (await asking.WaitAsync(Until.Soon)).ValueKind);

        // The service stops (an upgrade, a crash) and a newer one starts on the same pipe.
        await first.DisposeAsync();
        await Until.StateAsync(link, LinkState.Unavailable);
        await Assert.ThrowsAsync<ChannelClosedException>(() => link.AskAsync("list_subscriptions"));

        await using var second = Listen(pipe);
        await GreetAsync(second, "1.0.1");
        await Until.TrueAsync(() => link.Status.CoreVersion == "1.0.1", () => $"the link is {link.Status}");

        lock (states)
        {
            Assert.Equal(new[] { LinkState.Connected, LinkState.Unavailable, LinkState.Connected }, states.ToArray());
        }
    }

    [Fact]
    public async Task ClosingTheLinkClosesTheChannel()
    {
        var pipe = NewPipe();
        await using var server = Listen(pipe);
        var link = new CoreLink("0.3.0", Handler(), pipe, Quick);
        link.Start();
        await GreetAsync(server, "1.0.0");
        await Until.StateAsync(link, LinkState.Connected);
        await link.DisposeAsync();
        // The core sees the window leave.
        Assert.Null(await Frame.ReadAsync(server).WaitAsync(Until.Soon));
        // Closing twice is not an error.
        await link.DisposeAsync();
    }
}

/// <summary>
/// The window's side against the real service (apps/windows-service, run in the foreground): what the core
/// asks of the user's session arrives here and is done, on registry keys of the test's own.
/// </summary>
public sealed class ServiceTests
{
    private const string Executable = @"C:\Program Files\AnixOps\Net\AnixOpsNet.exe";

    // A node nobody answers at: connecting only has to start the local proxy.
    internal const string Node = "ss://YWVzLTI1Ni1nY206ZjQzYzBlZWUtMTNiOS00ZjA3LWJlYzktZDRiNzQ0MTQxNTAz@203.0.113.9:11111#Test";

    internal static JsonElement Json(string text) => JsonDocument.Parse(text).RootElement.Clone();

    /// <summary>
    /// Asks until the core has taken this window in: it asks who the window is right after the greeting, and
    /// until that is answered it refuses what needs the user's session.
    /// </summary>
    internal static async Task<JsonElement> AskOnceAdmittedAsync(CoreLink link, string command, string args)
    {
        var deadline = DateTime.UtcNow + Until.Soon;
        while (true)
        {
            try
            {
                return await link.AskAsync(command, Json(args));
            }
            catch (RefusedException refused) when (refused.Message.Contains("window is not open", StringComparison.Ordinal) && DateTime.UtcNow < deadline)
            {
                await Task.Delay(50);
            }
        }
    }

    [Fact]
    public async Task TheServiceChangesTheUsersSettingsThroughTheWindow()
    {
        if (!ServiceProcess.Available())
        {
            return;
        }
        using var proxyKey = new ScratchKey();
        using var runKey = new ScratchKey();
        var proxy = new RegistryProxyStore(proxyKey.Path, announce: false);
        var login = new RegistryLoginEntry(Executable, runKey.Path);
        // The user's own proxy, which must be there again afterwards.
        var own = new ProxySettings(true, "proxy.example.com:3128", "*.example.org;<local>");
        proxy.Write(own);

        using var service = ServiceProcess.Start();
        await using var link = new CoreLink("0.3.0", new SessionHandler(proxy, login), service.PipeName, TimeSpan.FromMilliseconds(100));
        link.Start();
        await Until.TrueAsync(
            () => link.Status.State == LinkState.Connected,
            () => service.HasExited(out var code) ? $"the service exited with code {code}" : $"the link is {link.Status}");
        Assert.False(string.IsNullOrWhiteSpace(link.Status.CoreVersion));

        // Start after login: the service asks the window, the window writes the user's Run key.
        Assert.False(login.Enabled);
        await AskOnceAdmittedAsync(link, "save_preferences", """{ "preferences": { "startAfterLogin": true, "autoConnect": false } }""");
        Assert.True(login.Enabled);
        Assert.True((await link.AskAsync("runtime_snapshot")).GetProperty("startAfterLogin").GetBoolean());
        await link.AskAsync("save_preferences", Json("""{ "preferences": { "startAfterLogin": false, "autoConnect": false } }"""));
        Assert.False(login.Enabled);

        // Connecting: the engine starts in the service and the window points the user's proxy at it.
        await link.AskAsync("import_subscription", JsonSerializer.SerializeToElement(new { location = Node }), TimeSpan.FromSeconds(60));
        await link.AskAsync("connect", patience: TimeSpan.FromSeconds(60));
        var applied = proxy.Read();
        Assert.True(applied.Enabled);
        Assert.StartsWith("127.0.0.1:", applied.Server, StringComparison.Ordinal);
        Assert.Equal(ProxySettings.LocalBypass, applied.Bypass);
        Assert.Equal("Enabled", (await link.AskAsync("runtime_snapshot")).GetProperty("proxy").GetProperty("label").GetString());

        // Disconnecting puts back what the user had.
        await link.AskAsync("disconnect", patience: TimeSpan.FromSeconds(60));
        Assert.Equal(own, proxy.Read());

        // What only a window may be asked is not a command of the core.
        await Assert.ThrowsAsync<RefusedException>(() => link.AskAsync("session.apply_proxy", Json("""{ "port": 1 }""")));
    }

    [Fact]
    public async Task TheRootIsTrustedInTheUsersOwnStoreAndTheServiceNotesIt()
    {
        if (!ServiceProcess.Available())
        {
            return;
        }
        using var proxyKey = new ScratchKey();
        using var runKey = new ScratchKey();
        var roots = new FakeRootStore();
        var handler = new SessionHandler(new RegistryProxyStore(proxyKey.Path, announce: false), new RegistryLoginEntry(Executable, runKey.Path), roots);

        using var service = ServiceProcess.Start();
        await using var link = new CoreLink("0.3.0", handler, service.PipeName, TimeSpan.FromMilliseconds(100));
        link.Start();
        await Until.StateAsync(link, LinkState.Connected);

        // Once this is answered the core has taken the window in and can ask it.
        await AskOnceAdmittedAsync(link, "save_preferences", """{ "preferences": { "startAfterLogin": false, "autoConnect": false } }""");

        // Nothing is generated or trusted before the user asks.
        var status = await link.AskAsync("mitm_root_status");
        Assert.False(status.GetProperty("generated").GetBoolean());
        Assert.False(status.GetProperty("trusted").GetBoolean());
        Assert.Empty(roots.Trusted);

        // The user's own request: the service makes the root and has the window add it to the user's store.
        await link.AskAsync("open_mitm_root", patience: TimeSpan.FromSeconds(60));
        var certificate = Assert.Single(roots.Trusted);
        Assert.StartsWith("-----BEGIN CERTIFICATE-----", certificate, StringComparison.Ordinal);
        status = await link.AskAsync("mitm_root_status");
        Assert.True(status.GetProperty("generated").GetBoolean());
        Assert.True(status.GetProperty("trusted").GetBoolean());

        // What removing the client takes out again is written down beside the service's data.
        using var parsed = System.Security.Cryptography.X509Certificates.X509Certificate2.CreateFromPem(certificate);
        var noted = await File.ReadAllLinesAsync(Path.Combine(service.Directory, "trusted-roots.txt"));
        Assert.Equal(new[] { parsed.Thumbprint }, noted);
        Assert.Contains("AnixOps Net HTTPS Decryption Root", parsed.Subject, StringComparison.Ordinal);

        // A second request changes nothing.
        await link.AskAsync("open_mitm_root", patience: TimeSpan.FromSeconds(60));
        Assert.Single(roots.Trusted);
        Assert.Single(await File.ReadAllLinesAsync(Path.Combine(service.Directory, "trusted-roots.txt")));

        await link.AskAsync("remove_mitm_root_trust", patience: TimeSpan.FromSeconds(60));
        Assert.Empty(roots.Trusted);
        Assert.False((await link.AskAsync("mitm_root_status")).GetProperty("trusted").GetBoolean());

        // The user answers No to Windows: the request fails with the reason.
        roots.UserAgrees = false;
        var refused = await Assert.ThrowsAsync<RefusedException>(() => link.AskAsync("open_mitm_root", patience: TimeSpan.FromSeconds(60)));
        Assert.Contains("answered with No", refused.Message, StringComparison.Ordinal);
        Assert.False((await link.AskAsync("mitm_root_status")).GetProperty("trusted").GetBoolean());
    }

    [Fact]
    public async Task TheWindowFindsTheServiceAgainAfterItRestarted()
    {
        if (!ServiceProcess.Available())
        {
            return;
        }
        using var proxyKey = new ScratchKey();
        using var runKey = new ScratchKey();
        var handler = new SessionHandler(new RegistryProxyStore(proxyKey.Path, announce: false), new RegistryLoginEntry(Executable, runKey.Path));

        using var service = ServiceProcess.Start();
        await using var link = new CoreLink("0.3.0", handler, service.PipeName, TimeSpan.FromMilliseconds(100));
        link.Start();
        await Until.StateAsync(link, LinkState.Connected);
        await AskOnceAdmittedAsync(link, "import_subscription", JsonSerializer.Serialize(new { location = Node }));

        service.Stop();
        await Until.StateAsync(link, LinkState.Unavailable);
        await Assert.ThrowsAsync<ChannelClosedException>(() => link.AskAsync("runtime_snapshot"));

        // The same service again, as after an upgrade: same pipe, same data.
        using var again = ServiceProcess.Start(service.PipeName, service.Directory);
        await Until.StateAsync(link, LinkState.Connected);
        var subscriptions = await link.AskAsync("list_subscriptions");
        Assert.Equal(1, subscriptions.GetArrayLength());
    }
}
