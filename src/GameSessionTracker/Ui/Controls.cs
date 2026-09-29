using System.Drawing.Drawing2D;

namespace GameSessionTracker.Ui;

/// <summary>Base for the dashboard's custom-painted pieces.</summary>
internal abstract class PaintedControl : Control
{
    protected PaintedControl()
    {
        SetStyle(ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer |
                 ControlStyles.ResizeRedraw | ControlStyles.UserPaint | ControlStyles.SupportsTransparentBackColor, true);
        // Transparent: WinForms paints the parent (and ultimately the window backdrop) behind us, which is what the glass shows through.
        BackColor = Color.Transparent;
    }

    public Theme Theme { get; set; } = Theme.Light;
    public Fonts? Fonts { get; set; }

    protected float UiScale => DeviceDpi / 96f;
    protected int S(float logical) => (int)Math.Round(logical * UiScale);

    /// <summary>Paints this control's whole area as a glass surface.</summary>
    protected void PaintCard(Graphics g, GlassLevel level = GlassLevel.Panel, float radius = Radius.Card)
    {
        g.SmoothingMode = SmoothingMode.AntiAlias;
        Glass.PaintSurface(g, new RectangleF(0.5f, 0.5f, Width - 1.5f, Height - 1.5f), S(radius), Glass.Fill(Theme, level), Theme);
    }

    /// <summary>The area inside the focus-ring margin, where focusable controls draw their body.</summary>
    protected RectangleF BodyRect
    {
        get
        {
            var m = Glass.FocusMargin * UiScale;
            return new RectangleF(m + 0.5f, m + 0.5f, Width - 2 * m - 1.5f, Height - 2 * m - 1.5f);
        }
    }

    protected bool ShowFocusRing => Focused && ShowFocusCues;

    protected static void DrawLabel(Graphics g, string text, Font font, Color color, Rectangle bounds, TextFormatFlags flags = TextFormatFlags.Left) =>
        TextRenderer.DrawText(g, text, font, bounds, color,
            flags | TextFormatFlags.NoPrefix | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine | TextFormatFlags.NoPadding);
}

/// <summary>A rounded glass card with a title; child controls sit below the title.</summary>
internal sealed class Card : Panel, IGlassSurface
{
    public Card()
    {
        SetStyle(ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw |
                 ControlStyles.UserPaint | ControlStyles.SupportsTransparentBackColor, true);
        BackColor = Color.Transparent;
    }

    public float CornerRadius => Radius.Card;

    public Theme Theme { get; set; } = Theme.Light;
    public Fonts? Fonts { get; set; }
    public string Title { get; set; } = "";
    public string Detail { get; set; } = "";

    public int HeaderHeight => (int)Math.Round(52 * DeviceDpi / 96f);

    public void ApplyPadding()
    {
        var s = DeviceDpi / 96f;
        Padding = new Padding((int)(8 * s), HeaderHeight, (int)(8 * s), (int)(8 * s));
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var s = DeviceDpi / 96f;
        var g = e.Graphics;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        Glass.PaintSurface(g, new RectangleF(0.5f, 0.5f, Width - 1.5f, Height - 1.5f), CornerRadius * s, Theme.GlassPanel, Theme);
        if (Fonts is null)
            return;
        var header = new Rectangle((int)(20 * s), (int)(2 * s), Width - (int)(40 * s), HeaderHeight);
        const TextFormatFlags flags = TextFormatFlags.VerticalCenter | TextFormatFlags.NoPrefix | TextFormatFlags.SingleLine;
        var detailWidth = TextRenderer.MeasureText(g, Detail, Fonts.Small, Size.Empty, flags | TextFormatFlags.NoPadding).Width + (int)(8 * s);
        TextRenderer.DrawText(g, Title, Fonts.Heading, header with { Width = Math.Max(0, header.Width - detailWidth - (int)(12 * s)) }, Theme.TextPrimary,
            flags | TextFormatFlags.Left | TextFormatFlags.EndEllipsis);
        TextRenderer.DrawText(g, Detail, Fonts.Small, header, Theme.TextMuted, flags | TextFormatFlags.Right);
    }
}

/// <summary>Label, big value, and a short caption.</summary>
internal sealed class StatTile : PaintedControl, IGlassSurface
{
    public float CornerRadius => Radius.Card;

    public string Label { get; set; } = "";
    public string Value { get; set; } = "";
    public string Caption { get; set; } = "";

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        PaintCard(g, GlassLevel.Card);
        if (Fonts is null)
            return;
        var x = S(20);
        var w = Width - S(40);
        DrawLabel(g, Label, Fonts.Small, Theme.TextSecondary, new Rectangle(x, S(16), w, S(18)));
        DrawLabel(g, Value, Fonts.TileValue, Theme.TextPrimary, new Rectangle(x, S(36), w, S(36)));
        DrawLabel(g, Caption, Fonts.Small, Theme.TextMuted, new Rectangle(x, S(74), w, S(18)));
    }
}

/// <summary>Header: page title on the left, what's playing now and the page switcher on the right.</summary>
internal sealed class HeaderBar : PaintedControl
{
    private readonly List<Rectangle> _tabBounds = new();
    private int _hotTab = -1;

    public HeaderBar()
    {
        SetStyle(ControlStyles.Selectable, true);
        TabStop = true;
    }

    public string Title { get; set; } = "";
    public string Status { get; set; } = "";
    public bool IsLive { get; set; }
    public string[] Tabs { get; set; } = Array.Empty<string>();
    public int SelectedTab { get; set; }

    public event Action<int>? TabClicked;

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        if (Fonts is null)
            return;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        const TextFormatFlags measure = TextFormatFlags.NoPadding | TextFormatFlags.NoPrefix | TextFormatFlags.SingleLine;

        // Segmented page switcher, right-aligned: a quiet glass track with the active page tinted in the accent.
        _tabBounds.Clear();
        var tabHeight = S(38);
        var inset = S(4);
        var widths = Tabs.Select(t => TextRenderer.MeasureText(g, t, Fonts.BodyStrong, Size.Empty, measure).Width + S(32)).ToList();
        var total = widths.Sum() + inset * 2;
        var outer = new Rectangle(Width - total - S(Glass.FocusMargin), (Height - tabHeight) / 2, total, tabHeight);
        Glass.PaintSurface(g, new RectangleF(outer.X + 0.5f, outer.Y + 0.5f, outer.Width - 1, outer.Height - 1), S(Radius.Input), Theme.GlassControl, Theme, sheen: false);
        var x = outer.X + inset;
        for (var i = 0; i < Tabs.Length; i++)
        {
            var r = new Rectangle(x, outer.Y + inset, widths[i], tabHeight - inset * 2);
            _tabBounds.Add(r);
            var rf = new RectangleF(r.X + 0.5f, r.Y + 0.5f, r.Width - 1, r.Height - 1);
            if (i == SelectedTab)
            {
                using var path = Theme.RoundedRect(rf, S(Radius.Small));
                using (var fill = new SolidBrush(Theme.AccentSoft))
                    g.FillPath(fill, path);
                Glass.PaintBorder(g, path, rf, Theme.AccentBorder, Theme.WithAlpha(Theme.AccentBorder, Theme.AccentBorder.A / 3));
                if (ShowFocusRing)
                    Glass.PaintFocusRing(g, rf, S(Radius.Small), Theme, UiScale);
            }
            else if (i == _hotTab)
            {
                using var path = Theme.RoundedRect(rf, S(Radius.Small));
                using var fill = new SolidBrush(Theme.GlassHover);
                g.FillPath(fill, path);
            }
            var color = i == SelectedTab ? (Theme.IsDark ? Theme.InfoText : Theme.Accent) : i == _hotTab ? Theme.TextPrimary : Theme.TextSecondary;
            DrawLabel(g, Tabs[i], i == SelectedTab ? Fonts.BodyStrong : Fonts.Body, color, r, TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
            x += widths[i];
        }

        var titleSize = TextRenderer.MeasureText(g, Title, Fonts.Title, Size.Empty, measure);
        var statusRight = outer.X - S(16);
        var titleWidth = Math.Min(titleSize.Width + S(4), Math.Max(S(120), statusRight - S(200)));
        DrawLabel(g, Title, Fonts.Title, Theme.TextPrimary, new Rectangle(0, 0, titleWidth, Height), TextFormatFlags.VerticalCenter);

        // Live status as a badge just left of the switcher. The dot is decoration beside the text, never the only signal.
        var badgeHeight = S(30);
        var statusLeftLimit = titleWidth + S(24);
        var wanted = Glass.MeasureBadge(g, Status, Fonts.Body, badgeHeight, UiScale, dot: true);
        var badgeWidth = Math.Min(wanted.Width, statusRight - statusLeftLimit);
        if (badgeWidth > S(96))
        {
            var badge = new Rectangle(statusRight - badgeWidth, (Height - badgeHeight) / 2, badgeWidth, badgeHeight);
            if (IsLive)
                Glass.PaintBadge(g, Status, Fonts.Body, Theme.SuccessSoft, Theme.SuccessText, badge, UiScale, Theme.Success);
            else
                Glass.PaintBadge(g, Status, Fonts.Body, Theme.NeutralSoft, Theme.TextSecondary, badge, UiScale, Theme.TextMuted);
        }
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);
        var hot = _tabBounds.FindIndex(r => r.Contains(e.Location));
        if (hot == _hotTab)
            return;
        _hotTab = hot;
        Cursor = hot >= 0 ? Cursors.Hand : Cursors.Default;
        Invalidate();
    }

    protected override void OnMouseLeave(EventArgs e)
    {
        base.OnMouseLeave(e);
        _hotTab = -1;
        Invalidate();
    }

    protected override void OnMouseClick(MouseEventArgs e)
    {
        base.OnMouseClick(e);
        var index = _tabBounds.FindIndex(r => r.Contains(e.Location));
        if (e.Button == MouseButtons.Left && index >= 0)
            TabClicked?.Invoke(index);
    }

    // Keyboard: Tab to the switcher, then Left/Right to change page.
    protected override bool IsInputKey(Keys keyData) => keyData is Keys.Left or Keys.Right || base.IsInputKey(keyData);

    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        if (Tabs.Length == 0)
            return;
        var next = e.KeyCode switch
        {
            Keys.Left => Math.Max(0, SelectedTab - 1),
            Keys.Right => Math.Min(Tabs.Length - 1, SelectedTab + 1),
            _ => SelectedTab,
        };
        if (next != SelectedTab)
            TabClicked?.Invoke(next);
    }

    protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
    protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }
}

/// <summary>A muted note with one link after it.</summary>
internal sealed class FooterBar : PaintedControl
{
    private Rectangle _linkBounds;
    private bool _linkHot;

    public string Message { get; set; } = "";
    public string LinkText { get; set; } = "";
    public event Action? LinkClicked;

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        if (Fonts is null)
            return;
        const TextFormatFlags measure = TextFormatFlags.NoPrefix | TextFormatFlags.SingleLine | TextFormatFlags.NoPadding;
        var messageWidth = TextRenderer.MeasureText(g, Message, Fonts.Small, Size.Empty, measure).Width;
        var linkSize = TextRenderer.MeasureText(g, LinkText, Fonts.Small, Size.Empty, measure);
        DrawLabel(g, Message, Fonts.Small, Theme.TextMuted, new Rectangle(0, 0, messageWidth + S(4), Height), TextFormatFlags.VerticalCenter);
        _linkBounds = new Rectangle(messageWidth + S(16), (Height - linkSize.Height) / 2, linkSize.Width + S(4), linkSize.Height);
        using var font = _linkHot ? new Font(Fonts.Small, FontStyle.Underline) : null;
        DrawLabel(g, LinkText, font ?? Fonts.Small, Theme.IsDark ? Theme.InfoText : Theme.Accent, _linkBounds, TextFormatFlags.VerticalCenter);
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);
        var hot = _linkBounds.Contains(e.Location);
        if (hot == _linkHot)
            return;
        _linkHot = hot;
        Cursor = hot ? Cursors.Hand : Cursors.Default;
        Invalidate();
    }

    protected override void OnMouseLeave(EventArgs e)
    {
        base.OnMouseLeave(e);
        _linkHot = false;
        Invalidate();
    }

    protected override void OnMouseClick(MouseEventArgs e)
    {
        base.OnMouseClick(e);
        if (e.Button == MouseButtons.Left && _linkBounds.Contains(e.Location))
            LinkClicked?.Invoke();
    }
}

/// <summary>Bar chart of playtime per day, with a hover tooltip per bar.</summary>
internal sealed class DailyChart : PaintedControl, IGlassSurface
{
    public float CornerRadius => Radius.Card;

    private IReadOnlyList<DayTotal> _days = Array.Empty<DayTotal>();
    private int _hover = -1;

    public string Title { get; set; } = "";
    public string Detail { get; set; } = "";

    public IReadOnlyList<DayTotal> Days
    {
        get => _days;
        set
        {
            _days = value;
            Invalidate();
        }
    }

    private Rectangle PlotArea
    {
        get
        {
            var left = S(20) + S(40);
            var top = S(60);
            return new Rectangle(left, top, Math.Max(10, Width - left - S(20)), Math.Max(10, Height - top - S(34)));
        }
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);
        var plot = PlotArea;
        var index = -1;
        if (_days.Count > 0 && e.X >= plot.Left && e.X < plot.Right && e.Y >= plot.Top - S(8) && e.Y <= plot.Bottom + S(24))
            index = Math.Clamp((int)((e.X - plot.Left) / (plot.Width / (float)_days.Count)), 0, _days.Count - 1);
        if (index != _hover)
        {
            _hover = index;
            Invalidate();
        }
    }

    protected override void OnMouseLeave(EventArgs e)
    {
        base.OnMouseLeave(e);
        if (_hover != -1)
        {
            _hover = -1;
            Invalidate();
        }
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        PaintCard(g);
        if (Fonts is null)
            return;

        var headerRect = new Rectangle(S(20), S(2), Width - S(40), S(52));
        DrawLabel(g, Title, Fonts.Heading, Theme.TextPrimary, headerRect, TextFormatFlags.VerticalCenter);
        DrawLabel(g, Detail, Fonts.Small, Theme.TextMuted, headerRect, TextFormatFlags.VerticalCenter | TextFormatFlags.Right);

        var plot = PlotArea;
        if (_days.Count == 0 || _days.All(d => d.Total <= TimeSpan.Zero))
        {
            // Empty state: keep the baseline so the card doesn't look broken, with the message centred above it.
            using (var baseline = new Pen(Theme.Gridline, 1))
                g.DrawLine(baseline, plot.Left, plot.Bottom, plot.Right, plot.Bottom);
            DrawLabel(g, "No playtime in the last 30 days", Fonts.Body, Theme.TextSecondary, plot, TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
            return;
        }

        var (stepMinutes, axisMinutes) = NiceAxis(_days.Max(d => d.Total.TotalMinutes));

        // Recessive gridlines + y labels.
        using (var gridPen = new Pen(Theme.Gridline, 1))
        {
            for (var i = 0; i <= 3; i++)
            {
                var y = plot.Bottom - (int)Math.Round(plot.Height * i / 3.0);
                g.SmoothingMode = SmoothingMode.None;
                g.DrawLine(gridPen, plot.Left, y, plot.Right, y);
                DrawLabel(g, AxisLabel(stepMinutes * i), Fonts.Small, Theme.TextMuted,
                    new Rectangle(S(12), y - S(9), plot.Left - S(20), S(18)), TextFormatFlags.Right | TextFormatFlags.VerticalCenter);
            }
        }

        var slot = plot.Width / (float)_days.Count;
        var barWidth = Math.Max(2f, slot - Math.Max(S(2), slot * 0.3f));
        var radius = Math.Min(S(4), barWidth / 2f);
        g.SmoothingMode = SmoothingMode.AntiAlias;

        if (_hover >= 0)
        {
            var column = new RectangleF(plot.Left + slot * _hover, plot.Top - S(6), slot, plot.Height + S(6));
            using var path = Theme.RoundedRect(column, Math.Min(S(6), slot / 2f));
            using var hoverBrush = new SolidBrush(Theme.GlassControl);
            g.FillPath(hoverBrush, path);
        }

        // Bars share one vertical gradient (accent at the baseline, secondary accent at the top), so taller days read warmer.
        var gradientRect = new RectangleF(plot.Left, plot.Top - 1, plot.Width, plot.Height + 2);
        using (var barBrush = new LinearGradientBrush(gradientRect, Theme.AccentSecondary, Theme.Accent, LinearGradientMode.Vertical))
        using (var hotBrush = new SolidBrush(Color.FromArgb(Theme.IsDark ? 46 : 36, 255, 255, 255)))
        {
            for (var i = 0; i < _days.Count; i++)
            {
                var minutes = _days[i].Total.TotalMinutes;
                if (minutes <= 0)
                    continue;
                var h = Math.Max(S(2), (float)(plot.Height * minutes / axisMinutes));
                var x = plot.Left + slot * i + (slot - barWidth) / 2f;
                using var path = Theme.TopRoundedRect(new RectangleF(x, plot.Bottom - h, barWidth, h), radius);
                g.FillPath(barBrush, path);
                if (i == _hover)
                    g.FillPath(hotBrush, path);
            }
        }

        // X labels: a few dates, ending with today.
        for (var i = _days.Count - 1; i >= 0; i -= 7)
        {
            var label = i == _days.Count - 1 ? "Today" : _days[i].Day.ToString("MMM d");
            var cx = (int)(plot.Left + slot * (i + 0.5f));
            DrawLabel(g, label, Fonts.Small, Theme.TextMuted, new Rectangle(cx - S(40), plot.Bottom + S(8), S(80), S(18)), TextFormatFlags.HorizontalCenter);
        }

        if (_hover >= 0)
            DrawTooltip(g, plot, slot);
    }

    private void DrawTooltip(Graphics g, Rectangle plot, float slot)
    {
        var day = _days[_hover];
        var line1 = day.Day.ToString("dddd, MMM d");
        var line2 = day.Total > TimeSpan.Zero ? ReportWriter.FormatDuration(day.Total) : "No play";
        var flags = TextFormatFlags.NoPadding | TextFormatFlags.NoPrefix;
        var size1 = TextRenderer.MeasureText(g, line1, Fonts!.Small, Size.Empty, flags);
        var size2 = TextRenderer.MeasureText(g, line2, Fonts.BodyStrong, Size.Empty, flags);
        var w = Math.Max(size1.Width, size2.Width) + S(24);
        var h = size1.Height + size2.Height + S(20);

        var cx = plot.Left + slot * (_hover + 0.5f);
        var x = cx + S(12);
        if (x + w > Width - S(8))
            x = cx - S(12) - w;
        var y = plot.Top;

        // Tooltips use a near-opaque surface so the bars behind never compete with the text.
        var box = new RectangleF(x, y, w, h);
        Glass.PaintShadow(g, box, S(Radius.Small), Theme, UiScale * 0.5f);
        Glass.PaintSurface(g, box, S(Radius.Small), Theme.Tooltip, Theme, sheen: false);
        TextRenderer.DrawText(g, line1, Fonts.Small, new Point((int)x + S(12), (int)y + S(9)), Theme.TextSecondary, flags);
        TextRenderer.DrawText(g, line2, Fonts.BodyStrong, new Point((int)x + S(12), (int)y + S(11) + size1.Height), Theme.TextPrimary, flags);
    }

    private static (int Step, int Max) NiceAxis(double maxMinutes)
    {
        foreach (var step in new[] { 10, 20, 30, 60, 90, 120, 180, 240, 300, 360, 480 })
        {
            if (step * 3 >= maxMinutes)
                return (step, step * 3);
        }
        var big = (int)Math.Ceiling(maxMinutes / 3 / 60) * 60;
        return (big, big * 3);
    }

    private static string AxisLabel(int minutes) =>
        minutes == 0 ? "0" : minutes % 60 == 0 ? $"{minutes / 60}h" : minutes < 60 ? $"{minutes}m" : $"{minutes / 60}h {minutes % 60}m";
}

/// <summary>Sends mouse-wheel messages on to the nearest scrollable ancestor (scroll chaining).</summary>
internal static class WheelForwarding
{
    public const int WmMouseWheel = 0x020A;

    public static bool TryForward(Control control, ref Message m)
    {
        for (var parent = control.Parent; parent is not null; parent = parent.Parent)
        {
            if (parent is ScrollableControl { AutoScroll: true } && parent.IsHandleCreated)
            {
                SendMessage(parent.Handle, m.Msg, m.WParam, m.LParam);
                m.Result = IntPtr.Zero;
                return true;
            }
        }
        return false;
    }

    public static int Delta(Message m) => (short)((m.WParam.ToInt64() >> 16) & 0xFFFF);

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern IntPtr SendMessage(IntPtr hWnd, int msg, IntPtr wParam, IntPtr lParam);
}

/// <summary>Owner-drawn list with hover highlighting; drawing is delegated to the dashboard.</summary>
/// <remarks>Rows are drawn over the parent's painting (the glass card and the backdrop behind it), not a flat colour.</remarks>
internal sealed class RowList : ListBox
{
    private const int WmVScroll = 0x0115;
    private const int WmKeyDown = 0x0100;

    private int _hot = -1;
    private int _paintedTop;

    public RowList()
    {
        DrawMode = DrawMode.OwnerDrawFixed;
        BorderStyle = BorderStyle.None;
        IntegralHeight = false;
        // UserPaint routes all painting through OnPaint (double-buffered), avoiding owner-draw flicker.
        SetStyle(ControlStyles.UserPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.AllPaintingInWmPaint | ControlStyles.ResizeRedraw, true);
    }

    protected override void OnSelectedIndexChanged(EventArgs e)
    {
        base.OnSelectedIndexChanged(e);
        Invalidate();
    }

    public int HotIndex => _hot;

    protected override void WndProc(ref Message m)
    {
        // At the top/bottom of the list (or when everything fits), let the page scroll instead.
        if (m.Msg == WheelForwarding.WmMouseWheel)
        {
            var delta = WheelForwarding.Delta(m);
            var visibleRows = Math.Max(1, ClientSize.Height / Math.Max(1, ItemHeight));
            var atTop = TopIndex <= 0;
            var atBottom = TopIndex + visibleRows >= Items.Count;
            if (((delta > 0 && atTop) || (delta < 0 && atBottom)) && WheelForwarding.TryForward(this, ref m))
                return;
        }
        base.WndProc(ref m);

        // Native scrolling moves pixels, which would drag the backdrop along with the rows: repaint everything instead.
        if ((m.Msg is WmVScroll or WheelForwarding.WmMouseWheel or WmKeyDown) && IsHandleCreated && TopIndex != _paintedTop)
            Invalidate();
    }

    public string EmptyText { get; set; } = "";

    public event Action<Graphics, Rectangle, int, bool, bool>? DrawRow; // g, bounds, index, selected, hot

    protected override void OnDrawItem(DrawItemEventArgs e)
    {
        if (e.Index < 0 || e.Index >= Items.Count)
            return;
        DrawRow?.Invoke(e.Graphics, e.Bounds, e.Index, (e.State & DrawItemState.Selected) != 0, e.Index == _hot);
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        // With AllPaintingInWmPaint we paint everything, so paint what's behind us and draw visible rows ourselves.
        _paintedTop = TopIndex;
        PaintParent(e.Graphics, e.ClipRectangle);
        if (Items.Count == 0 && EmptyText.Length > 0)
        {
            TextRenderer.DrawText(e.Graphics, EmptyText, Font, ClientRectangle, ForeColor,
                TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter | TextFormatFlags.WordBreak | TextFormatFlags.NoPrefix);
            return;
        }
        for (var i = TopIndex; i < Items.Count; i++)
        {
            var bounds = GetItemRectangle(i);
            if (bounds.Top > ClientSize.Height)
                break;
            if (bounds.IntersectsWith(e.ClipRectangle))
                DrawRow?.Invoke(e.Graphics, bounds, i, SelectionMode != SelectionMode.None && GetSelected(i), i == _hot);
        }
    }

    private void PaintParent(Graphics g, Rectangle clip)
    {
        if (Parent is null)
        {
            using var bg = new SolidBrush(BackColor);
            g.FillRectangle(bg, clip);
            return;
        }
        var state = g.Save();
        g.SetClip(clip);
        g.TranslateTransform(-Left, -Top);
        var shifted = clip;
        shifted.Offset(Left, Top);
        using (var args = new PaintEventArgs(g, shifted))
        {
            InvokePaintBackground(Parent, args);
            InvokePaint(Parent, args);
        }
        g.Restore(state);
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);
        var index = IndexFromPoint(e.Location);
        if (index != _hot)
        {
            _hot = index;
            Invalidate();
        }
    }

    protected override void OnMouseLeave(EventArgs e)
    {
        base.OnMouseLeave(e);
        if (_hot != -1)
        {
            _hot = -1;
            Invalidate();
        }
    }
}
