using System.Globalization;
using System.Text;

namespace GameSessionTracker;

/// <summary>Writes the human-readable stats file and a spreadsheet-friendly CSV of every session.</summary>
internal static class ReportWriter
{
    public static void WriteStats(string path, IReadOnlyList<SessionRecord> sessions, IReadOnlyCollection<ActiveSession> active, DateTimeOffset now, bool readOnly = false)
    {
        var sb = new StringBuilder();
        sb.AppendLine("PLAYTIME TRACKER");
        sb.AppendLine($"Last updated: {FormatDateTime(now)}");
        sb.AppendLine();

        if (active.Count > 0)
        {
            sb.AppendLine("NOW PLAYING");
            foreach (var session in active.OrderBy(a => a.Start))
                sb.AppendLine($"  {session.Game} - started {FormatDateTime(session.Start)} ({FormatDuration(now - session.Start)} so far)");
            sb.AppendLine();
        }

        var games = sessions
            .GroupBy(s => s.Game, StringComparer.OrdinalIgnoreCase)
            .Select(g => new
            {
                Name = g.OrderByDescending(s => s.Start).First().Game,
                Sessions = g.OrderBy(s => s.Start).ToList(),
                Total = TimeSpan.FromTicks(g.Sum(s => s.Duration.Ticks)),
                LastPlayed = g.Max(s => s.End),
            })
            .OrderByDescending(g => g.Total)
            .ToList();

        if (games.Count == 0)
        {
            sb.AppendLine("No sessions recorded yet. Launch a game and it will show up here once you close it.");
            FileUtil.WriteAllTextAtomicWithBom(path, sb.ToString(), readOnly);
            return;
        }

        var allTime = TimeSpan.FromTicks(games.Sum(g => g.Total.Ticks));
        var nameWidth = Math.Clamp(games.Max(g => g.Name.Length), 4, 40);

        sb.AppendLine($"SUMMARY  ({games.Count} games, {sessions.Count} sessions, {FormatDuration(allTime)} total)");
        sb.AppendLine($"  {Pad("Game", nameWidth)}  {"Times played",12}  {"Total time",12}  {"Average",10}  Last played");
        sb.AppendLine($"  {new string('-', nameWidth)}  {new string('-', 12)}  {new string('-', 12)}  {new string('-', 10)}  {new string('-', 22)}");
        foreach (var g in games)
        {
            var average = TimeSpan.FromTicks(g.Total.Ticks / g.Sessions.Count);
            sb.AppendLine($"  {Pad(g.Name, nameWidth)}  {g.Sessions.Count,12}  {FormatDuration(g.Total),12}  {FormatDuration(average),10}  {FormatDateTime(g.LastPlayed)}");
        }

        sb.AppendLine();
        sb.AppendLine("SESSIONS BY GAME  (newest first)");
        foreach (var g in games)
        {
            sb.AppendLine();
            sb.AppendLine($"{g.Name}");
            sb.AppendLine($"  Played {g.Sessions.Count} {(g.Sessions.Count == 1 ? "time" : "times")}, {FormatDuration(g.Total)} total");
            for (var i = g.Sessions.Count - 1; i >= 0; i--)
            {
                var s = g.Sessions[i];
                sb.AppendLine($"  #{i + 1,-4} {FormatDateTime(s.Start)}  ->  {FormatEnd(s.Start, s.End)}   {FormatDuration(s.Duration)}");
            }
        }

        FileUtil.WriteAllTextAtomicWithBom(path, sb.ToString(), readOnly);
    }

    public static void WriteCsv(string path, IReadOnlyList<SessionRecord> sessions, bool readOnly = false)
    {
        var sb = new StringBuilder();
        sb.AppendLine("Game,Start,End,Duration (minutes),Duration,Executable");
        foreach (var s in sessions.OrderBy(s => s.Start))
        {
            sb.Append(Csv(s.Game)).Append(',')
              .Append(Csv(s.Start.ToLocalTime().ToString("yyyy-MM-dd HH:mm:ss", CultureInfo.InvariantCulture))).Append(',')
              .Append(Csv(s.End.ToLocalTime().ToString("yyyy-MM-dd HH:mm:ss", CultureInfo.InvariantCulture))).Append(',')
              .Append(s.Duration.TotalMinutes.ToString("0.0", CultureInfo.InvariantCulture)).Append(',')
              .Append(Csv(FormatDuration(s.Duration))).Append(',')
              .AppendLine(Csv(s.Executable ?? ""));
        }
        FileUtil.WriteAllTextAtomicWithBom(path, sb.ToString(), readOnly);
    }

    public static string FormatDuration(TimeSpan duration)
    {
        if (duration < TimeSpan.FromSeconds(1))
            return "0m";
        var totalHours = (int)duration.TotalHours;
        if (totalHours > 0)
            return $"{totalHours}h {duration.Minutes:00}m";
        if (duration.Minutes > 0)
            return $"{duration.Minutes}m {duration.Seconds:00}s";
        return $"{duration.Seconds}s";
    }

    private static string FormatDateTime(DateTimeOffset value) =>
        value.ToLocalTime().ToString("ddd MMM d, yyyy  h:mm tt", CultureInfo.InvariantCulture);

    private static string FormatEnd(DateTimeOffset start, DateTimeOffset end) =>
        start.ToLocalTime().Date == end.ToLocalTime().Date
            ? end.ToLocalTime().ToString("h:mm tt", CultureInfo.InvariantCulture)
            : FormatDateTime(end);

    private static string Pad(string text, int width) =>
        text.Length > width ? text[..(width - 1)] + "~" : text.PadRight(width);

    private static string Csv(string value)
    {
        // Game names come from folder names, the Xbox Game Bar list and other programs' files. A cell starting with
        // = + - @ (or a tab/CR) is run as a formula by Excel ("CSV injection"), so it's prefixed with ' to stay text.
        if (value.Length > 0 && value[0] is '=' or '+' or '-' or '@' or '\t' or '\r')
            value = "'" + value;
        return value.IndexOfAny(new[] { ',', '"', '\n', '\r' }) >= 0 ? "\"" + value.Replace("\"", "\"\"") + "\"" : value;
    }
}
