using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;

namespace GameSessionTracker.Ui;

/// <summary>Glass levels from the design system (see <see cref="Theme"/>).</summary>
internal enum GlassLevel
{
    Panel,   // level 1: main containers
    Card,    // level 2: stat tiles
    Control, // level 3: interactive elements
}

/// <summary>A control painted as a glass surface; its container draws a soft shadow around it.</summary>
internal interface IGlassSurface
{
    /// <summary>Corner radius in logical pixels.</summary>
    float CornerRadius { get; }
}

/// <summary>Shared painting for glass surfaces, shadows and focus rings, so every control looks the same.</summary>
internal static class Glass
{
    public static Color Fill(Theme t, GlassLevel level) => level switch
    {
        GlassLevel.Panel => t.GlassPanel,
        GlassLevel.Card => t.GlassCard,
        _ => t.GlassControl,
    };

    /// <summary>Translucent fill, a faint top sheen, and a border that is brighter along the top edge (light from above).</summary>
    public static void PaintSurface(Graphics g, RectangleF rect, float radius, Color fill, Theme t, bool sheen = true)
    {
        if (rect.Width <= 1 || rect.Height <= 1)
            return;
        var smoothing = g.SmoothingMode;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        using var path = Theme.RoundedRect(rect, radius);
        using (var brush = new SolidBrush(fill))
            g.FillPath(brush, path);

        if (sheen)
        {
            var sheenRect = new RectangleF(rect.X, rect.Y, rect.Width, Math.Max(2, Math.Min(rect.Height * 0.5f, radius * 5)));
            using var sheenBrush = new LinearGradientBrush(sheenRect, t.GlassHighlight, Theme.WithAlpha(t.GlassHighlight, 0), LinearGradientMode.Vertical);
            sheenRect.Height -= 1; // keep the gradient's wrap-around row out of the fill
            var state = g.Save();
            g.SetClip(path, CombineMode.Intersect);
            g.FillRectangle(sheenBrush, sheenRect);
            g.Restore(state);
        }

        PaintBorder(g, path, rect, t.GlassBorderStrong, t.GlassBorder);
        g.SmoothingMode = smoothing;
    }

    public static void PaintBorder(Graphics g, GraphicsPath path, RectangleF rect, Color top, Color bottom)
    {
        var gradientRect = new RectangleF(rect.X, rect.Y - 1, rect.Width, rect.Height + 2);
        using var brush = new LinearGradientBrush(gradientRect, top, bottom, LinearGradientMode.Vertical);
        using var pen = new Pen(brush, 1);
        g.DrawPath(pen, path);
    }

    /// <summary>Soft drop shadow (≈ 0 8px 32px) drawn only outside the surface, like CSS box-shadow.</summary>
    public static void PaintShadow(Graphics g, RectangleF rect, float radius, Theme t, float scale)
    {
        if (t.ShadowAlpha <= 0 || rect.Width <= 0 || rect.Height <= 0)
            return;
        var state = g.Save();
        g.SmoothingMode = SmoothingMode.AntiAlias;
        using (var surface = Theme.RoundedRect(rect, radius))
            g.SetClip(surface, CombineMode.Exclude);
        const int layers = 8;
        var offsetY = 6 * scale;
        for (var i = layers; i >= 1; i--)
        {
            var spread = i * 2.2f * scale;
            var r = new RectangleF(rect.X - spread + 2 * scale, rect.Y - spread + offsetY + 2 * scale,
                rect.Width + spread * 2 - 4 * scale, rect.Height + spread * 2 - 4 * scale);
            // Alphas accumulate towards the surface, fading out at the edge of the blur.
            var alpha = (int)Math.Round(t.ShadowAlpha / (float)layers * 1.1f);
            using var path = Theme.RoundedRect(r, radius + spread);
            using var brush = new SolidBrush(Color.FromArgb(alpha, 0, 0, 0));
            g.FillPath(brush, path);
        }
        g.Restore(state);
    }

    /// <summary>Paints shadows under every visible glass child of <paramref name="container"/> that intersects the clip.</summary>
    public static void PaintChildShadows(Graphics g, Control container, Rectangle clip, Theme t)
    {
        var scale = container.DeviceDpi / 96f;
        var reach = (int)(24 * scale);
        foreach (Control child in container.Controls)
        {
            if (!child.Visible || child is not IGlassSurface surface)
                continue;
            var bounds = child.Bounds;
            if (!Rectangle.Inflate(bounds, reach, reach).IntersectsWith(clip))
                continue;
            PaintShadow(g, new RectangleF(bounds.X + 0.5f, bounds.Y + 0.5f, bounds.Width - 1.5f, bounds.Height - 1.5f), surface.CornerRadius * scale, t, scale);
        }
    }

    /// <summary>Keyboard focus ring hugging <paramref name="body"/>; controls reserve <see cref="FocusMargin"/> around their body for it.</summary>
    public static void PaintFocusRing(Graphics g, RectangleF body, float radius, Theme t, float scale)
    {
        var smoothing = g.SmoothingMode;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        var width = 2 * scale;
        var ring = RectangleF.Inflate(body, width * 0.5f + scale * 0.5f, width * 0.5f + scale * 0.5f);
        using var path = Theme.RoundedRect(ring, radius + width * 0.5f);
        using var pen = new Pen(t.FocusRing, width);
        g.DrawPath(pen, path);
        g.SmoothingMode = smoothing;
    }

    /// <summary>Logical pixels kept free around focusable controls for the focus ring.</summary>
    public const float FocusMargin = 3;

    /// <summary>A translucent pill with a coloured label (status badges).</summary>
    public static Size MeasureBadge(Graphics g, string text, Font font, int height, float scale, bool dot)
    {
        var textWidth = TextRenderer.MeasureText(g, text, font, Size.Empty, BadgeFlags).Width + (int)(2 * scale);
        return new Size(textWidth + (int)(10 * scale) * 2 + (dot ? (int)(14 * scale) : 0), height);
    }

    private const TextFormatFlags BadgeFlags = TextFormatFlags.NoPadding | TextFormatFlags.NoPrefix | TextFormatFlags.SingleLine;

    /// <summary>Paints a badge into <paramref name="rect"/> (see <see cref="MeasureBadge"/>); long text is ellipsized.</summary>
    public static void PaintBadge(Graphics g, string text, Font font, Color background, Color foreground, Rectangle rect, float scale, Color? dot = null)
    {
        const TextFormatFlags flags = BadgeFlags;
        var height = rect.Height;
        var padX = (int)(10 * scale);
        var dotSize = (int)(7 * scale);
        var dotSpace = dot is null ? 0 : (int)(14 * scale);
        var smoothing = g.SmoothingMode;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        using (var path = Theme.RoundedRect(new RectangleF(rect.X, rect.Y, rect.Width, rect.Height), height / 2f))
        using (var brush = new SolidBrush(background))
            g.FillPath(brush, path);
        if (dot is { } dotColor)
        {
            using var brush = new SolidBrush(dotColor);
            g.FillEllipse(brush, rect.X + padX, rect.Y + (height - dotSize) / 2f, dotSize, dotSize);
        }
        g.SmoothingMode = smoothing;
        TextRenderer.DrawText(g, text, font, new Rectangle(rect.X + padX + dotSpace, rect.Y, Math.Max(0, rect.Width - padX * 2 - dotSpace), height), foreground,
            flags | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis);
    }

    public static Color Lerp(Color a, Color b, float t)
    {
        t = Math.Clamp(t, 0, 1);
        return Color.FromArgb(
            (int)Math.Round(a.A + (b.A - a.A) * t),
            (int)Math.Round(a.R + (b.R - a.R) * t),
            (int)Math.Round(a.G + (b.G - a.G) * t),
            (int)Math.Round(a.B + (b.B - a.B) * t));
    }
}

/// <summary>
/// The atmospheric window background: a deep diagonal gradient with blurred ambient glows. Rendered once per size
/// and theme, then copied; glass surfaces are painted over the part of it that sits behind them.
/// </summary>
internal sealed class Backdrop : IDisposable
{
    private Bitmap? _cache;
    private Theme? _theme;

    public void Paint(Graphics g, Rectangle clip, Size size, Theme theme)
    {
        if (size.Width <= 0 || size.Height <= 0)
            return;
        if (_cache is null || _cache.Size != size || !ReferenceEquals(_theme, theme))
        {
            _cache?.Dispose();
            _cache = Render(size, theme);
            _theme = theme;
        }
        var area = Rectangle.Intersect(clip, new Rectangle(Point.Empty, size));
        if (area.IsEmpty)
            return;
        var state = g.Save();
        g.CompositingMode = CompositingMode.SourceCopy;
        g.InterpolationMode = InterpolationMode.NearestNeighbor;
        g.PixelOffsetMode = PixelOffsetMode.Half;
        g.DrawImage(_cache, area, area, GraphicsUnit.Pixel);
        g.Restore(state);
    }

    public void Dispose()
    {
        _cache?.Dispose();
        _cache = null;
    }

    private static Bitmap Render(Size size, Theme t)
    {
        var bmp = new Bitmap(size.Width, size.Height, PixelFormat.Format32bppPArgb);
        using (var g = Graphics.FromImage(bmp))
        {
            var rect = new Rectangle(Point.Empty, size);
            // 135deg: top-left to bottom-right.
            using (var brush = new LinearGradientBrush(new Rectangle(-1, -1, size.Width + 2, size.Height + 2), t.BackdropStart, t.BackdropEnd, 45f))
            {
                brush.InterpolationColors = new ColorBlend
                {
                    Colors = new[] { t.BackdropStart, t.BackdropMid, t.BackdropEnd },
                    Positions = new[] { 0f, 0.45f, 1f },
                };
                g.FillRectangle(brush, rect);
            }

            g.SmoothingMode = SmoothingMode.AntiAlias;
            var w = size.Width;
            var h = size.Height;
            var span = Math.Max(w, h);
            Glow(g, t.Glows[0], new PointF(w * 0.10f, h * 0.02f), span * 0.55f, span * 0.42f);
            Glow(g, t.Glows[1], new PointF(w * 0.95f, h * 0.32f), span * 0.45f, span * 0.40f);
            Glow(g, t.Glows[2], new PointF(w * 0.42f, h * 1.02f), span * 0.50f, span * 0.30f);
        }
        Dither(bmp);
        return bmp;
    }

    private static void Glow(Graphics g, Color color, PointF center, float rx, float ry)
    {
        using var path = new GraphicsPath();
        path.AddEllipse(center.X - rx, center.Y - ry, rx * 2, ry * 2);
        using var brush = new PathGradientBrush(path)
        {
            CenterPoint = center,
            CenterColor = color,
            SurroundColors = new[] { Theme.WithAlpha(color, 0) },
        };
        // Smooth (gaussian-like) fall-off so the glow has no visible edge.
        const int steps = 12;
        var positions = new float[steps + 1];
        var factors = new float[steps + 1];
        for (var i = 0; i <= steps; i++)
        {
            var p = i / (float)steps;
            positions[i] = p;
            factors[i] = p * p * (3 - 2 * p) * p;
        }
        brush.Blend = new Blend { Positions = positions, Factors = factors };
        g.FillPath(brush, path);
    }

    /// <summary>±1 of noise per channel, which hides the banding dark gradients otherwise show.</summary>
    private static void Dither(Bitmap bmp)
    {
        var rect = new Rectangle(Point.Empty, bmp.Size);
        var data = bmp.LockBits(rect, ImageLockMode.ReadWrite, PixelFormat.Format32bppPArgb);
        try
        {
            var bytes = new byte[data.Stride * data.Height];
            Marshal.Copy(data.Scan0, bytes, 0, bytes.Length);
            uint state = 2463534242;
            for (var i = 0; i < bytes.Length; i += 4)
            {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                var n = (int)(state % 3) - 1;
                bytes[i] = (byte)Math.Clamp(bytes[i] + n, 0, 255);
                bytes[i + 1] = (byte)Math.Clamp(bytes[i + 1] + n, 0, 255);
                bytes[i + 2] = (byte)Math.Clamp(bytes[i + 2] + n, 0, 255);
            }
            Marshal.Copy(bytes, 0, data.Scan0, bytes.Length);
        }
        finally
        {
            bmp.UnlockBits(data);
        }
    }
}

/// <summary>Eases a value towards a target over <see cref="Motion.DurationMs"/>; jumps straight there when motion is reduced.</summary>
internal sealed class Tween : IDisposable
{
    private readonly Control _owner;
    private System.Windows.Forms.Timer? _timer;
    private float _from;
    private float _to;
    private long _start;

    public Tween(Control owner, float initial = 0)
    {
        _owner = owner;
        Value = _to = initial;
    }

    public float Value { get; private set; }

    public void To(float target)
    {
        if (Math.Abs(target - _to) < 0.0001f)
            return; // already there, or already heading there
        if (!_owner.IsHandleCreated || !_owner.Visible || Motion.Reduced)
        {
            Jump(target);
            return;
        }
        _from = Value;
        _to = target;
        _start = Environment.TickCount64;
        _timer ??= CreateTimer();
        _timer.Start();
    }

    public void Jump(float value)
    {
        _timer?.Stop();
        Value = _to = value;
        _owner.Invalidate();
    }

    private System.Windows.Forms.Timer CreateTimer()
    {
        var timer = new System.Windows.Forms.Timer { Interval = 15 };
        timer.Tick += (_, _) =>
        {
            var p = Math.Clamp((Environment.TickCount64 - _start) / (float)Motion.DurationMs, 0, 1);
            var eased = 1 - (1 - p) * (1 - p) * (1 - p); // ease-out cubic
            Value = _from + (_to - _from) * eased;
            if (p >= 1)
                timer.Stop();
            _owner.Invalidate();
        };
        return timer;
    }

    public void Dispose() => _timer?.Dispose();
}

/// <summary>Darkens the owner window while a modal dialog or message box is open.</summary>
internal static class ModalScrim
{
    public static T Show<T>(IWin32Window? owner, Theme theme, Func<T> show)
    {
        if (owner is not Form form || !form.Visible || form.WindowState == FormWindowState.Minimized)
            return show();
        using var scrim = new ScrimForm(theme) { Bounds = form.Bounds };
        scrim.Show(form);
        try
        {
            return show();
        }
        finally
        {
            scrim.Close();
        }
    }

    private sealed class ScrimForm : Form
    {
        public ScrimForm(Theme theme)
        {
            FormBorderStyle = FormBorderStyle.None;
            ShowInTaskbar = false;
            StartPosition = FormStartPosition.Manual;
            BackColor = theme.Scrim;
            Opacity = theme.IsDark ? 0.45 : 0.28;
        }

        protected override bool ShowWithoutActivation => true;

        protected override void OnHandleCreated(EventArgs e)
        {
            base.OnHandleCreated(e);
            Theme.ApplyScrimChrome(Handle);
        }
    }
}
