using System.Collections.Concurrent;
using System.IO.Pipes;
using System.Security.Principal;
using System.Text.Json;

namespace AnixOps.Net.Ipc;

/// <summary>What the window does with what the core sends it.</summary>
public interface IWindowHandler
{
    /// <summary>
    /// Answers a question of the core (the <c>session.*</c> requests: this user's proxy settings and login
    /// entry). Throw <see cref="RefusedException"/> with a sentence for the user when it cannot be done.
    /// </summary>
    Task<JsonElement> RequestAsync(string command, JsonElement args);

    /// <summary>Takes an event. Events arrive in the order the core sent them, one at a time.</summary>
    void Event(string name, JsonElement payload);
}

/// <summary>The other side answered a request with a reason instead of a result; the message is that reason.</summary>
public sealed class RefusedException(string message) : Exception(message);

/// <summary>The channel to the core closed, so the request has no answer.</summary>
public sealed class ChannelClosedException(string message, Exception? inner = null) : IOException(message, inner);

/// <summary>
/// The window's end of an open channel: asks the core, answers the core's questions and passes its events
/// on. The same behaviour as <c>client_ipc::Peer</c>.
/// </summary>
public sealed class Channel : IAsyncDisposable
{
    /// <summary>The version of the wire format. Both sides say theirs first and stop when they differ.</summary>
    public const uint Protocol = 1;

    /// <summary>The pipe the service listens on (<c>\\.\pipe\AnixOpsNet.Core</c>).</summary>
    public const string DefaultPipeName = "AnixOpsNet.Core";

    /// <summary>How long a request waits for its answer unless the caller says otherwise.</summary>
    public static readonly TimeSpan DefaultPatience = TimeSpan.FromSeconds(30);

    private readonly Stream stream;
    private readonly IWindowHandler handler;
    private readonly SemaphoreSlim writing = new(1, 1);
    private readonly ConcurrentDictionary<ulong, TaskCompletionSource<JsonElement>> waiting = new();
    private readonly CancellationTokenSource stopping = new();
    private long lastId;
    private volatile bool closed;

    private Channel(Stream stream, IWindowHandler handler, string coreVersion)
    {
        this.stream = stream;
        this.handler = handler;
        CoreVersion = coreVersion;
        Closed = Task.Run(ReadUntilClosedAsync);
    }

    /// <summary>The core's version, as it said in its greeting.</summary>
    public string CoreVersion { get; }

    /// <summary>Completes when the channel has closed, with the error when it broke.</summary>
    public Task Closed { get; }

    /// <summary>Connects to the service's pipe and greets it. The service learns who the user is but cannot act as them.</summary>
    public static async Task<Channel> ConnectAsync(
        string version,
        IWindowHandler handler,
        string pipeName = DefaultPipeName,
        TimeSpan? timeout = null,
        CancellationToken cancellation = default)
    {
        var pipe = new NamedPipeClientStream(".", pipeName, PipeDirection.InOut, PipeOptions.Asynchronous, TokenImpersonationLevel.Identification);
        try
        {
            await pipe.ConnectAsync(timeout ?? TimeSpan.FromSeconds(5), cancellation).ConfigureAwait(false);
            return await OpenAsync(pipe, version, handler, cancellation).ConfigureAwait(false);
        }
        catch
        {
            await pipe.DisposeAsync().ConfigureAwait(false);
            throw;
        }
    }

    /// <summary>
    /// Greets the core over <paramref name="stream"/> and starts listening. Fails when the other side speaks
    /// another protocol or is not the core. The channel owns the stream from here on.
    /// </summary>
    public static async Task<Channel> OpenAsync(Stream stream, string version, IWindowHandler handler, CancellationToken cancellation = default)
    {
        await Frame.WriteAsync(stream, new Hello(Protocol, Hello.Window, version).ToUtf8(), cancellation).ConfigureAwait(false);
        var payload = await Frame.ReadAsync(stream, cancellation).ConfigureAwait(false)
            ?? throw new ChannelClosedException("the channel closed before the core introduced itself");
        if (Message.Parse(payload) is not Hello hello)
        {
            throw new InvalidDataException("the other side did not introduce itself first");
        }

        if (hello.Protocol != Protocol)
        {
            throw new InvalidDataException($"the core speaks protocol {hello.Protocol}, this window {Protocol}");
        }

        if (hello.Role != Hello.Core)
        {
            throw new InvalidDataException("the other side is not the AnixOps Net core");
        }

        return new Channel(stream, handler, hello.Version);
    }

    /// <summary>
    /// Asks the core to do <paramref name="command"/> and waits for its answer. A refusal is a
    /// <see cref="RefusedException"/> carrying the core's sentence, no answer in time a
    /// <see cref="TimeoutException"/>, a closed channel a <see cref="ChannelClosedException"/>.
    /// </summary>
    public async Task<JsonElement> AskAsync(string command, JsonElement? args = null, TimeSpan? patience = null, CancellationToken cancellation = default)
    {
        var id = (ulong)Interlocked.Increment(ref lastId);
        var answer = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
        waiting[id] = answer;
        try
        {
            // Checked after the request is listed, so a channel that closes now still fails this request.
            if (closed)
            {
                throw new ChannelClosedException("the channel to the core closed");
            }

            try
            {
                await SendAsync(new Request(id, command, args ?? Message.EmptyObject), cancellation).ConfigureAwait(false);
            }
            catch (Exception error) when (error is IOException or ObjectDisposedException)
            {
                throw new ChannelClosedException("the channel to the core closed", error);
            }

            try
            {
                return await answer.Task.WaitAsync(patience ?? DefaultPatience, cancellation).ConfigureAwait(false);
            }
            catch (TimeoutException)
            {
                throw new TimeoutException($"the core did not answer {command} in time");
            }
        }
        finally
        {
            // An answer that still comes after this is dropped: nobody waits for it any more.
            waiting.TryRemove(id, out _);
        }
    }

    /// <summary>Closes the channel; requests still waiting fail.</summary>
    public async ValueTask DisposeAsync()
    {
        await stopping.CancelAsync().ConfigureAwait(false);
        await stream.DisposeAsync().ConfigureAwait(false);
        try
        {
            await Closed.ConfigureAwait(false);
        }
        catch (Exception error) when (error is IOException or InvalidDataException or ObjectDisposedException or OperationCanceledException)
        {
            // Closing on purpose: how the reading ended does not matter.
        }

        stopping.Dispose();
        writing.Dispose();
    }

    private async Task SendAsync(Message message, CancellationToken cancellation)
    {
        var payload = message.ToUtf8();
        await writing.WaitAsync(cancellation).ConfigureAwait(false);
        try
        {
            await Frame.WriteAsync(stream, payload, cancellation).ConfigureAwait(false);
        }
        finally
        {
            writing.Release();
        }
    }

    private async Task ReadUntilClosedAsync()
    {
        Exception? broken = null;
        try
        {
            while (await Frame.ReadAsync(stream, stopping.Token).ConfigureAwait(false) is { } payload)
            {
                switch (Message.Parse(payload))
                {
                    case Request request:
                        // On its own task: the core may be waiting for this answer while it answers the
                        // window, and the reading must go on meanwhile.
                        _ = Task.Run(() => AnswerAsync(request));
                        break;
                    case Response response:
                        if (waiting.TryRemove(response.Id, out var answer))
                        {
                            if (response.Error is not null)
                            {
                                answer.TrySetException(new RefusedException(response.Error));
                            }
                            else
                            {
                                answer.TrySetResult(response.Ok);
                            }
                        }

                        break;
                    case Event told:
                        handler.Event(told.Name, told.Payload);
                        break;
                    case Hello:
                        throw new InvalidDataException("the core introduced itself twice");
                }
            }
        }
        catch (Exception error)
        {
            broken = error;
            throw;
        }
        finally
        {
            closed = true;
            foreach (var id in waiting.Keys)
            {
                if (waiting.TryRemove(id, out var answer))
                {
                    answer.TrySetException(new ChannelClosedException("the channel to the core closed", broken));
                }
            }
        }
    }

    private async Task AnswerAsync(Request request)
    {
        Response response;
        try
        {
            response = new Response(request.Id, await handler.RequestAsync(request.Command, request.Args).ConfigureAwait(false), null);
        }
        catch (RefusedException refused)
        {
            response = new Response(request.Id, Message.Null, refused.Message);
        }
        catch (Exception)
        {
            // What went wrong inside the window is not the core's to show; it gets the same sentence the
            // core's side uses.
            response = new Response(request.Id, Message.Null, "the command stopped unexpectedly");
        }

        try
        {
            await SendAsync(response, stopping.Token).ConfigureAwait(false);
        }
        catch (Exception error) when (error is IOException or ObjectDisposedException or OperationCanceledException)
        {
            // A channel that closed meanwhile has nobody left to tell.
        }
    }
}
