using System.Text.Json;
using AnixOps.Net.Ipc;

namespace AnixOps.Net.Session;

/// <summary>Whether the window can reach the core.</summary>
public enum LinkState
{
    /// <summary>Not tried yet.</summary>
    Connecting,

    /// <summary>The core answers.</summary>
    Connected,

    /// <summary>The service is not running, or is not one this window can talk to.</summary>
    Unavailable,
}

/// <summary>What the window knows about its link to the core.</summary>
/// <param name="State">Whether the core can be reached.</param>
/// <param name="CoreVersion">The core's version while connected.</param>
/// <param name="Problem">Why not, while unavailable (for the log and diagnostics, not shown as it is).</param>
public sealed record LinkStatus(LinkState State, string? CoreVersion = null, string? Problem = null);

/// <summary>
/// The window's standing link to the core service: connects, answers the core with the handler, and
/// connects again whenever the service went away (it was stopped, upgraded or restarted).
/// </summary>
public sealed class CoreLink : IAsyncDisposable
{
    /// <summary>How long between two tries while the service is not there.</summary>
    public static readonly TimeSpan DefaultRetry = TimeSpan.FromSeconds(2);

    // One short try: a service that is not there does not appear by waiting on its pipe.
    private static readonly TimeSpan ConnectPatience = TimeSpan.FromMilliseconds(100);

    private readonly string version;
    private readonly IWindowHandler handler;
    private readonly string pipeName;
    private readonly TimeSpan retry;
    private readonly CancellationTokenSource stopping = new();
    private readonly object gate = new();
    private Channel? channel;
    private LinkStatus status = new(LinkState.Connecting);
    private Task? running;

    /// <param name="version">This window's version, told to the core.</param>
    /// <param name="handler">What answers the core.</param>
    /// <param name="pipeName">The pipe's name without <c>\\.\pipe\</c>.</param>
    /// <param name="retry">How long between two tries.</param>
    public CoreLink(string version, IWindowHandler handler, string pipeName = Channel.DefaultPipeName, TimeSpan? retry = null)
    {
        this.version = version;
        this.handler = handler;
        this.pipeName = pipeName;
        this.retry = retry ?? DefaultRetry;
    }

    /// <summary>The link as it is now.</summary>
    public LinkStatus Status
    {
        get
        {
            lock (gate)
            {
                return status;
            }
        }
    }

    /// <summary>The link changed. Raised on a background thread, in the order the changes happened.</summary>
    public event Action<LinkStatus>? Changed;

    /// <summary>Starts connecting in the background. Calling it again does nothing.</summary>
    public void Start()
    {
        lock (gate)
        {
            running ??= Task.Run(RunAsync);
        }
    }

    /// <summary>
    /// Asks the core. Without a connection this fails at once with <see cref="ChannelClosedException"/>;
    /// the other failures are those of <see cref="Channel.AskAsync"/>.
    /// </summary>
    public Task<JsonElement> AskAsync(string command, JsonElement? args = null, TimeSpan? patience = null, CancellationToken cancellation = default)
    {
        Channel? current;
        lock (gate)
        {
            current = channel;
        }
        return current is null
            ? Task.FromException<JsonElement>(new ChannelClosedException("the core service is not connected"))
            : current.AskAsync(command, args, patience, cancellation);
    }

    /// <summary>Stops connecting and closes the channel.</summary>
    public async ValueTask DisposeAsync()
    {
        Task? task;
        lock (gate)
        {
            if (stopping.IsCancellationRequested)
            {
                return;
            }
            stopping.Cancel();
            task = running;
        }
        if (task is not null)
        {
            await task.ConfigureAwait(false);
        }
        stopping.Dispose();
    }

    private async Task RunAsync()
    {
        var token = stopping.Token;
        while (!token.IsCancellationRequested)
        {
            Channel? opened = null;
            try
            {
                opened = await Channel.ConnectAsync(version, handler, pipeName, ConnectPatience, token).ConfigureAwait(false);
                Set(new LinkStatus(LinkState.Connected, opened.CoreVersion), opened);
                var ended = await Task.WhenAny(opened.Closed, Task.Delay(Timeout.Infinite, token)).ConfigureAwait(false);
                if (ended == opened.Closed)
                {
                    string? problem = null;
                    try
                    {
                        await opened.Closed.ConfigureAwait(false);
                    }
                    catch (Exception error) when (error is not OutOfMemoryException)
                    {
                        problem = error.Message;
                    }
                    Set(new LinkStatus(LinkState.Unavailable, Problem: problem ?? "the core service closed the channel"), null);
                }
            }
            catch (OperationCanceledException) when (token.IsCancellationRequested)
            {
            }
            catch (Exception error) when (error is TimeoutException or IOException or InvalidDataException or UnauthorizedAccessException)
            {
                // Not there, busy, not ours, or not for this user: all are "try again later".
                Set(new LinkStatus(LinkState.Unavailable, Problem: error.Message), null);
            }
            finally
            {
                if (opened is not null)
                {
                    lock (gate)
                    {
                        if (ReferenceEquals(channel, opened))
                        {
                            channel = null;
                        }
                    }
                    try
                    {
                        await opened.DisposeAsync().ConfigureAwait(false);
                    }
                    catch (Exception error) when (error is not OutOfMemoryException)
                    {
                        // How a channel that is being thrown away ended does not matter.
                    }
                }
            }

            try
            {
                await Task.Delay(retry, token).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
            }
        }
    }

    private void Set(LinkStatus next, Channel? current)
    {
        lock (gate)
        {
            channel = current;
            if (status == next)
            {
                return;
            }
            status = next;
        }
        Changed?.Invoke(next);
    }
}
