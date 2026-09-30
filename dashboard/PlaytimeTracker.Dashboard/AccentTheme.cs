using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using PlaytimeTracker.Dashboard.Core;

namespace PlaytimeTracker.Dashboard;

/// <summary>
/// Applies the accent setting. Controls don't read <c>SystemAccentColor</c> directly: their templates use accent
/// brushes (<c>AccentFillColorDefaultBrush</c> and so on) that WinUI defines in its own theme dictionaries, so
/// overriding the colours alone changes nothing on screen. The brushes are overridden too, per theme, in the app's
/// theme dictionaries, which control templates look in first, using the shade and opacity WinUI gives each one.
/// "windows" removes the overrides and follows Windows.
/// </summary>
public static class AccentTheme
{
    private static readonly string[] ColorKeys =
    {
        "SystemAccentColor",
        "SystemAccentColorLight1", "SystemAccentColorLight2", "SystemAccentColorLight3",
        "SystemAccentColorDark1", "SystemAccentColorDark2", "SystemAccentColorDark3",
    };

    private enum Shade { Base, Light1, Light2, Light3, Dark1, Dark2, Dark3 }

    /// <summary>Each accent brush: its shade and opacity in the light theme, then in the dark theme.</summary>
    private static readonly (string Key, Shade Light, double LightOpacity, Shade Dark, double DarkOpacity)[] Brushes =
    {
        ("AccentFillColorDefaultBrush", Shade.Dark1, 1.0, Shade.Light2, 1.0),
        ("AccentFillColorSecondaryBrush", Shade.Dark1, 0.9, Shade.Light2, 0.9),
        ("AccentFillColorTertiaryBrush", Shade.Dark1, 0.8, Shade.Light2, 0.8),
        ("AccentTextFillColorPrimaryBrush", Shade.Dark2, 1.0, Shade.Light3, 1.0),
        ("AccentTextFillColorSecondaryBrush", Shade.Dark3, 1.0, Shade.Light3, 1.0),
        ("AccentTextFillColorTertiaryBrush", Shade.Dark1, 1.0, Shade.Light2, 1.0),
        ("AccentFillColorSelectedTextBackgroundBrush", Shade.Base, 1.0, Shade.Base, 1.0),
        ("SystemControlHighlightAccentBrush", Shade.Base, 1.0, Shade.Base, 1.0),
    };

    /// <summary>Theme dictionary keys; WinUI uses "Default" for the dark theme.</summary>
    private static readonly (string Name, bool Dark)[] Themes = { ("Light", false), ("Dark", true), ("Default", true) };

    private static string? _applied;

    public static void Apply(string? accent, FrameworkElement? root)
    {
        var key = (accent ?? "").Trim().ToLowerInvariant();
        if (key == _applied)
            return;
        _applied = key;
        var resources = Application.Current.Resources;
        var color = Accent.Resolve(key);
        var shades = color is { } c ? Accent.Shades(c) : null;
        foreach (var (name, dark) in Themes)
        {
            var theme = ThemeDictionary(resources, name);
            if (shades is null)
            {
                foreach (var k in ColorKeys)
                    theme.Remove(k);
                foreach (var brush in Brushes)
                    theme.Remove(brush.Key);
                continue;
            }
            for (var i = 0; i < ColorKeys.Length; i++)
                theme[ColorKeys[i]] = ToColor(Pick(shades, (Shade)i));
            foreach (var brush in Brushes)
            {
                var shade = dark ? brush.Dark : brush.Light;
                theme[brush.Key] = new SolidColorBrush(ToColor(Pick(shades, shade)))
                {
                    Opacity = dark ? brush.DarkOpacity : brush.LightOpacity,
                };
            }
        }
        // Older copies of the overrides at the top level (they never took effect) must not shadow the theme ones.
        foreach (var k in ColorKeys)
            resources.Remove(k);
        Refresh(root);
    }

    private static ResourceDictionary ThemeDictionary(ResourceDictionary resources, string name)
    {
        if (resources.ThemeDictionaries.TryGetValue(name, out var existing) && existing is ResourceDictionary dictionary)
            return dictionary;
        var created = new ResourceDictionary();
        resources.ThemeDictionaries[name] = created;
        return created;
    }

    private static Rgb Pick(AccentShades shades, Shade shade) => shade switch
    {
        Shade.Light1 => shades.Light1,
        Shade.Light2 => shades.Light2,
        Shade.Light3 => shades.Light3,
        Shade.Dark1 => shades.Dark1,
        Shade.Dark2 => shades.Dark2,
        Shade.Dark3 => shades.Dark3,
        _ => shades.Base,
    };

    private static Windows.UI.Color ToColor(Rgb rgb) => ColorHelper.FromArgb(255, rgb.R, rgb.G, rgb.B);

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
