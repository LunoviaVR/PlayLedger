using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using PlaytimeTracker.Dashboard.Core;

namespace PlaytimeTracker.Dashboard;

/// <summary>
/// Applies the accent setting. Controls don't read <c>SystemAccentColor</c> directly: their templates use accent
/// brushes (<c>AccentFillColorDefaultBrush</c> and so on) that WinUI defines once in its theme dictionaries and shares
/// between every control. Adding new brushes elsewhere isn't picked up by controls that are already loaded, so the
/// colour of those shared brushes is changed in place, which every control using them shows at once. Each brush gets
/// the shade WinUI gives it; their opacity is kept. High-contrast themes are left alone. "windows" puts the original
/// colours back.
/// </summary>
public static class AccentTheme
{
    private enum Shade { Base, Light1, Light2, Light3, Dark1, Dark2, Dark3 }

    /// <summary>Each accent brush and its shade in the light theme, then in the dark theme.</summary>
    private static readonly Dictionary<string, (Shade Light, Shade Dark)> Brushes = new()
    {
        ["AccentFillColorDefaultBrush"] = (Shade.Dark1, Shade.Light2),
        ["AccentFillColorSecondaryBrush"] = (Shade.Dark1, Shade.Light2),
        ["AccentFillColorTertiaryBrush"] = (Shade.Dark1, Shade.Light2),
        ["AccentTextFillColorPrimaryBrush"] = (Shade.Dark2, Shade.Light3),
        ["AccentTextFillColorSecondaryBrush"] = (Shade.Dark3, Shade.Light3),
        ["AccentTextFillColorTertiaryBrush"] = (Shade.Dark1, Shade.Light2),
        ["AccentFillColorSelectedTextBackgroundBrush"] = (Shade.Base, Shade.Base),
        ["SystemControlHighlightAccentBrush"] = (Shade.Base, Shade.Base),
    };

    /// <summary>The colours the shared brushes had before any accent was applied.</summary>
    private static readonly Dictionary<SolidColorBrush, Windows.UI.Color> Originals = new();

    private static string? _applied;

    public static void Apply(string? accent, FrameworkElement? root)
    {
        var key = (accent ?? "").Trim().ToLowerInvariant();
        if (key == _applied)
            return;
        _applied = key;
        var shades = Accent.Resolve(key) is { } color ? Accent.Shades(color) : null;
        var seen = new HashSet<ResourceDictionary>();
        foreach (var (dictionary, dark) in ThemeDictionaries(Application.Current.Resources, seen))
        {
            foreach (var (name, shade) in Brushes)
            {
                if (!dictionary.TryGetValue(name, out var value) || value is not SolidColorBrush brush)
                    continue;
                if (!Originals.ContainsKey(brush))
                    Originals[brush] = brush.Color;
                brush.Color = shades is null ? Originals[brush] : ToColor(Pick(shades, dark ? shade.Dark : shade.Light));
            }
        }
        Refresh(root);
    }

    /// <summary>
    /// The light and dark theme dictionaries under <paramref name="dictionary"/>, including those of merged
    /// dictionaries such as WinUI's own <c>XamlControlsResources</c>. WinUI uses "Default" for the dark theme.
    /// </summary>
    private static IEnumerable<(ResourceDictionary Dictionary, bool Dark)> ThemeDictionaries(ResourceDictionary dictionary, HashSet<ResourceDictionary> seen)
    {
        if (!seen.Add(dictionary))
            yield break;
        foreach (var (name, value) in dictionary.ThemeDictionaries)
        {
            if (value is not ResourceDictionary theme || name is not ("Light" or "Dark" or "Default"))
                continue;
            yield return (theme, name is not "Light");
            foreach (var merged in theme.MergedDictionaries)
            {
                foreach (var found in ThemeDictionaries(merged, seen))
                    yield return found;
                yield return (merged, name is not "Light");
            }
        }
        foreach (var merged in dictionary.MergedDictionaries)
        {
            foreach (var found in ThemeDictionaries(merged, seen))
                yield return found;
        }
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
