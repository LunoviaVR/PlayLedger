namespace GameSessionTracker.Ui;

/// <summary>An accent colour family: what the user picks under Settings → Appearance.</summary>
internal sealed record AccentSpec(
    string Key,
    string Name,
    Color Accent,          // dark theme: accent on dark glass
    Color Secondary,       // dark theme: secondary accent (chart tops, gradients)
    Color GradientStart,   // primary buttons, switches
    Color GradientEnd,
    Color LightAccent,     // light theme accent (darker, for contrast on light glass)
    Color Text);           // dark theme: accent-coloured text

/// <summary>The accent presets, custom colours, and how an accent is applied to a theme.</summary>
internal static class Accents
{
    public static readonly IReadOnlyList<AccentSpec> Presets = new[]
    {
        new AccentSpec("blue", "Blue", Theme.Hex("#60a5fa"), Theme.Hex("#818cf8"), Theme.Hex("#3b82f6"), Theme.Hex("#6366f1"), Theme.Hex("#2563eb"), Theme.Hex("#93c5fd")),
        new AccentSpec("violet", "Violet", Theme.Hex("#a78bfa"), Theme.Hex("#c084fc"), Theme.Hex("#8b5cf6"), Theme.Hex("#a855f7"), Theme.Hex("#7c3aed"), Theme.Hex("#c4b5fd")),
        new AccentSpec("teal", "Teal", Theme.Hex("#2dd4bf"), Theme.Hex("#38bdf8"), Theme.Hex("#14b8a6"), Theme.Hex("#0ea5e9"), Theme.Hex("#0f766e"), Theme.Hex("#99f6e4")),
        new AccentSpec("green", "Green", Theme.Hex("#34d399"), Theme.Hex("#a3e635"), Theme.Hex("#10b981"), Theme.Hex("#16a34a"), Theme.Hex("#047857"), Theme.Hex("#86efac")),
        new AccentSpec("amber", "Amber", Theme.Hex("#fbbf24"), Theme.Hex("#fb923c"), Theme.Hex("#f59e0b"), Theme.Hex("#f97316"), Theme.Hex("#b45309"), Theme.Hex("#fcd34d")),
        new AccentSpec("rose", "Rose", Theme.Hex("#fb7185"), Theme.Hex("#f472b6"), Theme.Hex("#f43f5e"), Theme.Hex("#ec4899"), Theme.Hex("#be123c"), Theme.Hex("#fda4af")),
    };

    public static AccentSpec Default => Presets[0];

    /// <summary>A preset key, or "#rrggbb" for a custom colour. Anything unreadable falls back to blue.</summary>
    public static AccentSpec Parse(string? value)
    {
        var key = value?.Trim().ToLowerInvariant() ?? "";
        var preset = Presets.FirstOrDefault(p => p.Key == key);
        if (preset is not null)
            return preset;
        if (key.Length == 7 && key[0] == '#' && int.TryParse(key.AsSpan(1), System.Globalization.NumberStyles.HexNumber, null, out var rgb))
            return FromColor(Color.FromArgb(255, (rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff));
        return Default;
    }

    public static string ToKey(Color color) => $"#{color.R:x2}{color.G:x2}{color.B:x2}";

    /// <summary>Builds a full accent family from one picked colour, keeping lightness in ranges that stay readable.</summary>
    public static AccentSpec FromColor(Color picked)
    {
        var (h, s, _) = ToHsl(picked);
        s = Math.Clamp(s, 0.45f, 0.95f);
        var hue2 = (h + 28) % 360;
        return new AccentSpec(ToKey(picked), "Custom",
            FromHsl(h, s, 0.68f), FromHsl(hue2, s, 0.72f),
            FromHsl(h, s, 0.52f), FromHsl(hue2, s, 0.55f),
            FromHsl(h, s, 0.38f), FromHsl(h, s, 0.80f));
    }

    public static Theme Apply(Theme theme, AccentSpec a)
    {
        var accent = theme.IsDark ? a.Accent : a.LightAccent;
        // White text on the button gradient unless the gradient is light (amber, lime...), then near-black.
        var mid = Glass.Lerp(a.GradientStart, a.GradientEnd, 0.5f);
        var onAccent = Luminance(mid) > 0.28 ? Theme.Hex("#0b1220") : Color.White;
        return theme with
        {
            Accent = accent,
            AccentSecondary = theme.IsDark ? a.Secondary : a.GradientEnd,
            AccentGradientStart = a.GradientStart,
            AccentGradientEnd = a.GradientEnd,
            AccentText = theme.IsDark ? a.Text : a.LightAccent,
            AccentSoft = Theme.WithAlpha(accent, theme.IsDark ? 36 : 26),
            AccentBorder = Theme.WithAlpha(accent, theme.IsDark ? 115 : 102),
            FocusRing = Theme.WithAlpha(accent, theme.IsDark ? 180 : 166),
            TextOnAccent = onAccent,
            // The ambient glows pick up the accent too, so the whole window shifts with it (subtly).
            Glows = new[]
            {
                Theme.WithAlpha(a.GradientStart, theme.Glows[0].A),
                Theme.WithAlpha(a.Secondary, theme.Glows[1].A),
                theme.Glows[2],
            },
        };
    }

    /// <summary>Relative luminance (WCAG).</summary>
    public static double Luminance(Color c)
    {
        static double Channel(int v)
        {
            var x = v / 255.0;
            return x <= 0.03928 ? x / 12.92 : Math.Pow((x + 0.055) / 1.055, 2.4);
        }
        return 0.2126 * Channel(c.R) + 0.7152 * Channel(c.G) + 0.0722 * Channel(c.B);
    }

    private static (float H, float S, float L) ToHsl(Color c) => (c.GetHue(), c.GetSaturation(), c.GetBrightness());

    private static Color FromHsl(float h, float s, float l)
    {
        var c = (1 - Math.Abs(2 * l - 1)) * s;
        var x = c * (1 - Math.Abs(h / 60 % 2 - 1));
        var m = l - c / 2;
        var (r, g, b) = (h % 360) switch
        {
            < 60 => (c, x, 0f),
            < 120 => (x, c, 0f),
            < 180 => (0f, c, x),
            < 240 => (0f, x, c),
            < 300 => (x, 0f, c),
            _ => (c, 0f, x),
        };
        return Color.FromArgb(255, (int)Math.Round((r + m) * 255), (int)Math.Round((g + m) * 255), (int)Math.Round((b + m) * 255));
    }
}
