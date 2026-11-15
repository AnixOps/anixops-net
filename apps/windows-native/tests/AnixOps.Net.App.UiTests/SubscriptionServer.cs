using System;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace AnixOps.Net.App.UiTests;

/// <summary>
/// A subscription on this machine: a link that hands out share links, with the headers a panel sends about
/// use and expiry. What it hands out is decided by the test, from how often it was downloaded.
/// </summary>
internal sealed class SubscriptionServer : IDisposable
{
    /// <summary>1000 bytes of 5000 used; the last second of 2033-05-17 UTC as the end, so the day it expires is the 18th.</summary>
    public const string UserInfo = "upload=100; download=900; total=5000; expire=1999999999";

    private readonly TcpListener listener = new(IPAddress.Loopback, 0);
    private readonly CancellationTokenSource stopping = new();
    private readonly Func<int, string> body;
    private int downloads;

    /// <param name="body">What the link hands out at its first, second, ... download.</param>
    public SubscriptionServer(Func<int, string> body)
    {
        this.body = body;
        listener.Start();
        Link = $"http://127.0.0.1:{((IPEndPoint)listener.LocalEndpoint).Port}/nodes";
        _ = Task.Run(ServeAsync);
    }

    /// <summary>The subscription's link.</summary>
    public string Link { get; }

    /// <summary>How often the link was downloaded.</summary>
    public int Downloads => Volatile.Read(ref downloads);

    private async Task ServeAsync()
    {
        while (!stopping.IsCancellationRequested)
        {
            TcpClient client;
            try
            {
                client = await listener.AcceptTcpClientAsync(stopping.Token);
            }
            catch (Exception error) when (error is OperationCanceledException or SocketException or ObjectDisposedException)
            {
                return;
            }
            _ = Task.Run(() => AnswerAsync(client));
        }
    }

    private async Task AnswerAsync(TcpClient client)
    {
        try
        {
            using (client)
            {
                var stream = client.GetStream();
                // The request has no body: it ends at the first empty line.
                var request = new StringBuilder();
                var buffer = new byte[4096];
                while (!request.ToString().Contains("\r\n\r\n", StringComparison.Ordinal))
                {
                    var read = await stream.ReadAsync(buffer, stopping.Token);
                    if (read == 0)
                    {
                        return;
                    }
                    request.Append(Encoding.ASCII.GetString(buffer, 0, read));
                }
                var content = Encoding.UTF8.GetBytes(body(Interlocked.Increment(ref downloads)));
                var head = "HTTP/1.1 200 OK\r\n"
                    + "content-type: text/plain; charset=utf-8\r\n"
                    + $"content-length: {content.Length}\r\n"
                    + $"subscription-userinfo: {UserInfo}\r\n"
                    + "connection: close\r\n\r\n";
                await stream.WriteAsync(Encoding.ASCII.GetBytes(head), stopping.Token);
                await stream.WriteAsync(content, stopping.Token);
                await stream.FlushAsync(stopping.Token);
            }
        }
        catch (Exception error) when (error is IOException or SocketException or OperationCanceledException or ObjectDisposedException)
        {
            // The downloader went away.
        }
    }

    public void Dispose()
    {
        stopping.Cancel();
        listener.Stop();
        stopping.Dispose();
    }
}
