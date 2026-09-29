using Microsoft.Win32;

namespace GameSessionTracker;

/// <summary>Controls whether the tracker launches when you sign in to Windows (per-user, no admin needed).</summary>
internal static class StartupManager
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    // Where Task Manager's "Startup apps" page records apps you've disabled.
    private const string ApprovedKey = @"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    private const string ValueName = "GameSessionTracker";

    private static string Command => $"\"{Environment.ProcessPath}\" --startup";

    public static bool IsEnabled()
    {
        using var run = Registry.CurrentUser.OpenSubKey(RunKey);
        if (run?.GetValue(ValueName) is not string)
            return false;

        // First byte 0x02 = enabled, 0x03 = disabled in Task Manager. No value means enabled.
        using var approved = Registry.CurrentUser.OpenSubKey(ApprovedKey);
        return approved?.GetValue(ValueName) is not byte[] { Length: > 0 } state || state[0] % 2 == 0;
    }

    public static void SetEnabled(bool enabled)
    {
        using var run = Registry.CurrentUser.CreateSubKey(RunKey, writable: true);
        using var approved = Registry.CurrentUser.OpenSubKey(ApprovedKey, writable: true);
        if (enabled)
        {
            run.SetValue(ValueName, Command);
            approved?.DeleteValue(ValueName, throwOnMissingValue: false); // clear a "disabled" flag from Task Manager
        }
        else
        {
            run.DeleteValue(ValueName, throwOnMissingValue: false);
        }
    }

    /// <summary>If startup is on but the exe was moved, point the startup entry at the new location.</summary>
    public static void RefreshPathIfEnabled()
    {
        using var run = Registry.CurrentUser.OpenSubKey(RunKey, writable: true);
        if (run?.GetValue(ValueName) is string current && !string.Equals(current, Command, StringComparison.OrdinalIgnoreCase))
            run.SetValue(ValueName, Command);
    }
}
