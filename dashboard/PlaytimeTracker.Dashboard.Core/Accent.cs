using System.Globalization;

namespace PlaytimeTracker.Dashboard.Core;

/// <summary>An RGB colour.</summary>
public readonly record struct Rgb(byte R, byte G, byte B)
{
    public string ToHex() => $"#{R:x2}{G:x2}{B:x2}";

    /// <summary>Blends toward <paramref name="target"/> by <paramref name="amount"/> (0–1).</summary>
    public Rgb Mix(Rgb target, double amount)
    {
        byte Channel(byte from, byte to) => (byte)Math.Round(from + (to - from) * Math.Clamp(amount, 0, 1));
        return new Rgb(Channel(R, target.R), Channel(G, target.G), Channel(B, target.B));
    }

    public static Rgb? ParseHex(string text)
    {
        var hex = text.Trim().TrimStart('#');
        return hex.Length == 6 && int.TryParse(hex, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var value)
            ? new Rgb((byte)(value >> 16), (byte)(value >> 8), (byte)value)
            : null;
    }
}

/// <summary>The seven accent shades WinUI derives its accent brushes from.</summary>
public sealed record AccentShades(Rgb Base, Rgb Light1, Rgb Light2, Rgb Light3, Rgb Dark1, Rgb Dark2, Rgb Dark3);

/// <summary>
/// The accent setting shared with the tracker (and the C# app): a preset name ("blue", "violet", …), a custom
/// "#rrggbb", or "windows" to follow the Windows accent colour.
/// </summary>
public static class Accent
{
    public const string WindowsKey = "windows";

    /// <summary>The presets, with the same main colours as the C# app's.</summary>
    public static readonly IReadOnlyList<(string Key, string Name, Rgb Color)> Presets = new[]
    {
        ("blue", "Blue", new Rgb(0x3b, 0x82, 0xf6)),
        ("violet", "Violet", new Rgb(0x8b, 0x5c, 0xf6)),
        ("teal", "Teal", new Rgb(0x14, 0xb8, 0xa6)),
        ("green", "Green", new Rgb(0x10, 0xb9, 0x81)),
        ("amber", "Amber", new Rgb(0xf5, 0x9e, 0x0b)),
        ("rose", "Rose", new Rgb(0xf4, 0x3f, 0x5e)),
    };

    private static readonly Rgb White = new(255, 255, 255);
    private static readonly Rgb Black = new(0, 0, 0);

    /// <summary>The colour for a setting value; null means "use the Windows accent". Unreadable values are blue.</summary>
    public static Rgb? Resolve(string? key)
    {
        var value = (key ?? "").Trim().ToLowerInvariant();
        if (value == WindowsKey)
            return null;
        foreach (var preset in Presets)
        {
            if (preset.Key == value)
                return preset.Color;
        }
        return Rgb.ParseHex(value) ?? Presets[0].Color;
    }

    public static AccentShades Shades(Rgb color) => new(
        color,
        color.Mix(White, 0.25), color.Mix(White, 0.45), color.Mix(White, 0.65),
        color.Mix(Black, 0.2), color.Mix(Black, 0.4), color.Mix(Black, 0.6));
}
