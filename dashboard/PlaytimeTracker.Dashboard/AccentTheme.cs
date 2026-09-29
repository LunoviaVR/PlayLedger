using Microsoft.UI.Xaml;
using PlaytimeTracker.Dashboard.Core;
using Microsoft.UI;

namespace PlaytimeTracker.Dashboard;

/// <summary>
/// Applies the accent setting by overriding the colours WinUI derives every accent brush from
/// (<c>SystemAccentColor</c> and its light/dark shades). "windows" removes the overrides and follows Windows.
/// </summary>
public static class AccentTheme
{
    private static readonly string[] Keys =
    {
        "SystemAccentColor",
        "SystemAccentColorLight1", "SystemAccentColorLight2", "SystemAccentColorLight3",
        "SystemAccentColorDark1", "SystemAccentColorDark2", "SystemAccentColorDark3",
    };

    private static string? _applied;

    public static void Apply(string? accent, FrameworkElement? root)
    {
        var key = (accent ?? "").Trim().ToLowerInvariant();
        if (key == _applied)
            return;
        _applied = key;
        var resources = Application.Current.Resources;
        if (Accent.Resolve(key) is not { } color)
        {
            foreach (var name in Keys)
                resources.Remove(name);
        }
        else
        {
            var shades = Accent.Shades(color);
            var values = new[] { shades.Base, shades.Light1, shades.Light2, shades.Light3, shades.Dark1, shades.Dark2, shades.Dark3 };
            for (var i = 0; i < Keys.Length; i++)
                resources[Keys[i]] = ColorHelper.FromArgb(255, values[i].R, values[i].G, values[i].B);
        }
        Refresh(root);
    }

    /// <summary>Theme resources are resolved when the theme changes, so flip it and back to pick up new colours.</summary>
    private static void Refresh(FrameworkElement? root)
    {
        if (root is null)
            return;
        var theme = root.RequestedTheme;
        var actual = root.ActualTheme;
        root.RequestedTheme = actual == ElementTheme.Dark ? ElementTheme.Light : ElementTheme.Dark;
        root.RequestedTheme = theme;
    }
}
