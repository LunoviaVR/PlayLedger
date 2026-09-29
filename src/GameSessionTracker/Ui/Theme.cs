using System.Drawing.Drawing2D;
using System.Runtime.InteropServices;
using Microsoft.Win32;

namespace GameSessionTracker.Ui;

/// <summary>
/// Design tokens for the glass UI. Follows the Windows light/dark app setting; dark is the primary look.
/// Colours with alpha are painted with GDI+ over the atmospheric backdrop. Text colours are opaque because
/// GDI text (TextRenderer) ignores alpha.
/// </summary>
internal sealed class Theme
{
    public required bool IsDark { get; init; }

    // ---- Backdrop: a deep diagonal gradient with a few soft ambient glows behind the glass ----
    public required Color BackdropStart { get; init; }
    public required Color BackdropMid { get; init; }
    public required Color BackdropEnd { get; init; }
    public required Color[] Glows { get; init; }        // top-left, right, bottom
    public required Color Base { get; init; }           // solid stand-in for the backdrop (native controls, fallbacks)

    // ---- Glass levels ----
    public required Color GlassPanel { get; init; }     // level 1: main containers (chart, lists, settings cards)
    public required Color GlassCard { get; init; }      // level 2: stat tiles
    public required Color GlassControl { get; init; }   // level 3: interactive elements (inputs, secondary buttons, switcher)
    public required Color GlassHover { get; init; }     // level 4: hover
    public required Color GlassPressed { get; init; }
    public required Color GlassBorder { get; init; }
    public required Color GlassBorderStrong { get; init; }
    public required Color GlassHighlight { get; init; } // top-edge sheen
    public required int ShadowAlpha { get; init; }

    // ---- Opaque surfaces (things that must stay readable over anything) ----
    public required Color SurfaceStrong { get; init; }  // menus, modals
    public required Color InputSolid { get; init; }     // native text box fill inside the modal
    public required Color Tooltip { get; init; }
    public required Color Scrim { get; init; }          // darkened backdrop behind modals

    // ---- Text ----
    public required Color TextPrimary { get; init; }
    public required Color TextSecondary { get; init; }
    public required Color TextMuted { get; init; }
    public required Color TextOnAccent { get; init; }

    // ---- Accent ----
    public required Color Accent { get; init; }
    public required Color AccentSecondary { get; init; }
    public required Color AccentGradientStart { get; init; } // primary buttons, switch "on"
    public required Color AccentGradientEnd { get; init; }
    public required Color AccentSoft { get; init; }          // selected rows, active tab
    public required Color AccentBorder { get; init; }
    public required Color FocusRing { get; init; }

    // ---- Data ----
    public required Color Track { get; init; }          // empty part of bars
    public required Color Gridline { get; init; }
    public required Color Separator { get; init; }

    // ---- Status ----
    public required Color Success { get; init; }
    public required Color SuccessSoft { get; init; }
    public required Color SuccessText { get; init; }
    public required Color Warning { get; init; }
    public required Color WarningSoft { get; init; }
    public required Color WarningText { get; init; }
    public required Color Error { get; init; }
    public required Color ErrorSoft { get; init; }
    public required Color ErrorText { get; init; }
    public required Color InfoSoft { get; init; }
    public required Color InfoText { get; init; }
    public required Color NeutralSoft { get; init; }

    public static readonly Theme Dark = new()
    {
        IsDark = true,
        BackdropStart = Hex("#070b14"),
        BackdropMid = Hex("#0b1020"),
        BackdropEnd = Hex("#111827"),
        Glows = new[] { Rgba(59, 130, 246, 0.16), Rgba(129, 140, 248, 0.13), Rgba(167, 139, 250, 0.08) },
        Base = Hex("#0b1020"),

        GlassPanel = Rgba(15, 23, 42, 0.60),
        GlassCard = Rgba(255, 255, 255, 0.045),
        GlassControl = Rgba(255, 255, 255, 0.06),
        GlassHover = Rgba(255, 255, 255, 0.10),
        GlassPressed = Rgba(255, 255, 255, 0.14),
        GlassBorder = Rgba(255, 255, 255, 0.08),
        GlassBorderStrong = Rgba(255, 255, 255, 0.14),
        GlassHighlight = Rgba(255, 255, 255, 0.05),
        ShadowAlpha = 64, // ≈ rgba(0,0,0,.25)

        SurfaceStrong = Hex("#121a2c"),
        InputSolid = Hex("#182032"),
        Tooltip = Rgba(8, 12, 24, 0.94),
        Scrim = Color.Black,

        TextPrimary = Hex("#f1f3f7"),   // ≈ white .95
        TextSecondary = Hex("#a9afbc"), // ≈ white .65
        TextMuted = Hex("#7f8697"),     // ≈ white .42, nudged up for 4.5:1 on the panels
        TextOnAccent = Color.White,

        Accent = Hex("#60a5fa"),
        AccentSecondary = Hex("#818cf8"),
        AccentGradientStart = Hex("#3b82f6"),
        AccentGradientEnd = Hex("#6366f1"),
        AccentSoft = Rgba(96, 165, 250, 0.14),
        AccentBorder = Rgba(96, 165, 250, 0.45),
        FocusRing = Rgba(96, 165, 250, 0.70),

        Track = Rgba(255, 255, 255, 0.07),
        Gridline = Rgba(255, 255, 255, 0.06),
        Separator = Rgba(255, 255, 255, 0.06),

        Success = Hex("#34d399"),
        SuccessSoft = Rgba(52, 211, 153, 0.12),
        SuccessText = Hex("#6ee7b7"),
        Warning = Hex("#fbbf24"),
        WarningSoft = Rgba(251, 191, 36, 0.12),
        WarningText = Hex("#fcd34d"),
        Error = Hex("#f87171"),
        ErrorSoft = Rgba(248, 113, 113, 0.12),
        ErrorText = Hex("#fca5a5"),
        InfoSoft = Rgba(96, 165, 250, 0.12),
        InfoText = Hex("#93c5fd"),
        NeutralSoft = Rgba(255, 255, 255, 0.06),
    };

    public static readonly Theme Light = new()
    {
        IsDark = false,
        BackdropStart = Hex("#eef2fa"),
        BackdropMid = Hex("#e8edf7"),
        BackdropEnd = Hex("#e1e7f2"),
        Glows = new[] { Rgba(96, 165, 250, 0.22), Rgba(129, 140, 248, 0.18), Rgba(167, 139, 250, 0.12) },
        Base = Hex("#e8edf7"),

        GlassPanel = Rgba(255, 255, 255, 0.62),
        GlassCard = Rgba(255, 255, 255, 0.52),
        GlassControl = Rgba(15, 23, 42, 0.045),
        GlassHover = Rgba(15, 23, 42, 0.075),
        GlassPressed = Rgba(15, 23, 42, 0.11),
        GlassBorder = Rgba(15, 23, 42, 0.09),
        GlassBorderStrong = Rgba(15, 23, 42, 0.16),
        GlassHighlight = Rgba(255, 255, 255, 0.55),
        ShadowAlpha = 26,

        SurfaceStrong = Hex("#fbfcfe"),
        InputSolid = Hex("#f1f4f9"),
        Tooltip = Rgba(255, 255, 255, 0.97),
        Scrim = Hex("#0b1020"),

        TextPrimary = Hex("#0b1220"),
        TextSecondary = Hex("#475069"),
        TextMuted = Hex("#5f6679"),
        TextOnAccent = Color.White,

        Accent = Hex("#2563eb"),
        AccentSecondary = Hex("#6366f1"),
        AccentGradientStart = Hex("#3b82f6"),
        AccentGradientEnd = Hex("#6366f1"),
        AccentSoft = Rgba(37, 99, 235, 0.10),
        AccentBorder = Rgba(37, 99, 235, 0.40),
        FocusRing = Rgba(37, 99, 235, 0.65),

        Track = Rgba(15, 23, 42, 0.07),
        Gridline = Rgba(15, 23, 42, 0.07),
        Separator = Rgba(15, 23, 42, 0.07),

        Success = Hex("#059669"),
        SuccessSoft = Rgba(16, 185, 129, 0.14),
        SuccessText = Hex("#047857"),
        Warning = Hex("#d97706"),
        WarningSoft = Rgba(245, 158, 11, 0.14),
        WarningText = Hex("#b45309"),
        Error = Hex("#dc2626"),
        ErrorSoft = Rgba(239, 68, 68, 0.12),
        ErrorText = Hex("#b91c1c"),
        InfoSoft = Rgba(59, 130, 246, 0.12),
        InfoText = Hex("#1d4ed8"),
        NeutralSoft = Rgba(15, 23, 42, 0.06),
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

    private static Color Hex(string html) => ColorTranslator.FromHtml(html);
    private static Color Rgba(int r, int g, int b, double a) => Color.FromArgb((int)Math.Round(a * 255), r, g, b);

    /// <summary>Straight alpha-blend of <paramref name="top"/> over an opaque <paramref name="bottom"/>, for GDI text and native controls.</summary>
    public static Color Over(Color top, Color bottom)
    {
        var a = top.A / 255f;
        return Color.FromArgb(255,
            (int)Math.Round(top.R * a + bottom.R * (1 - a)),
            (int)Math.Round(top.G * a + bottom.G * (1 - a)),
            (int)Math.Round(top.B * a + bottom.B * (1 - a)));
    }

    public static Color WithAlpha(Color c, int alpha) => Color.FromArgb(Math.Clamp(alpha, 0, 255), c.R, c.G, c.B);

    /// <summary>Dark (or light) title bar and border that blend with the backdrop (colour tweaks are Windows 11 only).</summary>
    public static void ApplyWindowChrome(Form form, Theme theme)
    {
        try
        {
            var on = theme.IsDark ? 1 : 0;
            DwmSetWindowAttribute(form.Handle, DwmwaUseImmersiveDarkMode, ref on, sizeof(int));
            if (IsWindows11)
            {
                var caption = ColorRef(theme.BackdropStart);
                DwmSetWindowAttribute(form.Handle, DwmwaCaptionColor, ref caption, sizeof(int));
                var border = ColorRef(Over(theme.GlassBorderStrong, theme.BackdropStart));
                DwmSetWindowAttribute(form.Handle, DwmwaBorderColor, ref border, sizeof(int));
            }
        }
        catch
        {
            // Not supported on this Windows version.
        }
    }

    /// <summary>Rounded corners and a themed border for popups (menus); no-op before Windows 11.</summary>
    public static void ApplyPopupChrome(IntPtr handle, Theme theme)
    {
        if (!IsWindows11)
            return;
        try
        {
            var corners = DwmwcpRoundSmall;
            DwmSetWindowAttribute(handle, DwmwaWindowCornerPreference, ref corners, sizeof(int));
            var border = ColorRef(Over(theme.GlassBorderStrong, theme.SurfaceStrong));
            DwmSetWindowAttribute(handle, DwmwaBorderColor, ref border, sizeof(int));
        }
        catch
        {
            // Not supported on this Windows version.
        }
    }

    /// <summary>The modal scrim matches the main window's rounded corners and has no border of its own.</summary>
    public static void ApplyScrimChrome(IntPtr handle)
    {
        if (!IsWindows11)
            return;
        try
        {
            var corners = DwmwcpRound;
            DwmSetWindowAttribute(handle, DwmwaWindowCornerPreference, ref corners, sizeof(int));
            var none = DwmwaColorNone;
            DwmSetWindowAttribute(handle, DwmwaBorderColor, ref none, sizeof(int));
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

    public static bool IsWindows11 => Environment.OSVersion.Version.Build >= 22000;

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

    private static int ColorRef(Color c) => c.R | (c.G << 8) | (c.B << 16);

    private const int DwmwaUseImmersiveDarkMode = 20;
    private const int DwmwaWindowCornerPreference = 33;
    private const int DwmwaBorderColor = 34;
    private const int DwmwaCaptionColor = 35;
    private const int DwmwcpRound = 2;
    private const int DwmwcpRoundSmall = 3;
    private const int DwmwaColorNone = unchecked((int)0xFFFFFFFE);

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);

    [DllImport("uxtheme.dll", CharSet = CharSet.Unicode)]
    private static extern int SetWindowTheme(IntPtr hwnd, string? appName, string? idList);
}

/// <summary>Corner radii in logical pixels (multiply by the DPI scale).</summary>
internal static class Radius
{
    public const float Small = 8;    // small controls, row highlights
    public const float Control = 10; // buttons, switcher
    public const float Input = 12;   // inputs, steppers
    public const float Card = 16;    // cards, tiles
    public const float Panel = 20;   // modals
}

/// <summary>Motion tokens. Honours Windows' "Animation effects" setting (the reduced-motion switch).</summary>
internal static class Motion
{
    public const int DurationMs = 160;

    public static bool Reduced
    {
        get
        {
            try
            {
                var enabled = true;
                return SystemParametersInfo(SpiGetClientAreaAnimation, 0, ref enabled, 0) && !enabled;
            }
            catch
            {
                return false;
            }
        }
    }

    private const int SpiGetClientAreaAnimation = 0x1042;

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SystemParametersInfo(int action, int param, [MarshalAs(UnmanagedType.Bool)] ref bool value, int winIni);
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
        Title = new Font(semibold, 28 * scale, FontStyle.Regular, GraphicsUnit.Pixel);
        TileValue = new Font(semibold, 26 * scale, FontStyle.Regular, GraphicsUnit.Pixel);
        Heading = new Font(semibold, 15 * scale, FontStyle.Regular, GraphicsUnit.Pixel);
        Body = new Font(regular, 14 * scale, FontStyle.Regular, GraphicsUnit.Pixel);
        BodyStrong = new Font(semibold, 14 * scale, FontStyle.Regular, GraphicsUnit.Pixel);
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
