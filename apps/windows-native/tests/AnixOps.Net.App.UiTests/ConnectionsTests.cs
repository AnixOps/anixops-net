using System;
using System.Collections.Generic;
using System.Linq;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Automation;
using AnixOps.Net.Presentation;
using AnixOps.Net.Testing;
using Xunit;

namespace AnixOps.Net.App.UiTests;

/// <summary>
/// The list of open connections over the overview page: connections held open through the engine are listed,
/// searched, closed one by one and all at once.
/// </summary>
public sealed class ConnectionsTests
{
    private const string State = "Overview.State";
    private const string Connect = "Overview.Connect";
    private const string Node = "Overview.Node";
    private const string Open = "Overview.OpenConnections";
    private const string Listing = "Connections.List";
    private const string Search = "Connections.Search";
    private const string Count = "Connections.Count";
    private const string CloseAll = "Connections.CloseAll";
    private const string Close = "Connections.Close";
    private const string Notice = "Connections.Notice";

    // The dialog's own button, as Windows names it.
    private const string Leave = "CloseButton";

    // Starting the engine on a busy CI machine takes its time.
    private static readonly TimeSpan Patience = TimeSpan.FromSeconds(90);

    private static void Wait(Func<bool> done, Func<string> otherwise)
    {
        var deadline = DateTime.UtcNow + Patience;
        while (!done())
        {
            Assert.True(DateTime.UtcNow < deadline, otherwise());
            Thread.Sleep(50);
        }
    }

    private static AutomationElement? Present(AppSession app, string automationId) =>
        app.Window.FindFirst(TreeScope.Descendants, new PropertyCondition(AutomationElement.AutomationIdProperty, automationId));

    private static void Press(AutomationElement button, string what)
    {
        Assert.True(button.TryGetCurrentPattern(InvokePattern.Pattern, out var invoke), $"{what} cannot be pressed");
        ((InvokePattern)invoke).Invoke();
    }

    /// <summary>The rows of the list that exist as controls, in order; none when the list is not there.</summary>
    private static List<AutomationElement> Rows(AppSession app)
    {
        try
        {
            return Present(app, Listing) is { } found
                ? found
                    .FindAll(TreeScope.Descendants, new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.ListItem))
                    .Cast<AutomationElement>()
                    .ToList()
                : [];
        }
        catch (ElementNotAvailableException)
        {
            // The list changed while it was read.
            return [];
        }
    }

    /// <summary>Where each listed connection goes, in order.</summary>
    private static List<string> Titles(AppSession app)
    {
        try
        {
            return Rows(app).Select(row => row.Current.Name).ToList();
        }
        catch (ElementNotAvailableException)
        {
            return [];
        }
    }

    private static List<string> Says(AutomationElement row)
    {
        try
        {
            return row
                .FindAll(TreeScope.Descendants, new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Text))
                .Cast<AutomationElement>()
                .Select(text => text.Current.Name)
                .ToList();
        }
        catch (ElementNotAvailableException)
        {
            return [];
        }
    }

    private static string Read(AppSession app, string automationId)
    {
        try
        {
            return Present(app, automationId)?.Current.Name ?? "";
        }
        catch (ElementNotAvailableException)
        {
            return "";
        }
    }

    /// <summary>A connection to host:port through the engine's local proxy, left open.</summary>
    private static TcpClient Through(IPEndPoint proxy, string host, int port)
    {
        var client = new TcpClient();
        client.Connect(proxy);
        var stream = client.GetStream();
        stream.ReadTimeout = (int)Patience.TotalMilliseconds;
        stream.Write(Encoding.ASCII.GetBytes($"CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n"));
        var head = new StringBuilder();
        var one = new byte[1];
        while (!head.ToString().EndsWith("\r\n\r\n", StringComparison.Ordinal))
        {
            Assert.True(stream.Read(one, 0, 1) == 1, $"the engine's proxy closed the connection to {host}:{port} after '{head}'");
            head.Append((char)one[0]);
        }
        var status = head.ToString().Split(' ');
        Assert.True(status.Length > 1 && status[1] == "200", $"the engine's proxy answered the connection to {host}:{port} with '{head}'");
        return client;
    }

    /// <summary>Whether the other side has closed that connection.</summary>
    private static bool Ended(TcpClient client)
    {
        try
        {
            var socket = client.Client;
            return socket.Poll(100_000, SelectMode.SelectRead) && socket.Available == 0;
        }
        catch (Exception error) when (error is SocketException or ObjectDisposedException)
        {
            return true;
        }
    }

    [Theory]
    [InlineData(Strings.Chinese, "light")]
    [InlineData(Strings.English, "dark")]
    public async Task ConnectionsAreListedSearchedAndClosedThroughTheService(string language, string theme)
    {
        if (!ServiceProcess.Available())
        {
            return;
        }
        var strings = Strings.For(language);
        using var service = ServiceProcess.Start();
        using var app = AppSession.Start("--language", language, "--theme", theme, "--pipe", service.PipeName);
        app.WaitForName(Node, strings["overview.node.none"]);
        Assert.Equal(strings["overview.connections.open"], app.Find(Open).Current.Name);

        // Connecting writes the second window's proxy settings, not the machine's real ones.
        await using var second = await SecondWindow.JoinAsync(service.PipeName);

        async Task<ConnectionList> ListedAsync(string query = "") =>
            Connections.Read(await second.AskAsync(Connections.Command, Connections.Args(query).GetRawText()));

        // Before connecting there is no engine whose connections could be listed, and the list says so.
        Assert.False((await ListedAsync()).Running);
        Press(app.Find(Open), "the button that opens the connections");
        app.WaitForName(Notice, strings["connections.stopped"]);
        Assert.Equal(strings["connections.search"], app.Find(Search).Current.Name);
        Assert.Equal(strings["connections.close_all"], app.Find(CloseAll).Current.Name);
        Assert.False(app.Find(CloseAll).Current.IsEnabled);
        Press(app.Find(Leave), "the dialog's close button");
        Wait(() => Present(app, Search) is null, () => "the list of connections did not close");

        // Traffic leaves directly: connecting works without a node, and needs no server anywhere.
        await second.AskAsync("set_direct_egress", """{ "enabled": true }""");
        app.WaitForName(Node, strings["overview.node.direct"]);
        Wait(() => app.Find(Connect).Current.IsEnabled, () => "the overview does not offer to connect");
        Press(app.Find(Connect), "the button that connects");
        app.WaitForName(State, strings["overview.state.connected"]);
        var settings = second.Proxy.Read();
        Assert.True(settings.Enabled, "connecting did not point the second window's proxy settings at the engine");
        var proxy = IPEndPoint.Parse(settings.Server);

        // Two connections of this test's own, held open through the engine to a server on this machine.
        var server = new TcpListener(IPAddress.Loopback, 0);
        server.Start();
        var held = new List<TcpClient>();
        using var stopped = new CancellationTokenSource();
        var accepting = Task.Run(async () =>
        {
            try
            {
                while (true)
                {
                    var client = await server.AcceptTcpClientAsync(stopped.Token);
                    lock (held)
                    {
                        held.Add(client);
                    }
                }
            }
            catch (Exception error) when (error is OperationCanceledException or SocketException or ObjectDisposedException)
            {
                // The test is over.
            }
        });
        var port = ((IPEndPoint)server.LocalEndpoint).Port;
        var byName = $"localhost:{port}";
        var byAddress = $"127.0.0.1:{port}";
        var first = Through(proxy, "127.0.0.1", port);
        var other = Through(proxy, "localhost", port);
        try
        {
            var deadline = DateTime.UtcNow + Patience;
            while (true)
            {
                var targets = (await ListedAsync()).Open.Select(connection => connection.Target).ToList();
                if (targets.Contains(byName) && targets.Contains(byAddress))
                {
                    break;
                }
                Assert.True(DateTime.UtcNow < deadline, $"the core lists [{string.Join(", ", targets)}], expected {byAddress} and {byName}");
                await Task.Delay(100);
            }

            // What the dialog lists and counts is what the core lists, once both have settled.
            async Task<ConnectionsShown> SettledAsync(string query = "")
            {
                var until = DateTime.UtcNow + Patience;
                while (true)
                {
                    var expected = Connections.Show(strings, await ListedAsync(query), query, [], false);
                    var titles = Titles(app);
                    var count = Read(app, Count);
                    if (titles.SequenceEqual(expected.Rows.Select(row => row.Title)) && count == expected.Count)
                    {
                        return expected;
                    }
                    Assert.True(
                        DateTime.UtcNow < until,
                        $"the dialog lists [{string.Join(", ", titles)}] under '{count}', the core [{string.Join(", ", expected.Rows.Select(row => row.Title))}] under '{expected.Count}'");
                    await Task.Delay(100);
                }
            }

            Press(app.Find(Open), "the button that opens the connections");
            var shown = await SettledAsync();
            Assert.Contains(byName, shown.Rows.Select(row => row.Title));
            Assert.Contains(byAddress, shown.Rows.Select(row => row.Title));
            Assert.Equal(strings["overview.connections"], app.Find(Listing).Current.Name);
            Assert.True(app.Find(CloseAll).Current.IsEnabled);
            foreach (var connection in (await ListedAsync()).Open)
            {
                // Under where it goes, a row says by which rule and through which exit.
                var row = Rows(app).First(item => item.Current.Name == connection.Target);
                var how = string.Join(" · ", new[] { connection.Rule, string.Join(" → ", connection.Route) }.Where(part => part.Length > 0));
                Assert.Contains(Says(row), line => line.Contains(how, StringComparison.Ordinal));
                var button = row.FindFirst(TreeScope.Descendants, new PropertyCondition(AutomationElement.AutomationIdProperty, Close));
                Assert.True(button is not null, $"the row of {connection.Target} has no button that closes the connection");
                Assert.Equal(strings["connections.close"], button!.Current.Name);
            }

            // A search leaves the connections it names, and says how many of how many.
            app.SetText(Search, "localhost");
            var searched = await SettledAsync("localhost");
            Assert.Equal(new[] { byName }, searched.Rows.Select(row => row.Title).ToArray());
            app.SetText(Search, "no such connection");
            app.WaitForName(Notice, strings["connections.no_match"]);
            app.SetText(Search, "");
            await SettledAsync();
            app.Screenshot($"overview-connections-{language}-{theme}");

            // Closing one connection ends that one, for the program that holds it too.
            var target = Rows(app).First(item => item.Current.Name == byName);
            Press(
                target.FindFirst(TreeScope.Descendants, new PropertyCondition(AutomationElement.AutomationIdProperty, Close))!,
                $"the button that closes {byName}");
            Wait(() => Ended(other), () => $"the connection to {byName} is still open");
            Wait(
                () => !Titles(app).Contains(byName) && Titles(app).Contains(byAddress),
                () => $"the dialog lists [{string.Join(", ", Titles(app))}] after {byName} was closed");
            Assert.False(Ended(first), $"closing the connection to {byName} closed the one to {byAddress} too");

            // "Close all" ends the rest.
            Press(app.Find(CloseAll), "the button that closes every connection");
            Wait(() => Ended(first), () => $"the connection to {byAddress} is still open");
            app.WaitForName(Notice, strings["connections.empty"]);
            app.WaitForName(Count, strings.Format("connections.count", "0"));
            Assert.False(app.Find(CloseAll).Current.IsEnabled);

            Press(app.Find(Leave), "the dialog's close button");
            Wait(() => Present(app, Search) is null, () => "the list of connections did not close");
        }
        finally
        {
            first.Dispose();
            other.Dispose();
            stopped.Cancel();
            server.Stop();
            await accepting;
            lock (held)
            {
                foreach (var client in held)
                {
                    client.Dispose();
                }
            }
        }
    }
}
