namespace GameSessionTracker;

/// <summary>Finds the data folder, moving it over from the app's old name ("Game Session Tracker") the first time.</summary>
internal static class DataFolderMigration
{
    public const string FolderName = "Playtime Tracker";
    private const string LegacyFolderName = "Game Session Tracker";

    public static string Resolve(string documents)
    {
        var current = Path.Combine(documents, FolderName);
        var legacy = Path.Combine(documents, LegacyFolderName);
        if (Directory.Exists(current) || !Directory.Exists(legacy))
            return current;
        try
        {
            Directory.Move(legacy, current);
            return current;
        }
        catch
        {
            // Something has a file open (or the folder is synced and locked): keep using the old folder rather than
            // starting with an empty history. The move is retried on the next start.
            return legacy;
        }
    }
}
