using System.Drawing.Drawing2D;
using System.Runtime.InteropServices;
using Microsoft.Win32;

namespace GameSessionTracker.Ui;

/// <summary>Colours for the dashboard. Follows the Windows light/dark app setting.</summary>
internal sealed class Theme
{
    public required bool IsDark { get; init; }
    public required Color Window { get; init; }        // page background
    public required Color Surface { get; init; }       // cards
    public required Color Border { get; init; }
    public required Color TextPrimary { get; init; }
    public required Color TextSecondary { get; init; }
    public required Color TextMuted { get; init; }
    public required Color Accent { get; init; }        // the one data colour
    public required Color Track { get; init; }         // empty part of bars, gridlines
    public required Color Selection { get; init; }
    public required Color Hover { get; init; }
    public required Color Live { get; init; }          // "now playing" dot (always paired with text)

    public static readonly Theme Light = new()
    {
        IsDark = false,
        Window = ColorTranslator.FromHtml("#f4f4f2"),
        Surface = ColorTranslator.FromHtml("#fcfcfb"),
        Border = ColorTranslator.FromHtml("#e4e3de"),
        TextPrimary = ColorTranslator.FromHtml("#0b0b0b"),
        TextSecondary = ColorTranslator.FromHtml("#52514e"),
        TextMuted = ColorTranslator.FromHtml("#6f6e69"),
        Accent = ColorTranslator.FromHtml("#2a78d6"),
        Track = ColorTranslator.FromHtml("#ebeae6"),
        Selection = ColorTranslator.FromHtml("#e8f0fb"),
        Hover = ColorTranslator.FromHtml("#f3f3f0"),
        Live = ColorTranslator.FromHtml("#1a8a3a"),
    };

    public static readonly Theme Dark = new()
    {
        IsDark = true,
        Window = ColorTranslator.FromHtml("#111110"),
        Surface = ColorTranslator.FromHtml("#1a1a19"),
        Border = ColorTranslator.FromHtml("#2b2b29"),
        TextPrimary = ColorTranslator.FromHtml("#ffffff"),
        TextSecondary = ColorTranslator.FromHtml("#c3c2b7"),
        TextMuted = ColorTranslator.FromHtml("#96958c"),
        Accent = ColorTranslator.FromHtml("#3987e5"),
        Track = ColorTranslator.FromHtml("#2a2a28"),
        Selection = ColorTranslator.FromHtml("#1d2a3b"),
        Hover = ColorTranslator.FromHtml("#212120"),
        Live = ColorTranslator.FromHtml("#3fb950"),
    };

    public static Theme Current()
    {
        try
        {
            var value = Registry.GetValue(@"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme", 1);
            return value is int i && i == 0 ? Dark : Light;
        }
        catch
        {
            return Light;
        }
    }

    /// <summary>Dark title bar and dark scrollbars to match the theme (no-ops on older Windows).</summary>
    public static void ApplyWindowChrome(Form form, bool dark)
    {
        try
        {
            var on = dark ? 1 : 0;
            DwmSetWindowAttribute(form.Handle, DwmwaUseImmersiveDarkMode, ref on, sizeof(int));
        }
        catch
        {
            // Not supported on this Windows version.
        }
    }

    public static void ApplyScrollbarTheme(Control control, bool dark)
    {
        try
        {
            SetWindowTheme(control.Handle, dark ? "DarkMode_Explorer" : "Explorer", null);
        }
        catch
        {
            // Not supported on this Windows version.
        }
    }

    public static GraphicsPath RoundedRect(RectangleF r, float radius)
    {
        var path = new GraphicsPath();
        var d = Math.Min(radius * 2, Math.Min(r.Width, r.Height));
        if (d <= 0)
        {
            path.AddRectangle(r);
            return path;
        }
        path.AddArc(r.X, r.Y, d, d, 180, 90);
        path.AddArc(r.Right - d, r.Y, d, d, 270, 90);
        path.AddArc(r.Right - d, r.Bottom - d, d, d, 0, 90);
        path.AddArc(r.X, r.Bottom - d, d, d, 90, 90);
        path.CloseFigure();
        return path;
    }

    /// <summary>A rectangle with only the top corners rounded (bar ends), anchored flat on the baseline.</summary>
    public static GraphicsPath TopRoundedRect(RectangleF r, float radius)
    {
        var path = new GraphicsPath();
        var d = Math.Min(radius * 2, Math.Min(r.Width, r.Height * 2));
        if (d <= 0.5f)
        {
            path.AddRectangle(r);
            return path;
        }
        path.AddArc(r.X, r.Y, d, d, 180, 90);
        path.AddArc(r.Right - d, r.Y, d, d, 270, 90);
        path.AddLine(r.Right, r.Y + d / 2, r.Right, r.Bottom);
        path.AddLine(r.Right, r.Bottom, r.X, r.Bottom);
        path.CloseFigure();
        return path;
    }

    private const int DwmwaUseImmersiveDarkMode = 20;

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);

    [DllImport("uxtheme.dll", CharSet = CharSet.Unicode)]
    private static extern int SetWindowTheme(IntPtr hwnd, string? appName, string? idList);
}

/// <summary>Pixel-sized fonts for the current DPI, rebuilt when the window moves to another monitor.</summary>
internal sealed class Fonts : IDisposable
{
    public Font Title { get; }
    public Font TileValue { get; }
    public Font Heading { get; }
    public Font Body { get; }
    public Font BodyStrong { get; }
    public Font Small { get; }

    public Fonts(float scale)
    {
        // Classic (non-variable) Segoe UI families render reliably through GDI.
        var regular = Pick("Segoe UI");
        var semibold = Pick("Segoe UI Semibold", "Segoe UI");
        Title = new Font(semibold, 22 * scale, FontStyle.Regular, GraphicsUnit.Pixel);
        TileValue = new Font(semibold, 26 * scale, FontStyle.Regular, GraphicsUnit.Pixel);
        Heading = new Font(semibold, 14 * scale, FontStyle.Regular, GraphicsUnit.Pixel);
        Body = new Font(regular, 13.5f * scale, FontStyle.Regular, GraphicsUnit.Pixel);
        BodyStrong = new Font(semibold, 13.5f * scale, FontStyle.Regular, GraphicsUnit.Pixel);
        Small = new Font(regular, 12 * scale, FontStyle.Regular, GraphicsUnit.Pixel);
    }

    private static string Pick(params string[] families)
    {
        using var installed = new System.Drawing.Text.InstalledFontCollection();
        foreach (var name in families)
        {
            if (installed.Families.Any(f => f.Name.Equals(name, StringComparison.OrdinalIgnoreCase)))
                return name;
        }
        return SystemFonts.MessageBoxFont?.FontFamily.Name ?? "Segoe UI";
    }

    public void Dispose()
    {
        Title.Dispose();
        TileValue.Dispose();
        Heading.Dispose();
        Body.Dispose();
        BodyStrong.Dispose();
        Small.Dispose();
    }
}
