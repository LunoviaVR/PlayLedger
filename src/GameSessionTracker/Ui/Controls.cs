using System.Drawing.Drawing2D;

namespace GameSessionTracker.Ui;

/// <summary>Base for the dashboard's custom-painted pieces.</summary>
internal abstract class PaintedControl : Control
{
    protected PaintedControl()
    {
        SetStyle(ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer |
                 ControlStyles.ResizeRedraw | ControlStyles.UserPaint | ControlStyles.SupportsTransparentBackColor, true);
    }

    public Theme Theme { get; set; } = Theme.Light;
    public Fonts? Fonts { get; set; }

    protected float UiScale => DeviceDpi / 96f;
    protected int S(float logical) => (int)Math.Round(logical * UiScale);

    protected void PaintCard(Graphics g)
    {
        g.Clear(Theme.Window);
        g.SmoothingMode = SmoothingMode.AntiAlias;
        var rect = new RectangleF(0.5f, 0.5f, Width - 1.5f, Height - 1.5f);
        using var path = Theme.RoundedRect(rect, S(8));
        using var fill = new SolidBrush(Theme.Surface);
        using var pen = new Pen(Theme.Border, 1);
        g.FillPath(fill, path);
        g.DrawPath(pen, path);
    }

    protected static void DrawLabel(Graphics g, string text, Font font, Color color, Rectangle bounds, TextFormatFlags flags = TextFormatFlags.Left) =>
        TextRenderer.DrawText(g, text, font, bounds, color,
            flags | TextFormatFlags.NoPrefix | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine | TextFormatFlags.NoPadding);
}

/// <summary>A rounded card with a title; child controls sit below the title.</summary>
internal sealed class Card : Panel
{
    public Card()
    {
        SetStyle(ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw | ControlStyles.UserPaint, true);
    }

    public Theme Theme { get; set; } = Theme.Light;
    public Fonts? Fonts { get; set; }
    public string Title { get; set; } = "";
    public string Detail { get; set; } = "";

    public int HeaderHeight => (int)Math.Round(48 * DeviceDpi / 96f);

    public void ApplyPadding()
    {
        var s = DeviceDpi / 96f;
        Padding = new Padding((int)(8 * s), HeaderHeight, (int)(8 * s), (int)(8 * s));
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var s = DeviceDpi / 96f;
        var g = e.Graphics;
        g.Clear(Theme.Window);
        g.SmoothingMode = SmoothingMode.AntiAlias;
        using (var path = Theme.RoundedRect(new RectangleF(0.5f, 0.5f, Width - 1.5f, Height - 1.5f), 8 * s))
        using (var fill = new SolidBrush(Theme.Surface))
        using (var pen = new Pen(Theme.Border, 1))
        {
            g.FillPath(fill, path);
            g.DrawPath(pen, path);
        }
        if (Fonts is null)
            return;
        var header = new Rectangle((int)(20 * s), 0, Width - (int)(40 * s), HeaderHeight);
        TextRenderer.DrawText(g, Title, Fonts.Heading, header, Theme.TextPrimary,
            TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.NoPrefix | TextFormatFlags.EndEllipsis);
        TextRenderer.DrawText(g, Detail, Fonts.Small, header, Theme.TextMuted,
            TextFormatFlags.Right | TextFormatFlags.VerticalCenter | TextFormatFlags.NoPrefix);
    }
}

/// <summary>Label, big value, and a short caption.</summary>
internal sealed class StatTile : PaintedControl
{
    public string Label { get; set; } = "";
    public string Value { get; set; } = "";
    public string Caption { get; set; } = "";

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        PaintCard(g);
        if (Fonts is null)
            return;
        var x = S(20);
        var w = Width - S(40);
        DrawLabel(g, Label, Fonts.Small, Theme.TextSecondary, new Rectangle(x, S(14), w, S(18)));
        DrawLabel(g, Value, Fonts.TileValue, Theme.TextPrimary, new Rectangle(x, S(33), w, S(36)));
        DrawLabel(g, Caption, Fonts.Small, Theme.TextMuted, new Rectangle(x, S(70), w, S(18)));
    }
}

/// <summary>Header: page title on the left, what's playing now and the page switcher on the right.</summary>
internal sealed class HeaderBar : PaintedControl
{
    private readonly List<Rectangle> _tabBounds = new();
    private int _hotTab = -1;

    public string Title { get; set; } = "";
    public string Status { get; set; } = "";
    public bool IsLive { get; set; }
    public string[] Tabs { get; set; } = Array.Empty<string>();
    public int SelectedTab { get; set; }

    public event Action<int>? TabClicked;

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        g.Clear(Theme.Window);
        if (Fonts is null)
            return;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        const TextFormatFlags measure = TextFormatFlags.NoPadding | TextFormatFlags.NoPrefix | TextFormatFlags.SingleLine;

        // Segmented page switcher, right-aligned.
        _tabBounds.Clear();
        var tabHeight = S(34);
        var widths = Tabs.Select(t => TextRenderer.MeasureText(g, t, Fonts.BodyStrong, Size.Empty, measure).Width + S(28)).ToList();
        var total = widths.Sum() + S(6);
        var outer = new Rectangle(Width - total, (Height - tabHeight) / 2, total, tabHeight);
        using (var path = Theme.RoundedRect(new RectangleF(outer.X + 0.5f, outer.Y + 0.5f, outer.Width - 1, outer.Height - 1), S(8)))
        using (var fill = new SolidBrush(Theme.Track))
            g.FillPath(fill, path);
        var x = outer.X + S(3);
        for (var i = 0; i < Tabs.Length; i++)
        {
            var r = new Rectangle(x, outer.Y + S(3), widths[i], tabHeight - S(6));
            _tabBounds.Add(r);
            if (i == SelectedTab)
            {
                using var path = Theme.RoundedRect(new RectangleF(r.X + 0.5f, r.Y + 0.5f, r.Width - 1, r.Height - 1), S(6));
                using var fill = new SolidBrush(Theme.Surface);
                using var pen = new Pen(Theme.Border, 1);
                g.FillPath(fill, path);
                g.DrawPath(pen, path);
            }
            var color = i == SelectedTab || i == _hotTab ? Theme.TextPrimary : Theme.TextSecondary;
            DrawLabel(g, Tabs[i], i == SelectedTab ? Fonts.BodyStrong : Fonts.Body, color, r, TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
            x += widths[i];
        }

        var titleSize = TextRenderer.MeasureText(g, Title, Fonts.Title, Size.Empty, measure);
        var statusRight = outer.X - S(24);
        var titleWidth = Math.Min(titleSize.Width + S(4), Math.Max(S(120), statusRight - S(200)));
        DrawLabel(g, Title, Fonts.Title, Theme.TextPrimary, new Rectangle(0, 0, titleWidth, Height), TextFormatFlags.VerticalCenter);

        // Live status, just left of the switcher. The dot is decoration beside the text, never the only signal.
        var dot = S(8);
        var statusLeftLimit = titleWidth + S(32) + dot;
        var statusSize = TextRenderer.MeasureText(g, Status, Fonts.Body, Size.Empty, measure);
        var statusWidth = Math.Min(statusSize.Width + S(4), statusRight - statusLeftLimit);
        if (statusWidth > S(60))
        {
            var statusRect = new Rectangle(statusRight - statusWidth, 0, statusWidth, Height);
            DrawLabel(g, Status, Fonts.Body, IsLive ? Theme.TextPrimary : Theme.TextMuted, statusRect, TextFormatFlags.VerticalCenter);
            using var brush = new SolidBrush(IsLive ? Theme.Live : Theme.TextMuted);
            g.FillEllipse(brush, statusRect.X - dot - S(8), (Height - dot) / 2f, dot, dot);
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
        g.Clear(Theme.Window);
        if (Fonts is null)
            return;
        const TextFormatFlags measure = TextFormatFlags.NoPrefix | TextFormatFlags.SingleLine | TextFormatFlags.NoPadding;
        var messageWidth = TextRenderer.MeasureText(g, Message, Fonts.Small, Size.Empty, measure).Width;
        var linkSize = TextRenderer.MeasureText(g, LinkText, Fonts.Small, Size.Empty, measure);
        DrawLabel(g, Message, Fonts.Small, Theme.TextMuted, new Rectangle(0, 0, messageWidth + S(4), Height), TextFormatFlags.VerticalCenter);
        _linkBounds = new Rectangle(messageWidth + S(16), (Height - linkSize.Height) / 2, linkSize.Width + S(4), linkSize.Height);
        using var font = _linkHot ? new Font(Fonts.Small, FontStyle.Underline) : null;
        DrawLabel(g, LinkText, font ?? Fonts.Small, Theme.Accent, _linkBounds, TextFormatFlags.VerticalCenter);
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
internal sealed class DailyChart : PaintedControl
{
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
            var top = S(52);
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

        var headerRect = new Rectangle(S(20), 0, Width - S(40), S(48));
        DrawLabel(g, Title, Fonts.Heading, Theme.TextPrimary, headerRect, TextFormatFlags.VerticalCenter);
        DrawLabel(g, Detail, Fonts.Small, Theme.TextMuted, headerRect, TextFormatFlags.VerticalCenter | TextFormatFlags.Right);

        var plot = PlotArea;
        if (_days.Count == 0 || _days.All(d => d.Total <= TimeSpan.Zero))
        {
            DrawLabel(g, "No playtime in the last 30 days", Fonts.Body, Theme.TextMuted, plot, TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
            return;
        }

        var (stepMinutes, axisMinutes) = NiceAxis(_days.Max(d => d.Total.TotalMinutes));

        // Recessive gridlines + y labels.
        using (var gridPen = new Pen(Theme.Track, 1))
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
            using var hoverBrush = new SolidBrush(Theme.Hover);
            g.FillRectangle(hoverBrush, plot.Left + slot * _hover, plot.Top, slot, plot.Height);
        }

        using (var barBrush = new SolidBrush(Theme.Accent))
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

        var box = new RectangleF(x, y, w, h);
        using (var path = Theme.RoundedRect(box, S(6)))
        using (var fill = new SolidBrush(Theme.IsDark ? Theme.Track : Theme.Surface))
        using (var pen = new Pen(Theme.Border, 1))
        {
            g.FillPath(fill, path);
            g.DrawPath(pen, path);
        }
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
internal sealed class RowList : ListBox
{
    private int _hot = -1;

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
        // With AllPaintingInWmPaint we paint everything, so fill the background and draw visible rows ourselves.
        using (var bg = new SolidBrush(BackColor))
            e.Graphics.FillRectangle(bg, ClientRectangle);
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
