using System.Text;
using System.Text.Json.Nodes;

namespace PlaytimeTracker.Dashboard.Core;

/// <summary>The tracker went away or sent something unusable.</summary>
public sealed class TrackerUnavailableException : Exception
{
    public TrackerUnavailableException(string message, Exception? inner = null) : base(message, inner) { }
}

/// <summary>The tracker answered with an error message meant for the user.</summary>
public sealed class TrackerErrorException : Exception
{
    public TrackerErrorException(string message) : base(message) { }
}

/// <summary>
/// One connection to the tracker over any duplex stream (the named pipe in the app, a test stream in tests):
/// requests are written as JSON lines and answered in order.
/// </summary>
public sealed class TrackerConnection : IAsyncDisposable
{
    private readonly Stream _stream;
    private readonly SemaphoreSlim _gate = new(1, 1);
    private readonly byte[] _buffer = new byte[64 * 1024];
    private int _bufferStart;
    private int _bufferEnd;

    public TrackerConnection(Stream stream) => _stream = stream;

    /// <summary>Sends a request and returns its response. Error responses throw <see cref="TrackerErrorException"/>.</summary>
    public async Task<TrackerResponse> SendAsync(JsonObject request, CancellationToken cancellationToken = default)
    {
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            await WriteAsync(request, cancellationToken).ConfigureAwait(false);
            while (true)
            {
                var line = await ReadLineAsync(cancellationToken).ConfigureAwait(false)
                           ?? throw new TrackerUnavailableException("Playtime Tracker closed the connection.");
                var response = Parse(line);
                if (response is EventResponse)
                    continue; // events belong to subscription connections; never an answer
                return response is ErrorResponse error ? throw new TrackerErrorException(error.Message) : response;
            }
        }
        finally
        {
            _gate.Release();
        }
    }

    /// <summary>Turns this connection into an event stream (after it answers "ok").</summary>
    public async IAsyncEnumerable<TrackerEvent> SubscribeAsync(
        [System.Runtime.CompilerServices.EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        await SendAsync(Protocol.Subscribe(), cancellationToken).ConfigureAwait(false);
        while (true)
        {
            var line = await ReadLineAsync(cancellationToken).ConfigureAwait(false);
            if (line is null)
                yield break;
            if (Parse(line) is EventResponse { Event: var e } && e is not HeartbeatEvent and not UnknownEvent)
                yield return e;
        }
    }

    private static TrackerResponse Parse(string line)
    {
        try
        {
            return Protocol.ParseResponse(line);
        }
        catch (Exception ex) when (ex is System.Text.Json.JsonException or InvalidOperationException or FormatException)
        {
            throw new TrackerUnavailableException("Playtime Tracker sent a message this dashboard doesn't understand.", ex);
        }
    }

    private async Task WriteAsync(JsonObject request, CancellationToken cancellationToken)
    {
        var bytes = Encoding.UTF8.GetBytes(request.ToJsonString(Protocol.Json) + "\n");
        try
        {
            await _stream.WriteAsync(bytes, cancellationToken).ConfigureAwait(false);
            await _stream.FlushAsync(cancellationToken).ConfigureAwait(false);
        }
        catch (IOException ex)
        {
            throw new TrackerUnavailableException("Lost the connection to Playtime Tracker.", ex);
        }
    }

    /// <summary>Reads one line (without the newline); null at end of stream. Lines over the limit are an error.</summary>
    internal async Task<string?> ReadLineAsync(CancellationToken cancellationToken)
    {
        using var line = new MemoryStream();
        while (true)
        {
            if (_bufferStart == _bufferEnd)
            {
                int read;
                try
                {
                    read = await _stream.ReadAsync(_buffer, cancellationToken).ConfigureAwait(false);
                }
                catch (IOException ex)
                {
                    throw new TrackerUnavailableException("Lost the connection to Playtime Tracker.", ex);
                }
                if (read == 0)
                {
                    if (line.Length == 0)
                        return null;
                    throw new TrackerUnavailableException("Playtime Tracker's message was cut off.");
                }
                _bufferStart = 0;
                _bufferEnd = read;
            }
            var newline = Array.IndexOf(_buffer, (byte)'\n', _bufferStart, _bufferEnd - _bufferStart);
            var end = newline >= 0 ? newline : _bufferEnd;
            if (line.Length + (end - _bufferStart) > Protocol.MaxMessageBytes)
                throw new TrackerUnavailableException("Playtime Tracker sent a message that's too large.");
            line.Write(_buffer, _bufferStart, end - _bufferStart);
            _bufferStart = newline >= 0 ? newline + 1 : _bufferEnd;
            if (newline >= 0)
            {
                var text = Encoding.UTF8.GetString(line.GetBuffer(), 0, (int)line.Length);
                return text.EndsWith('\r') ? text[..^1] : text;
            }
        }
    }

    public async ValueTask DisposeAsync()
    {
        await _stream.DisposeAsync().ConfigureAwait(false);
        _gate.Dispose();
    }
}
