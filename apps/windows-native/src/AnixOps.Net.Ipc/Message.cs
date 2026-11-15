using System.Buffers;
using System.Text.Json;

namespace AnixOps.Net.Ipc;

/// <summary>
/// What goes in a frame. Either side sends requests and answers the other's; the core also sends events,
/// which expect no answer. The JSON is fixed by the samples in <c>crates/client-ipc/fixtures</c>.
/// </summary>
public abstract record Message
{
    /// <summary>The JSON value <c>null</c>, for answers and events that carry nothing.</summary>
    public static readonly JsonElement Null = JsonDocument.Parse("null").RootElement.Clone();

    /// <summary>An empty JSON object, for commands without arguments.</summary>
    public static readonly JsonElement EmptyObject = JsonDocument.Parse("{}").RootElement.Clone();

    /// <summary>Reads one message. Text that is not a message of this protocol is an error.</summary>
    public static Message Parse(ReadOnlyMemory<byte> utf8)
    {
        JsonDocument document;
        try
        {
            document = JsonDocument.Parse(utf8);
        }
        catch (JsonException error)
        {
            throw new InvalidDataException("the other side sent something that is not JSON", error);
        }

        using (document)
        {
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object || !root.TryGetProperty("type", out var type) || type.ValueKind != JsonValueKind.String)
            {
                throw new InvalidDataException("the other side sent something that is not a message");
            }

            try
            {
                return type.GetString() switch
                {
                    "hello" => new Hello(
                        root.GetProperty("protocol").GetUInt32(),
                        root.GetProperty("role").GetString() ?? string.Empty,
                        root.GetProperty("version").GetString() ?? string.Empty),
                    "request" => new Request(
                        root.GetProperty("id").GetUInt64(),
                        root.GetProperty("command").GetString() ?? string.Empty,
                        Optional(root, "args") ?? EmptyObject),
                    "response" => new Response(
                        root.GetProperty("id").GetUInt64(),
                        Optional(root, "ok") ?? Null,
                        root.TryGetProperty("error", out var error) && error.ValueKind == JsonValueKind.String ? error.GetString() : null),
                    "event" => new Event(
                        root.GetProperty("name").GetString() ?? string.Empty,
                        Optional(root, "payload") ?? Null),
                    var other => throw new InvalidDataException($"the other side sent a message of unknown type {other}"),
                };
            }
            catch (Exception error) when (error is KeyNotFoundException or InvalidOperationException or FormatException)
            {
                throw new InvalidDataException("the other side sent a message with a field missing or of the wrong kind", error);
            }
        }
    }

    /// <summary>The message as UTF-8 JSON, ready for a frame.</summary>
    public byte[] ToUtf8()
    {
        var buffer = new ArrayBufferWriter<byte>();
        using (var writer = new Utf8JsonWriter(buffer))
        {
            writer.WriteStartObject();
            Write(writer);
            writer.WriteEndObject();
        }

        return buffer.WrittenSpan.ToArray();
    }

    private protected abstract void Write(Utf8JsonWriter writer);

    /// <summary>A copy of the property that outlives the document, or <c>null</c> when it is absent or JSON null.</summary>
    private static JsonElement? Optional(JsonElement root, string name) =>
        root.TryGetProperty(name, out var value) && value.ValueKind != JsonValueKind.Null ? value.Clone() : null;
}

/// <summary>The first message each side sends: which protocol it speaks and which program it is.</summary>
public sealed record Hello(uint Protocol, string Role, string Version) : Message
{
    /// <summary>The role of a window.</summary>
    public const string Window = "window";

    /// <summary>The role of the core.</summary>
    public const string Core = "core";

    private protected override void Write(Utf8JsonWriter writer)
    {
        writer.WriteString("type", "hello");
        writer.WriteNumber("protocol", Protocol);
        writer.WriteString("role", Role);
        writer.WriteString("version", Version);
    }
}

/// <summary>Asks the other side to do <paramref name="Command"/>; the answer carries the same id.</summary>
public sealed record Request(ulong Id, string Command, JsonElement Args) : Message
{
    private protected override void Write(Utf8JsonWriter writer)
    {
        writer.WriteString("type", "request");
        writer.WriteNumber("id", Id);
        writer.WriteString("command", Command);
        writer.WritePropertyName("args");
        Args.WriteTo(writer);
    }
}

/// <summary>The answer to the request with the same id: a result, or a sentence saying why there is none.</summary>
public sealed record Response(ulong Id, JsonElement Ok, string? Error) : Message
{
    private protected override void Write(Utf8JsonWriter writer)
    {
        writer.WriteString("type", "response");
        writer.WriteNumber("id", Id);
        if (Error is not null)
        {
            writer.WriteString("error", Error);
        }
        else if (Ok.ValueKind is not (JsonValueKind.Null or JsonValueKind.Undefined))
        {
            writer.WritePropertyName("ok");
            Ok.WriteTo(writer);
        }
    }
}

/// <summary>Something one side tells the other without being asked. No answer is sent.</summary>
public sealed record Event(string Name, JsonElement Payload) : Message
{
    private protected override void Write(Utf8JsonWriter writer)
    {
        writer.WriteString("type", "event");
        writer.WriteString("name", Name);
        if (Payload.ValueKind is not (JsonValueKind.Null or JsonValueKind.Undefined))
        {
            writer.WritePropertyName("payload");
            Payload.WriteTo(writer);
        }
    }
}
