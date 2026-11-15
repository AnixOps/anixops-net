using System;
using System.Text.Json;
using System.Threading.Tasks;
using AnixOps.Net.Ipc;
using AnixOps.Net.Session;
using Microsoft.Win32;
using Xunit;

namespace AnixOps.Net.App.UiTests;

/// <summary>
/// A second window of this user at a test's service, opened after the app's. The service asks the newest
/// window to change the user's settings, so what the app's buttons set off writes this window's registry
/// keys (a scratch key of the test's own), not the machine's real proxy settings. The test also asks the
/// core through it, as another program of the user would.
/// </summary>
internal sealed class SecondWindow : IAsyncDisposable
{
    private static readonly TimeSpan Patience = TimeSpan.FromSeconds(90);

    private readonly string scratch;

    private SecondWindow(string scratch, RegistryProxyStore proxy, CoreLink link)
    {
        this.scratch = scratch;
        Proxy = proxy;
        Link = link;
    }

    /// <summary>The proxy settings this window writes when the service asks.</summary>
    public RegistryProxyStore Proxy { get; }

    /// <summary>The window's link to the core.</summary>
    public CoreLink Link { get; }

    internal static JsonElement Json(string text)
    {
        using var document = JsonDocument.Parse(text);
        return document.RootElement.Clone();
    }

    /// <summary>Opens the window on the service's pipe and waits until the service has taken it in.</summary>
    public static async Task<SecondWindow> JoinAsync(string pipeName)
    {
        var scratch = $@"Software\AnixOps\Net\Tests\{Guid.NewGuid():N}";
        var proxy = new RegistryProxyStore($@"{scratch}\Proxy", announce: false);
        var handler = new SessionHandler(proxy, new RegistryLoginEntry(@"C:\Program Files\AnixOps\Net\AnixOpsNet.exe", $@"{scratch}\Run"));
        var link = new CoreLink("0.3.0", handler, pipeName, TimeSpan.FromMilliseconds(100));
        var window = new SecondWindow(scratch, proxy, link);
        try
        {
            link.Start();
            var deadline = DateTime.UtcNow + Patience;
            while (true)
            {
                // Once this is answered the service has taken the window in and can ask it.
                try
                {
                    await link.AskAsync("save_preferences", Json("""{ "preferences": { "startAfterLogin": false, "autoConnect": false } }"""));
                    return window;
                }
                catch (Exception problem) when (problem is RefusedException or ChannelClosedException)
                {
                    Assert.True(DateTime.UtcNow < deadline, $"the service did not take the second window in: {problem.Message}; the link is {link.Status}");
                    await Task.Delay(100);
                }
            }
        }
        catch
        {
            await window.DisposeAsync();
            throw;
        }
    }

    /// <summary>Asks the core.</summary>
    public Task<JsonElement> AskAsync(string command, string args = "{}") => Link.AskAsync(command, Json(args), Patience);

    public async ValueTask DisposeAsync()
    {
        await Link.DisposeAsync();
        Registry.CurrentUser.DeleteSubKeyTree(scratch, throwOnMissingSubKey: false);
    }
}
