using System.Drawing.Drawing2D;

namespace GameSessionTracker.Ui;

/// <summary>On/off switch.</summary>
internal sealed class ToggleSwitch : PaintedControl
{
    private bool _checked;
    private bool _hot;

    public ToggleSwitch()
    {
        SetStyle(ControlStyles.Selectable, true);
        TabStop = true;
        Cursor = Cursors.Hand;
    }

    public event EventHandler? CheckedChanged;

    public bool Checked
    {
        get => _checked;
        set
        {
            if (_checked == value)
                return;
            _checked = value;
            Invalidate();
        }
    }

    public Size LogicalSize => new(S(40), S(22));

    protected override void OnClick(EventArgs e)
    {
        base.OnClick(e);
        Focus();
        Toggle();
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        if (e.KeyCode is Keys.Space or Keys.Enter)
            Toggle();
    }

    private void Toggle()
    {
        Checked = !Checked;
        CheckedChanged?.Invoke(this, EventArgs.Empty);
    }

    protected override void OnMouseEnter(EventArgs e) { base.OnMouseEnter(e); _hot = true; Invalidate(); }
    protected override void OnMouseLeave(EventArgs e) { base.OnMouseLeave(e); _hot = false; Invalidate(); }
    protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
    protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        g.Clear(Parent?.BackColor is { A: 255 } bg ? bg : Theme.Surface);
        g.SmoothingMode = SmoothingMode.AntiAlias;
        var track = new RectangleF(1, 1, Width - 3, Height - 3);
        using (var path = Theme.RoundedRect(track, track.Height / 2))
        {
            if (_checked)
            {
                using var fill = new SolidBrush(Theme.Accent);
                g.FillPath(fill, path);
            }
            else
            {
                using var fill = new SolidBrush(_hot ? Theme.Hover : Theme.Surface);
                using var pen = new Pen(Theme.TextMuted, 1);
                g.FillPath(fill, path);
                g.DrawPath(pen, path);
            }
            if (Focused && ShowFocusCues)
            {
                using var focus = new Pen(Theme.TextPrimary, 1) { DashStyle = DashStyle.Dot };
                g.DrawPath(focus, path);
            }
        }
        var knob = track.Height - S(8);
        var x = _checked ? track.Right - S(4) - knob : track.X + S(4);
        using var knobBrush = new SolidBrush(_checked ? Color.White : Theme.TextMuted);
        g.FillEllipse(knobBrush, x, track.Y + S(4), knob, knob);
    }
}

/// <summary>A number with − and + buttons (mouse wheel works too).</summary>
internal sealed class Stepper : PaintedControl
{
    private int _value;
    private int _hotZone; // -1 minus, 1 plus, 0 none

    public Stepper()
    {
        SetStyle(ControlStyles.Selectable, true);
        TabStop = true;
    }

    public int Minimum { get; set; }
    public int Maximum { get; set; } = 100;
    public int Step { get; set; } = 1;
    public Func<int, string> Format { get; set; } = v => v.ToString();

    public event EventHandler? ValueChanged;

    public int Value
    {
        get => _value;
        set
        {
            var clamped = Math.Clamp(value, Minimum, Maximum);
            if (clamped == _value)
                return;
            _value = clamped;
            Invalidate();
        }
    }

    public Size LogicalSize => new(S(150), S(32));

    private int ZoneWidth => S(34);

    private void Change(int delta)
    {
        var before = _value;
        Value = _value + delta;
        if (_value != before)
            ValueChanged?.Invoke(this, EventArgs.Empty);
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);
        var zone = e.X < ZoneWidth ? -1 : e.X > Width - ZoneWidth ? 1 : 0;
        if (zone != _hotZone)
        {
            _hotZone = zone;
            Cursor = zone == 0 ? Cursors.Default : Cursors.Hand;
            Invalidate();
        }
    }

    protected override void OnMouseLeave(EventArgs e)
    {
        base.OnMouseLeave(e);
        _hotZone = 0;
        Invalidate();
    }

    protected override void OnMouseDown(MouseEventArgs e)
    {
        base.OnMouseDown(e);
        Focus();
        if (e.Button != MouseButtons.Left)
            return;
        if (e.X < ZoneWidth)
            Change(-Step);
        else if (e.X > Width - ZoneWidth)
            Change(Step);
    }

    protected override void WndProc(ref Message m)
    {
        // Only change the value with the wheel once the stepper has been clicked; otherwise the wheel scrolls the page.
        if (m.Msg == WheelForwarding.WmMouseWheel)
        {
            if (Focused)
                Change(WheelForwarding.Delta(m) > 0 ? Step : -Step);
            else
                WheelForwarding.TryForward(this, ref m);
            return;
        }
        base.WndProc(ref m);
    }

    protected override bool IsInputKey(Keys keyData) => keyData is Keys.Up or Keys.Down or Keys.Left or Keys.Right || base.IsInputKey(keyData);

    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        if (e.KeyCode is Keys.Up or Keys.Right)
            Change(Step);
        else if (e.KeyCode is Keys.Down or Keys.Left)
            Change(-Step);
    }

    protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
    protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        g.Clear(Parent?.BackColor is { A: 255 } bg ? bg : Theme.Surface);
        g.SmoothingMode = SmoothingMode.AntiAlias;
        var rect = new RectangleF(0.5f, 0.5f, Width - 1.5f, Height - 1.5f);
        using (var path = Theme.RoundedRect(rect, S(6)))
        using (var fill = new SolidBrush(Theme.Surface))
        using (var pen = new Pen(Focused ? Theme.Accent : Theme.Border, 1))
        {
            g.FillPath(fill, path);
            if (_hotZone != 0)
            {
                using var hover = new SolidBrush(Theme.Hover);
                var zone = _hotZone < 0 ? new RectangleF(rect.X, rect.Y, ZoneWidth, rect.Height) : new RectangleF(rect.Right - ZoneWidth, rect.Y, ZoneWidth, rect.Height);
                var state = g.Save();
                g.SetClip(path);
                g.FillRectangle(hover, zone);
                g.Restore(state);
            }
            g.DrawPath(pen, path);
        }
        if (Fonts is null)
            return;
        var minusColor = _value > Minimum ? Theme.TextPrimary : Theme.TextMuted;
        var plusColor = _value < Maximum ? Theme.TextPrimary : Theme.TextMuted;
        DrawLabel(g, "−", Fonts.BodyStrong, minusColor, new Rectangle(0, 0, ZoneWidth, Height), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
        DrawLabel(g, "+", Fonts.BodyStrong, plusColor, new Rectangle(Width - ZoneWidth, 0, ZoneWidth, Height), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
        DrawLabel(g, Format(_value), Fonts.Body, Theme.TextPrimary, new Rectangle(ZoneWidth, 0, Width - 2 * ZoneWidth, Height), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
    }
}

/// <summary>Rounded text button. Primary buttons are filled with the accent colour.</summary>
internal sealed class PillButton : PaintedControl
{
    private bool _hot;
    private bool _pressed;

    public PillButton(string text)
    {
        Text = text;
        SetStyle(ControlStyles.Selectable, true);
        TabStop = true;
        Cursor = Cursors.Hand;
    }

    public bool Primary { get; set; }

    public Size PreferredButtonSize(Graphics? g = null)
    {
        var width = Fonts is null ? S(80) : TextRenderer.MeasureText(Text, Fonts.Body, Size.Empty, TextFormatFlags.NoPrefix | TextFormatFlags.NoPadding).Width + S(32);
        return new Size(Math.Max(S(72), width), S(32));
    }

    protected override void OnMouseEnter(EventArgs e) { base.OnMouseEnter(e); _hot = true; Invalidate(); }
    protected override void OnMouseLeave(EventArgs e) { base.OnMouseLeave(e); _hot = false; _pressed = false; Invalidate(); }
    protected override void OnMouseDown(MouseEventArgs e) { base.OnMouseDown(e); _pressed = true; Invalidate(); }
    protected override void OnMouseUp(MouseEventArgs e) { base.OnMouseUp(e); _pressed = false; Invalidate(); }
    protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
    protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        if (e.KeyCode is Keys.Space or Keys.Enter)
            OnClick(EventArgs.Empty);
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        g.Clear(Parent?.BackColor is { A: 255 } bg ? bg : Theme.Surface);
        g.SmoothingMode = SmoothingMode.AntiAlias;
        var rect = new RectangleF(0.5f, 0.5f, Width - 1.5f, Height - 1.5f);
        using var path = Theme.RoundedRect(rect, S(6));
        Color fill, text;
        if (Primary)
        {
            fill = _pressed ? ControlPaint.Dark(Theme.Accent, 0.05f) : _hot ? ControlPaint.Light(Theme.Accent, 0.15f) : Theme.Accent;
            text = Color.White;
        }
        else
        {
            fill = _pressed ? Theme.Track : _hot ? Theme.Hover : Theme.Surface;
            text = Theme.TextPrimary;
        }
        using (var brush = new SolidBrush(fill))
            g.FillPath(brush, path);
        if (!Primary || Focused)
        {
            using var pen = new Pen(Focused ? Theme.TextPrimary : Theme.Border, 1);
            g.DrawPath(pen, path);
        }
        if (Fonts is not null)
            DrawLabel(g, Text, Fonts.Body, text, ClientRectangle, TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
    }
}

/// <summary>A settings card: title, optional description, and rows of "label + description → control".</summary>
internal sealed class SettingsCard : PaintedControl
{
    private readonly List<(string Title, string Description, Control Control)> _rows = new();

    public string Title { get; set; } = "";
    public string Description { get; set; } = "";

    private int HeaderHeight => string.IsNullOrEmpty(Description) ? S(52) : S(70);
    private int RowHeight => S(64);

    public void AddRow(string title, string description, Control control)
    {
        _rows.Add((title, description, control));
        Controls.Add(control);
    }

    public void SetRowDescription(Control control, string description)
    {
        var index = _rows.FindIndex(r => r.Control == control);
        if (index < 0)
            return;
        _rows[index] = (_rows[index].Title, description, control);
        Invalidate();
    }

    public int PreferredHeight => HeaderHeight + _rows.Count * RowHeight + S(8);

    protected override void OnLayout(LayoutEventArgs e)
    {
        base.OnLayout(e);
        for (var i = 0; i < _rows.Count; i++)
        {
            var control = _rows[i].Control;
            var size = control switch
            {
                ToggleSwitch t => t.LogicalSize,
                Stepper s => s.LogicalSize,
                PillButton b => b.PreferredButtonSize(),
                _ => control.Size,
            };
            var rowTop = HeaderHeight + i * RowHeight;
            control.Bounds = new Rectangle(Width - S(20) - size.Width, rowTop + (RowHeight - size.Height) / 2, size.Width, size.Height);
        }
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        PaintCard(g);
        if (Fonts is null)
            return;
        var x = S(20);
        var w = Width - S(40);
        DrawLabel(g, Title, Fonts.Heading, Theme.TextPrimary, new Rectangle(x, S(18), w, S(20)));
        if (!string.IsNullOrEmpty(Description))
            DrawLabel(g, Description, Fonts.Small, Theme.TextSecondary, new Rectangle(x, S(40), w, S(18)));

        using var separator = new Pen(Theme.Track, 1);
        for (var i = 0; i < _rows.Count; i++)
        {
            var (title, description, control) = _rows[i];
            var top = HeaderHeight + i * RowHeight;
            g.DrawLine(separator, x, top, Width - x, top);
            var textWidth = Math.Max(0, control.Left - S(24) - x);
            DrawLabel(g, title, Fonts.Body, Theme.TextPrimary, new Rectangle(x, top + S(13), textWidth, S(20)));
            DrawLabel(g, description, Fonts.Small, Theme.TextSecondary, new Rectangle(x, top + S(34), textWidth, S(18)));
        }
    }

    protected override void OnBackColorChanged(EventArgs e)
    {
        base.OnBackColorChanged(e);
        foreach (Control c in Controls)
            c.Invalidate();
    }
}

/// <summary>A card with an editable list: header buttons add items, hovering a row reveals "Remove".</summary>
internal sealed class ListEditor : PaintedControl
{
    private const int MaxVisibleRows = 6;
    private readonly RowList _list = new() { SelectionMode = SelectionMode.None };
    private readonly List<PillButton> _buttons = new();
    private List<(string Primary, string Secondary)> _items = new();

    public ListEditor()
    {
        Controls.Add(_list);
        _list.DrawRow += DrawRow;
        _list.MouseClick += (_, e) =>
        {
            var index = _list.IndexFromPoint(e.Location);
            if (index >= 0 && index < _items.Count && e.X >= _list.Width - S(90))
                RemoveRequested?.Invoke(index);
        };
        _list.MouseMove += (_, e) => _list.Cursor = _list.HotIndex >= 0 && e.X >= _list.Width - S(90) ? Cursors.Hand : Cursors.Default;
    }

    public string Title { get; set; } = "";
    public string Description { get; set; } = "";
    public string EmptyText { get => _list.EmptyText; set => _list.EmptyText = value; }

    public event Action<int>? RemoveRequested;

    public PillButton AddButton(string text)
    {
        var button = new PillButton(text) { Theme = Theme, Fonts = Fonts };
        _buttons.Add(button);
        Controls.Add(button);
        return button;
    }

    public IEnumerable<PillButton> Buttons => _buttons;

    public void SetItems(IEnumerable<(string Primary, string Secondary)> items)
    {
        _items = items.ToList();
        var top = _list.Items.Count > 0 ? _list.TopIndex : 0;
        _list.BeginUpdate();
        _list.Items.Clear();
        for (var i = 0; i < _items.Count; i++)
            _list.Items.Add(i);
        if (_items.Count > 0)
            _list.TopIndex = Math.Min(top, _items.Count - 1);
        _list.EndUpdate();
        Parent?.PerformLayout();
    }

    private int HeaderHeight => S(76);
    private int RowHeight => S(40);

    public int PreferredHeight => HeaderHeight + Math.Clamp(_items.Count, 1, MaxVisibleRows) * RowHeight + S(12);

    public void ApplyTheme()
    {
        _list.BackColor = Theme.Surface;
        _list.ForeColor = Theme.TextMuted;
        _list.ItemHeight = RowHeight;
        if (Fonts is not null)
            _list.Font = Fonts.Body;
        foreach (var b in _buttons)
        {
            b.Theme = Theme;
            b.Fonts = Fonts;
            b.Invalidate();
        }
        if (IsHandleCreated)
            Theme.ApplyScrollbarTheme(_list, Theme.IsDark);
        PerformLayout();
        Invalidate(true);
    }

    protected override void OnHandleCreated(EventArgs e)
    {
        base.OnHandleCreated(e);
        Theme.ApplyScrollbarTheme(_list, Theme.IsDark);
    }

    protected override void OnLayout(LayoutEventArgs e)
    {
        base.OnLayout(e);
        var right = Width - S(20);
        foreach (var button in Enumerable.Reverse(_buttons))
        {
            var size = button.PreferredButtonSize();
            button.Bounds = new Rectangle(right - size.Width, S(20), size.Width, size.Height);
            right -= size.Width + S(8);
        }
        _list.ItemHeight = RowHeight;
        _list.Bounds = new Rectangle(S(8), HeaderHeight, Width - S(16), Height - HeaderHeight - S(10));
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        PaintCard(g);
        if (Fonts is null)
            return;
        var x = S(20);
        var textWidth = (_buttons.Count > 0 ? _buttons.Min(b => b.Left) - S(16) : Width - S(20)) - x;
        DrawLabel(g, Title, Fonts.Heading, Theme.TextPrimary, new Rectangle(x, S(18), textWidth, S(20)));
        DrawLabel(g, Description, Fonts.Small, Theme.TextSecondary, new Rectangle(x, S(40), textWidth, S(18)));
        using var separator = new Pen(Theme.Track, 1);
        g.DrawLine(separator, x, HeaderHeight - S(6), Width - x, HeaderHeight - S(6));
    }

    private void DrawRow(Graphics g, Rectangle bounds, int index, bool selected, bool hot)
    {
        using (var bg = new SolidBrush(Theme.Surface))
            g.FillRectangle(bg, bounds);
        if (index >= _items.Count || Fonts is null)
            return;
        if (hot)
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            using var path = Theme.RoundedRect(new RectangleF(bounds.X + S(2), bounds.Y + S(2), bounds.Width - S(4), bounds.Height - S(4)), S(6));
            using var hover = new SolidBrush(Theme.Hover);
            g.FillPath(hover, path);
        }
        var (primary, secondary) = _items[index];
        var flags = TextFormatFlags.NoPrefix | TextFormatFlags.SingleLine | TextFormatFlags.NoPadding | TextFormatFlags.VerticalCenter;
        var x = bounds.X + S(12);
        var removeWidth = S(80);
        var available = bounds.Width - S(24) - removeWidth;
        var primaryWidth = string.IsNullOrEmpty(secondary)
            ? available
            : Math.Min(available / 2, TextRenderer.MeasureText(g, primary, Fonts.Body, Size.Empty, flags).Width + S(4));
        TextRenderer.DrawText(g, primary, Fonts.Body, new Rectangle(x, bounds.Y, primaryWidth, bounds.Height), Theme.TextPrimary, flags | TextFormatFlags.EndEllipsis);
        if (!string.IsNullOrEmpty(secondary))
        {
            var sx = x + primaryWidth + S(12);
            TextRenderer.DrawText(g, secondary, Fonts.Small, new Rectangle(sx, bounds.Y, Math.Max(0, available - primaryWidth - S(12)), bounds.Height), Theme.TextMuted, flags | TextFormatFlags.PathEllipsis);
        }
        if (hot)
            TextRenderer.DrawText(g, "Remove", Fonts.Body, new Rectangle(bounds.Right - S(12) - removeWidth, bounds.Y, removeWidth, bounds.Height), Theme.Accent, flags | TextFormatFlags.Right);
    }
}

/// <summary>Small themed dialog asking for one line of text.</summary>
internal sealed class PromptDialog : Form
{
    private readonly TextBox _input;

    private PromptDialog(Theme theme, Fonts fonts, string title, string label, string initial)
    {
        var s = DeviceDpi / 96f;
        int S(float v) => (int)Math.Round(v * s);

        Text = title;
        AutoScaleMode = AutoScaleMode.None;
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = MinimizeBox = false;
        ShowInTaskbar = false;
        StartPosition = FormStartPosition.CenterParent;
        BackColor = theme.Surface;
        ClientSize = new Size(S(420), S(150));

        var caption = new Label
        {
            Text = label, Font = fonts.Body, ForeColor = theme.TextPrimary, BackColor = theme.Surface,
            AutoSize = false, Bounds = new Rectangle(S(20), S(16), S(380), S(22)),
        };
        _input = new TextBox
        {
            Text = initial, Font = fonts.Body, ForeColor = theme.TextPrimary, BackColor = theme.Window,
            BorderStyle = BorderStyle.FixedSingle, Bounds = new Rectangle(S(20), S(44), S(380), S(28)),
        };
        var ok = new PillButton("OK") { Theme = theme, Fonts = fonts, Primary = true, Bounds = new Rectangle(S(228), S(100), S(80), S(32)) };
        var cancel = new PillButton("Cancel") { Theme = theme, Fonts = fonts, Bounds = new Rectangle(S(320), S(100), S(80), S(32)) };
        ok.Click += (_, _) => { DialogResult = DialogResult.OK; Close(); };
        cancel.Click += (_, _) => { DialogResult = DialogResult.Cancel; Close(); };
        Controls.AddRange(new Control[] { caption, _input, ok, cancel });
        KeyPreview = true;
        KeyDown += (_, e) =>
        {
            if (e.KeyCode == Keys.Enter) { DialogResult = DialogResult.OK; Close(); e.SuppressKeyPress = true; }
            else if (e.KeyCode == Keys.Escape) { DialogResult = DialogResult.Cancel; Close(); }
        };
        HandleCreated += (_, _) => Theme.ApplyWindowChrome(this, theme.IsDark);
        Shown += (_, _) => { _input.Focus(); _input.SelectAll(); };
    }

    /// <summary>Returns the trimmed text, or null if cancelled or left empty.</summary>
    public static string? Ask(IWin32Window owner, Theme theme, Fonts fonts, string title, string label, string initial = "")
    {
        using var dialog = new PromptDialog(theme, fonts, title, label, initial);
        return dialog.ShowDialog(owner) == DialogResult.OK && !string.IsNullOrWhiteSpace(dialog._input.Text)
            ? dialog._input.Text.Trim()
            : null;
    }
}

/// <summary>Right-click menus that match the light/dark theme.</summary>
internal static class ThemedMenu
{
    public static void Apply(ContextMenuStrip menu, Theme theme, Font font)
    {
        menu.Renderer = new ToolStripProfessionalRenderer(new Colors(theme)) { RoundedEdges = true };
        menu.BackColor = theme.Surface;
        menu.ForeColor = theme.TextPrimary;
        menu.Font = font;
        menu.ShowImageMargin = false;
    }

    public static ToolStripMenuItem Item(string text, Theme theme, Action onClick) =>
        new(text, null, (_, _) => onClick()) { ForeColor = theme.TextPrimary, Padding = new Padding(4, 6, 4, 6) };

    private sealed class Colors : ProfessionalColorTable
    {
        private readonly Theme _t;
        public Colors(Theme theme) { _t = theme; UseSystemColors = false; }
        public override Color ToolStripDropDownBackground => _t.Surface;
        public override Color MenuBorder => _t.Border;
        public override Color MenuItemBorder => _t.Hover;
        public override Color MenuItemSelected => _t.Hover;
        public override Color MenuItemSelectedGradientBegin => _t.Hover;
        public override Color MenuItemSelectedGradientEnd => _t.Hover;
        public override Color SeparatorDark => _t.Border;
        public override Color SeparatorLight => _t.Surface;
        public override Color ImageMarginGradientBegin => _t.Surface;
        public override Color ImageMarginGradientMiddle => _t.Surface;
        public override Color ImageMarginGradientEnd => _t.Surface;
    }
}
