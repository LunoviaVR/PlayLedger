using System.Globalization;
using System.Text;
using System.Text.Json.Nodes;
using PlaytimeTracker.Dashboard.Core;
using Xunit;

namespace PlaytimeTracker.Dashboard.Tests;

public class ProtocolTests
{
    private static string[] FixtureLines() =>
        File.ReadAllLines(Path.Combine(AppContext.BaseDirectory, "fixtures", "responses.jsonl"))
            .Where(l => l.Length > 0)
            .ToArray();

    [Fact]
    public void Parses_every_message_the_tracker_sends()
    {
        var responses = FixtureLines().Select(Protocol.ParseResponse).ToList();

        var dashboard = Assert.IsType<DashboardResponse>(responses[0]).Snapshot;
        Assert.Equal(42, dashboard.Revision);
        Assert.Equal("Hades", dashboard.Sessions[0].Game);
        Assert.True(dashboard.Sessions[0].IsLive);
        Assert.Equal(TimeSpan.FromMinutes(20), dashboard.Sessions[0].Duration);
        Assert.Null(dashboard.Sessions[1].Executable);
        Assert.Equal(new[] { "Elden Ring", "Beat Saber", "Hades" }, dashboard.Games.Select(g => g.Name));
        Assert.Equal(5700, dashboard.Games[0].TotalSeconds);
        Assert.Single(dashboard.Live);
        Assert.Equal(new DateOnly(2026, 9, 29), dashboard.Daily[^1].Day);
        Assert.Equal(new DateOnly(2026, 9, 29), dashboard.History[0].Day);
        Assert.Equal(11_460, dashboard.PastWeekSeconds);
        Assert.Equal("steam_1245620", dashboard.IdentityOf("elden ring")?.ArtworkKey);
        Assert.Equal(new DateTimeOffset(2026, 9, 29, 9, 20, 0, TimeSpan.Zero), dashboard.Now);

        var settings = Assert.IsType<SettingsResponse>(responses[1]);
        Assert.Equal(5, settings.Settings.PollIntervalSeconds);
        Assert.False(settings.Settings.OnlineArtwork);
        Assert.Contains("SteamVR", settings.Settings.IgnoredGames);
        Assert.False(settings.HasSteamGridDbKey);
        Assert.True(settings.StartWithWindows);
        Assert.True(settings.IsInstalledCopy);

        var events = responses.OfType<EventResponse>().Select(e => e.Event).ToList();
        Assert.Equal("Hades", Assert.IsType<SessionStartedEvent>(events[0]).Game);
        Assert.Equal(TimeSpan.FromMinutes(95), Assert.IsType<SessionEndedEvent>(events[1]).Session.Duration);
        Assert.Equal(43, Assert.IsType<DataChangedEvent>(events[2]).Revision);
        Assert.Equal("cover", Assert.IsType<ArtworkReadyEvent>(events[3]).Kind);
        Assert.Equal("2.3.0", Assert.IsType<UpdateAvailableEvent>(events[4]).Version);
        Assert.IsType<HeartbeatEvent>(events[5]);

        Assert.Equal(Protocol.Version, responses.OfType<HelloResponse>().Single().Protocol);
        Assert.Single(responses.OfType<OkResponse>());
        Assert.EndsWith("cover.jpg", responses.OfType<ArtworkResponse>().Single().Path);
        Assert.Equal("Game,Start\r\n", responses.OfType<CsvResponse>().Single().Text);
        Assert.Equal("nope", responses.OfType<ErrorResponse>().Single().Message);
        var update = responses.OfType<UpdateStatusResponse>().Single();
        Assert.Equal("2.3.0", update.AvailableVersion);
        Assert.True(update.CanInstall);
        Assert.Null(update.LastError);
        Assert.Equal(new DateTimeOffset(2026, 9, 29, 9, 0, 0, TimeSpan.Zero), update.LastChecked);
    }

    [Fact]
    public void Unknown_events_are_tolerated_and_unknown_responses_rejected()
    {
        var e = Assert.IsType<EventResponse>(Protocol.ParseResponse("""{"type":"event","event":{"type":"somethingNew","x":1}}"""));
        Assert.Equal("somethingNew", Assert.IsType<UnknownEvent>(e.Event).Type);
        Assert.Throws<System.Text.Json.JsonException>(() => Protocol.ParseResponse("""{"type":"surprise"}"""));
    }

    [Fact]
    public void Settings_keep_fields_this_dashboard_does_not_know()
    {
        var settings = new TrackerSettings(JsonNode.Parse("""{"pollIntervalSeconds":5,"futureOption":{"a":1},"customGames":[{"name":"Minecraft","executable":"javaw.exe"}]}""")!.AsObject());
        settings.PollIntervalSeconds = 1000;
        settings.OnlineArtwork = true;
        settings.CustomGames = settings.CustomGames.Append(new CustomGame(" Tetris ", "tetris.exe")).ToList();
        var json = settings.ToJson();
        Assert.Equal(300, json["pollIntervalSeconds"]!.GetValue<int>());
        Assert.True(json["onlineArtwork"]!.GetValue<bool>());
        Assert.Equal(1, json["futureOption"]!["a"]!.GetValue<int>());
        Assert.Equal("Tetris", settings.CustomGames[1].Name);
        settings.ThemeMode = "purple";
        Assert.Equal("system", settings.ThemeMode);
    }

    [Fact]
    public void Requests_match_the_trackers_names()
    {
        Assert.Equal("""{"type":"hello","protocol":1}""", Protocol.Hello().ToJsonString());
        var delete = Protocol.DeleteSession("Hades", new DateTimeOffset(2026, 9, 29, 9, 0, 0, TimeSpan.FromHours(2)));
        Assert.Equal("deleteSession", delete["type"]!.GetValue<string>());
        Assert.Equal("2026-09-29T09:00:00.0000000+02:00", delete["start"]!.GetValue<string>());
        Assert.Equal("""{"type":"setSteamGridDbKey","key":null}""", Protocol.SetSteamGridDbKey(null).ToJsonString());
        Assert.Equal("""{"type":"getArtwork","game":"Hades","kind":"cover"}""", Protocol.GetArtwork("Hades", ArtworkKinds.Cover).ToJsonString());
    }

    [Fact]
    public void Accent_settings_resolve_like_the_existing_app()
    {
        Assert.Equal(new Rgb(0x3b, 0x82, 0xf6), Accent.Resolve("blue"));
        Assert.Equal(new Rgb(0xf4, 0x3f, 0x5e), Accent.Resolve(" ROSE "));
        Assert.Equal(new Rgb(0xff, 0x88, 0x00), Accent.Resolve("#ff8800"));
        Assert.Null(Accent.Resolve("windows"));
        Assert.Equal(Accent.Presets[0].Color, Accent.Resolve("nonsense"));
        var shades = Accent.Shades(new Rgb(100, 100, 100));
        Assert.True(shades.Light3.R > shades.Light1.R && shades.Dark3.R < shades.Dark1.R);
        Assert.Equal("#646464", shades.Base.ToHex());
    }

    [Fact]
    public void Formats_like_the_existing_app()
    {
        Assert.Equal("0m", Format.Duration(0));
        Assert.Equal("45s", Format.Duration(45));
        Assert.Equal("12m 03s", Format.Duration(12 * 60 + 3));
        Assert.Equal("1248h 32m", Format.Duration(1248 * 3600 + 32 * 60));
        Assert.Equal("2 hours 5 minutes", Format.SpokenDuration(2 * 3600 + 5 * 60));
        var today = new DateOnly(2026, 9, 29);
        var culture = CultureInfo.InvariantCulture;
        Assert.Equal("Today", Format.Day(today, today, culture));
        Assert.Equal("Yesterday", Format.Day(today.AddDays(-1), today, culture));
        Assert.Equal("Sun, Sep 27", Format.Day(today.AddDays(-2), today, culture));
        Assert.Equal("Sep 27, 2025", Format.Day(new DateOnly(2025, 9, 27), today, culture));
    }
}

public class ConnectionTests
{
    /// <summary>A fake tracker: answers each request line with the next canned response.</summary>
    private sealed class ScriptedStream : Stream
    {
        private readonly Queue<string> _answers;
        private readonly MemoryStream _pending = new();
        public List<string> Written { get; } = new();

        public ScriptedStream(params string[] answers) => _answers = new Queue<string>(answers);

        public override void Write(byte[] buffer, int offset, int count)
        {
            Written.Add(Encoding.UTF8.GetString(buffer, offset, count));
            var answer = _answers.Count > 0 ? _answers.Dequeue() : "";
            var bytes = Encoding.UTF8.GetBytes(answer);
            var position = _pending.Position;
            _pending.Seek(0, SeekOrigin.End);
            _pending.Write(bytes);
            _pending.Position = position;
        }

        public override int Read(byte[] buffer, int offset, int count) => _pending.Read(buffer, offset, Math.Min(count, 7));
        public override bool CanRead => true;
        public override bool CanWrite => true;
        public override bool CanSeek => false;
        public override long Length => throw new NotSupportedException();
        public override long Position { get => throw new NotSupportedException(); set => throw new NotSupportedException(); }
        public override void Flush() { }
        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException();
    }

    [Fact]
    public async Task Requests_and_answers_are_paired_and_errors_surface()
    {
        var stream = new ScriptedStream(
            "{\"type\":\"event\",\"event\":{\"type\":\"heartbeat\"}}\r\n{\"type\":\"hello\",\"protocol\":1,\"version\":\"2.2.0\"}\n",
            "{\"type\":\"error\",\"message\":\"That session wasn't found.\"}\n");
        await using var connection = new TrackerConnection(stream);

        var hello = Assert.IsType<HelloResponse>(await connection.SendAsync(Protocol.Hello()));
        Assert.Equal("2.2.0", hello.Version);
        var error = await Assert.ThrowsAsync<TrackerErrorException>(() => connection.SendAsync(Protocol.GetDashboard()));
        Assert.Equal("That session wasn't found.", error.Message);
        Assert.Equal("{\"type\":\"hello\",\"protocol\":1}\n", stream.Written[0]);
        await Assert.ThrowsAsync<TrackerUnavailableException>(() => connection.SendAsync(Protocol.GetDashboard()));
    }

    [Fact]
    public async Task Subscriptions_yield_events_but_not_heartbeats()
    {
        var stream = new ScriptedStream(
            "{\"type\":\"ok\"}\n{\"type\":\"event\",\"event\":{\"type\":\"heartbeat\"}}\n{\"type\":\"event\",\"event\":{\"type\":\"dataChanged\",\"revision\":7}}\n");
        await using var connection = new TrackerConnection(stream);
        var events = new List<TrackerEvent>();
        await foreach (var e in connection.SubscribeAsync())
            events.Add(e);
        Assert.Equal(new TrackerEvent[] { new DataChangedEvent(7) }, events);
    }
}
