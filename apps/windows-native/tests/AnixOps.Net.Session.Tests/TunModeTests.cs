using System.Net;
using System.Net.NetworkInformation;
using System.Net.Sockets;
using System.Text.Json;
using AnixOps.Net.Ipc;
using AnixOps.Net.Testing;
using Xunit;

namespace AnixOps.Net.Session.Tests;

/// <summary>
/// TUN mode through the service: the service makes the network adapter that carries every application, the
/// window needs no rights for it, and nothing of it is left after disconnecting.
///
/// These change the machine's routes for a few seconds, so they run only where they are asked for
/// (<see cref="Variable"/>), in a CI step of their own, apart from the tests that measure time.
/// </summary>
public sealed class TunModeTests
{
    /// <summary>Set to 1 to run these tests. They need administrator rights and <c>wintun.dll</c> beside the service.</summary>
    public const string Variable = "ANIXOPS_NET_TUN_TESTS";

    /// <summary>Set to 1 where the client is installed: the test then uses the installed service on its own pipe.</summary>
    public const string InstalledVariable = "ANIXOPS_NET_INSTALLED";

    // What the core names and numbers its adapter (platform-windows, WindowsTunSettings::default).
    private const string AdapterName = "AnixOps Net";
    private static readonly IPAddress AdapterAddress = IPAddress.Parse("172.19.0.1");

    private const string Executable = @"C:\Program Files\AnixOps\Net\AnixOpsNet.exe";

    private static bool Set(string variable) => Environment.GetEnvironmentVariable(variable) == "1";

    private static NetworkInterface? Adapter() =>
        NetworkInterface.GetAllNetworkInterfaces().FirstOrDefault(candidate => candidate.Name == AdapterName);

    /// <summary>The address this machine sends from towards the internet, which tells the route it takes.</summary>
    private static IPAddress SourceAddress()
    {
        using var socket = new Socket(AddressFamily.InterNetwork, SocketType.Dgram, ProtocolType.Udp);
        // Connecting a UDP socket sends nothing; it only picks the route. The address is a documentation one.
        socket.Connect(new IPEndPoint(IPAddress.Parse("203.0.113.1"), 9));
        return ((IPEndPoint)socket.LocalEndPoint!).Address;
    }

    /// <summary>
    /// Fetches a page without any proxy, so that it can only arrive through the machine's routes. A route
    /// that has just changed may cost a first try, so it tries until it works.
    /// </summary>
    private static async Task FetchAsync(Func<string, string> failure)
    {
        using var client = new HttpClient(new SocketsHttpHandler { UseProxy = false }) { Timeout = TimeSpan.FromSeconds(15) };
        var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(60);
        while (true)
        {
            string problem;
            try
            {
                using var answer = await client.GetAsync("https://github.com/robots.txt");
                if (answer.IsSuccessStatusCode)
                {
                    return;
                }
                problem = $"github.com answered {(int)answer.StatusCode}";
            }
            catch (Exception error) when (error is HttpRequestException or TaskCanceledException)
            {
                problem = error.Message;
            }
            Assert.True(DateTime.UtcNow < deadline, failure(problem));
            await Task.Delay(500);
        }
    }

    private static async Task CarriesEveryApplicationAsync(string pipeName, Func<string> serviceState)
    {
        using var proxyKey = new ScratchKey();
        using var runKey = new ScratchKey();
        var proxy = new RegistryProxyStore(proxyKey.Path, announce: false);
        var own = new ProxySettings(true, "proxy.example.com:3128", "*.example.org;<local>");
        proxy.Write(own);
        var handler = new SessionHandler(proxy, new RegistryLoginEntry(Executable, runKey.Path));

        Assert.Null(Adapter());
        var before = SourceAddress();
        Assert.NotEqual(AdapterAddress, before);

        await using var link = new CoreLink("0.3.0", handler, pipeName, TimeSpan.FromMilliseconds(100));
        link.Start();
        await Until.TrueAsync(() => link.Status.State == LinkState.Connected, () => $"the link is {link.Status}; {serviceState()}");

        await ServiceTests.AskOnceAdmittedAsync(link, "import_subscription", JsonSerializer.Serialize(new { location = ServiceTests.Node }));
        // Everything goes out directly, so that what the adapter carries really arrives: the test node
        // answers nobody.
        await link.AskAsync("set_routing_mode", ServiceTests.Json("""{ "mode": "direct" }"""), TimeSpan.FromSeconds(60));
        await link.AskAsync("set_tun_mode", ServiceTests.Json("""{ "enabled": true }"""));
        Assert.True((await link.AskAsync("runtime_snapshot")).GetProperty("tunModeEnabled").GetBoolean());
        try
        {
            // The first connection also installs the driver, which takes a while.
            await link.AskAsync("connect", patience: TimeSpan.FromSeconds(120));

            // The adapter is there and the machine's traffic leaves through it.
            await Until.TrueAsync(
                () => Adapter() is { OperationalStatus: OperationalStatus.Up },
                () => $"the adapter is {Adapter()?.OperationalStatus.ToString() ?? "not there"}; {serviceState()}");
            await Until.TrueAsync(() => SourceAddress().Equals(AdapterAddress), () => $"traffic still leaves from {SourceAddress()}");
            // The user's proxy settings are not touched: the adapter carries everything already.
            Assert.Equal(own, proxy.Read());

            // What goes into the adapter comes out at the other end.
            await FetchAsync(problem => $"nothing arrives through the adapter: {problem}; {serviceState()}");
            Assert.Equal(AdapterAddress, SourceAddress());

            // The mode is switched only while disconnected.
            await Assert.ThrowsAsync<RefusedException>(() => link.AskAsync("set_tun_mode", ServiceTests.Json("""{ "enabled": false }""")));
        }
        finally
        {
            await link.AskAsync("disconnect", patience: TimeSpan.FromSeconds(60));
        }

        // Disconnected: no adapter, the old route, the user's proxy as it was, and the network still works.
        await Until.TrueAsync(() => Adapter() is null, () => "the adapter is still there after disconnecting");
        await Until.TrueAsync(() => SourceAddress().Equals(before), () => $"traffic leaves from {SourceAddress()}, before it was {before}");
        Assert.Equal(own, proxy.Read());
        await FetchAsync(problem => $"the network does not work after disconnecting: {problem}");

        await link.AskAsync("set_tun_mode", ServiceTests.Json("""{ "enabled": false }"""));
        Assert.False((await link.AskAsync("runtime_snapshot")).GetProperty("tunModeEnabled").GetBoolean());
    }

    [Fact]
    public async Task TheServiceCarriesEveryApplicationThroughItsAdapter()
    {
        if (!Set(Variable) || Set(InstalledVariable))
        {
            return;
        }
        var executable = ServiceProcess.Executable() ?? throw new InvalidOperationException($"{ServiceProcess.ExecutableVariable} is not set");
        var library = Path.Combine(Path.GetDirectoryName(executable)!, "wintun.dll");
        Assert.True(File.Exists(library), $"{library} is not there");

        using var service = ServiceProcess.Start();
        await CarriesEveryApplicationAsync(service.PipeName, () => service.HasExited(out var code) ? $"the service exited with code {code}" : "the service runs");
    }

    [Fact]
    public async Task TheInstalledServiceCarriesEveryApplicationThroughItsAdapter()
    {
        if (!Set(Variable) || !Set(InstalledVariable))
        {
            return;
        }
        await CarriesEveryApplicationAsync(Channel.DefaultPipeName, () => "the installed service");
    }
}
