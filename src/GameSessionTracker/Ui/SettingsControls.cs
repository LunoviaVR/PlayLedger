using System.Drawing.Drawing2D;

namespace GameSessionTracker.Ui;

/// <summary>On/off switch.</summary>
internal sealed class ToggleSwitch : PaintedControl
{
    private readonly Tween _knob;
    private readonly Tween _hover;
    private bool _checked;

    public ToggleSwitch()
    {
        SetStyle(ControlStyles.Selectable, true);
        TabStop = true;
        Cursor = Cursors.Hand;
        AccessibleRole = AccessibleRole.CheckButton;
        _knob = new Tween(this);
        _hover = new Tween(this);
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
            _knob.Jump(value ? 1 : 0); // loading a value doesn't animate; only the user's toggles do
        }
    }

    /// <summary>44×24 switch plus the focus-ring margin.</summary>
    public Size LogicalSize => new(S(44 + 2 * Glass.FocusMargin), S(24 + 2 * Glass.FocusMargin));

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
        _checked = !_checked;
        _knob.To(_checked ? 1 : 0);
        CheckedChanged?.Invoke(this, EventArgs.Empty);
    }

    protected override void OnMouseEnter(EventArgs e) { base.OnMouseEnter(e); _hover.To(1); }
    protected override void OnMouseLeave(EventArgs e) { base.OnMouseLeave(e); _hover.To(0); }
    protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
    protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _knob.Dispose();
            _hover.Dispose();
        }
        base.Dispose(disposing);
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        var track = BodyRect;
        var on = _knob.Value;
        using (var path = Theme.RoundedRect(track, track.Height / 2))
        {
            // Off: a glass track. On: the accent gradient fades in over it.
            using (var fill = new SolidBrush(Glass.Lerp(Theme.GlassControl, Theme.GlassHover, _hover.Value)))
                g.FillPath(fill, path);
            if (on > 0.001f)
            {
                using var accent = new LinearGradientBrush(new RectangleF(track.X - 1, track.Y, track.Width + 2, track.Height),
                    Theme.WithAlpha(Theme.AccentGradientStart, (int)(255 * on)), Theme.WithAlpha(Theme.AccentGradientEnd, (int)(255 * on)),
                    LinearGradientMode.Horizontal);
                g.FillPath(accent, path);
            }
            using var pen = new Pen(Glass.Lerp(Theme.GlassBorderStrong, Theme.WithAlpha(Theme.AccentGradientEnd, 0), on), 1);
            g.DrawPath(pen, path);
        }
        if (ShowFocusRing)
            Glass.PaintFocusRing(g, track, track.Height / 2, Theme, UiScale);

        var inset = S(3);
        var knob = track.Height - inset * 2;
        var x = track.X + inset + (track.Width - knob - inset * 2) * on;
        var knobColor = Glass.Lerp(Theme.TextSecondary, Color.White, on);
        if (on > 0.5f)
        {
            using var shadow = new SolidBrush(Color.FromArgb(50, 0, 0, 0));
            g.FillEllipse(shadow, x, track.Y + inset + S(1), knob, knob);
        }
        using var knobBrush = new SolidBrush(knobColor);
        g.FillEllipse(knobBrush, x, track.Y + inset, knob, knob);
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
        AccessibleRole = AccessibleRole.SpinButton;
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
            AccessibleDescription = Format(_value);
            Invalidate();
        }
    }

    /// <summary>156×36 field plus the focus-ring margin.</summary>
    public Size LogicalSize => new(S(156 + 2 * Glass.FocusMargin), S(36 + 2 * Glass.FocusMargin));

    private int ZoneWidth => S(36 + Glass.FocusMargin);

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
        g.SmoothingMode = SmoothingMode.AntiAlias;
        var rect = BodyRect;
        var radius = S(Radius.Control);
        using (var path = Theme.RoundedRect(rect, radius))
        {
            using (var fill = new SolidBrush(Theme.GlassControl))
                g.FillPath(fill, path);
            var canChange = _hotZone < 0 ? _value > Minimum : _value < Maximum;
            if (_hotZone != 0 && canChange)
            {
                using var hover = new SolidBrush(Theme.GlassHover);
                var zoneWidth = ZoneWidth - (rect.X - 0.5f);
                var zone = _hotZone < 0 ? new RectangleF(rect.X, rect.Y, zoneWidth, rect.Height) : new RectangleF(rect.Right - zoneWidth, rect.Y, zoneWidth, rect.Height);
                var state = g.Save();
                g.SetClip(path);
                g.FillRectangle(hover, zone);
                g.Restore(state);
            }
            using (var divider = new Pen(Theme.Separator, 1))
            {
                g.DrawLine(divider, ZoneWidth, rect.Y + S(8), ZoneWidth, rect.Bottom - S(8));
                g.DrawLine(divider, Width - ZoneWidth, rect.Y + S(8), Width - ZoneWidth, rect.Bottom - S(8));
            }
            if (Focused)
                Glass.PaintBorder(g, path, rect, Theme.AccentBorder, Theme.AccentBorder);
            else
                Glass.PaintBorder(g, path, rect, Theme.GlassBorderStrong, Theme.GlassBorder);
        }
        if (ShowFocusRing)
            Glass.PaintFocusRing(g, rect, radius, Theme, UiScale);
        if (Fonts is null)
            return;
        var minusColor = _value > Minimum ? Theme.TextPrimary : Theme.TextMuted;
        var plusColor = _value < Maximum ? Theme.TextPrimary : Theme.TextMuted;
        DrawLabel(g, "−", Fonts.BodyStrong, minusColor, new Rectangle(0, 0, ZoneWidth, Height), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
        DrawLabel(g, "+", Fonts.BodyStrong, plusColor, new Rectangle(Width - ZoneWidth, 0, ZoneWidth, Height), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
        DrawLabel(g, Format(_value), Fonts.Body, Theme.TextPrimary, new Rectangle(ZoneWidth, 0, Width - 2 * ZoneWidth, Height), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
    }
}

/// <summary>Rounded text button. Primary buttons carry the accent gradient; secondary buttons are glass.</summary>
internal sealed class PillButton : PaintedControl
{
    private readonly Tween _hover;
    private bool _pressed;

    public PillButton(string text)
    {
        Text = text;
        SetStyle(ControlStyles.Selectable, true);
        TabStop = true;
        Cursor = Cursors.Hand;
        AccessibleRole = AccessibleRole.PushButton;
        AccessibleName = text;
        _hover = new Tween(this);
    }

    public bool Primary { get; set; }

    /// <summary>Button body (text + padding, 34 px tall) plus the focus-ring margin.</summary>
    public Size PreferredButtonSize(Graphics? g = null)
    {
        var width = Fonts is null ? S(80) : TextRenderer.MeasureText(Text, Fonts.BodyStrong, Size.Empty, TextFormatFlags.NoPrefix | TextFormatFlags.NoPadding).Width + S(36);
        var margin = S(2 * Glass.FocusMargin);
        return new Size(Math.Max(S(80), width) + margin, S(34) + margin);
    }

    protected override void OnMouseEnter(EventArgs e) { base.OnMouseEnter(e); _hover.To(1); }
    protected override void OnMouseLeave(EventArgs e) { base.OnMouseLeave(e); _pressed = false; _hover.To(0); }
    protected override void OnMouseDown(MouseEventArgs e) { base.OnMouseDown(e); _pressed = true; Invalidate(); }
    protected override void OnMouseUp(MouseEventArgs e) { base.OnMouseUp(e); _pressed = false; Invalidate(); }
    protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
    protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }

    protected override void OnEnabledChanged(EventArgs e)
    {
        base.OnEnabledChanged(e);
        Cursor = Enabled ? Cursors.Hand : Cursors.Default;
        Invalidate();
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        if (e.KeyCode is Keys.Space or Keys.Enter)
            OnClick(EventArgs.Empty);
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
            _hover.Dispose();
        base.Dispose(disposing);
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        var hover = Enabled ? _hover.Value : 0;
        var body = BodyRect;
        // Hovered buttons rise by 1 px (the margin leaves room); pressed ones settle back.
        var lift = _pressed || Motion.Reduced ? 0 : hover * UiScale;
        body.Y -= lift;
        var radius = S(Radius.Control);
        using var path = Theme.RoundedRect(body, radius);
        Color text;
        if (Primary)
        {
            if (Enabled)
            {
                using var shadow = Theme.RoundedRect(new RectangleF(body.X + S(2), body.Y + S(2) + lift, body.Width - S(4), body.Height), radius);
                using var shadowBrush = new SolidBrush(Theme.WithAlpha(Theme.AccentGradientEnd, (int)(40 + 30 * hover)));
                g.FillPath(shadowBrush, shadow);
            }
            using (var fill = new LinearGradientBrush(new RectangleF(body.X - 1, body.Y - 1, body.Width + 2, body.Height + 2),
                       Theme.AccentGradientStart, Theme.AccentGradientEnd, LinearGradientMode.ForwardDiagonal))
                g.FillPath(fill, path);
            var overlay = _pressed ? Color.FromArgb(28, 0, 0, 0) : Color.FromArgb((int)(28 * hover), 255, 255, 255);
            using (var brush = new SolidBrush(overlay))
                g.FillPath(brush, path);
            Glass.PaintBorder(g, path, body, Color.FromArgb(70, 255, 255, 255), Color.FromArgb(10, 255, 255, 255));
            text = Theme.TextOnAccent;
        }
        else
        {
            var fill = _pressed ? Theme.GlassPressed : Glass.Lerp(Theme.GlassControl, Theme.GlassHover, hover);
            using (var brush = new SolidBrush(fill))
                g.FillPath(brush, path);
            Glass.PaintBorder(g, path, body, Glass.Lerp(Theme.GlassBorderStrong, Theme.WithAlpha(Theme.GlassBorderStrong, Math.Min(255, Theme.GlassBorderStrong.A * 3 / 2)), hover), Theme.GlassBorder);
            text = Theme.TextPrimary;
        }
        if (!Enabled)
        {
            // Disabled: washed out and flat, with muted text.
            using var wash = new SolidBrush(Color.FromArgb(150, Theme.Base));
            g.FillPath(wash, path);
            text = Theme.TextMuted;
        }
        if (ShowFocusRing)
            Glass.PaintFocusRing(g, body, radius, Theme, UiScale);
        if (Fonts is not null)
            DrawLabel(g, Text, Fonts.BodyStrong, text, Rectangle.Round(body), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
    }
}

/// <summary>A settings card: title, optional description, and rows of "label + description → control".</summary>
internal sealed class SettingsCard : PaintedControl, IGlassSurface
{
    private readonly List<(string Title, string Description, Control Control)> _rows = new();

    public float CornerRadius => Radius.Card;

    public string Title { get; set; } = "";
    public string Description { get; set; } = "";

    private int HeaderHeight => string.IsNullOrEmpty(Description) ? S(58) : S(76);
    private int RowHeight => S(68);

    public void AddRow(string title, string description, Control control)
    {
        _rows.Add((title, description, control));
        if (string.IsNullOrEmpty(control.AccessibleName))
            control.AccessibleName = title; // screen readers announce the row's label for its control
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

    public int PreferredHeight => HeaderHeight + _rows.Count * RowHeight + S(4);

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
            // Controls include a focus-ring margin; line their visible edge up with the card's 24 px padding.
            control.Bounds = new Rectangle(Width - S(24 - Glass.FocusMargin) - size.Width, rowTop + (RowHeight - size.Height) / 2, size.Width, size.Height);
        }
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        PaintCard(g);
        if (Fonts is null)
            return;
        var x = S(24);
        var w = Width - S(48);
        DrawLabel(g, Title, Fonts.Heading, Theme.TextPrimary, new Rectangle(x, S(20), w, S(22)));
        if (!string.IsNullOrEmpty(Description))
            DrawLabel(g, Description, Fonts.Small, Theme.TextSecondary, new Rectangle(x, S(44), w, S(18)));

        using var separator = new Pen(Theme.Separator, 1);
        g.SmoothingMode = SmoothingMode.None;
        for (var i = 0; i < _rows.Count; i++)
        {
            var (title, description, control) = _rows[i];
            var top = HeaderHeight + i * RowHeight;
            g.DrawLine(separator, x, top, Width - x, top);
            var textWidth = Math.Max(0, control.Left - S(24) - x);
            DrawLabel(g, title, Fonts.Body, Theme.TextPrimary, new Rectangle(x, top + S(14), textWidth, S(20)));
            DrawLabel(g, description, Fonts.Small, Theme.TextSecondary, new Rectangle(x, top + S(37), textWidth, S(18)));
        }
    }
}

/// <summary>A card with an editable list: header buttons add items, hovering a row reveals "Remove".</summary>
internal sealed class ListEditor : PaintedControl, IGlassSurface
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

    public float CornerRadius => Radius.Card;

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

    private int HeaderHeight => S(80);
    private int RowHeight => S(40);

    public int PreferredHeight => HeaderHeight + Math.Clamp(_items.Count, 1, MaxVisibleRows) * RowHeight + S(12);

    public void ApplyTheme()
    {
        _list.BackColor = Theme.Base;
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
        var right = Width - S(24 - Glass.FocusMargin);
        foreach (var button in Enumerable.Reverse(_buttons))
        {
            var size = button.PreferredButtonSize();
            button.Bounds = new Rectangle(right - size.Width, S(20 - Glass.FocusMargin), size.Width, size.Height);
            right -= size.Width + S(8 - 2 * Glass.FocusMargin);
        }
        _list.ItemHeight = RowHeight;
        _list.Bounds = new Rectangle(S(12), HeaderHeight, Width - S(24), Height - HeaderHeight - S(10));
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        PaintCard(g);
        if (Fonts is null)
            return;
        var x = S(24);
        var textWidth = (_buttons.Count > 0 ? _buttons.Min(b => b.Left) - S(12) : Width - S(24)) - x;
        DrawLabel(g, Title, Fonts.Heading, Theme.TextPrimary, new Rectangle(x, S(20), textWidth, S(22)));
        DrawLabel(g, Description, Fonts.Small, Theme.TextSecondary, new Rectangle(x, S(44), textWidth, S(18)));
        using var separator = new Pen(Theme.Separator, 1);
        g.SmoothingMode = SmoothingMode.None;
        g.DrawLine(separator, x, HeaderHeight - S(6), Width - x, HeaderHeight - S(6));
    }

    private void DrawRow(Graphics g, Rectangle bounds, int index, bool selected, bool hot)
    {
        if (index >= _items.Count || Fonts is null)
            return;
        if (hot)
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            using var path = Theme.RoundedRect(new RectangleF(bounds.X + S(2), bounds.Y + S(2), bounds.Width - S(4), bounds.Height - S(4)), S(Radius.Small));
            using var hover = new SolidBrush(Theme.GlassControl);
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
            TextRenderer.DrawText(g, "Remove", Fonts.Body, new Rectangle(bounds.Right - S(12) - removeWidth, bounds.Y, removeWidth, bounds.Height), Theme.ErrorText, flags | TextFormatFlags.Right);
    }
}

/// <summary>Themed modal asking for one line of text, shown over a darkened window.</summary>
internal sealed class PromptDialog : Form
{
    private readonly Theme _theme;
    private readonly Backdrop _backdrop = new();
    private readonly InputField _field;

    private TextBox Input => _field.TextBox;

    private PromptDialog(Theme theme, Fonts fonts, string title, string label, string initial)
    {
        _theme = theme;
        var s = DeviceDpi / 96f;
        int S(float v) => (int)Math.Round(v * s);
        var margin = S(Glass.FocusMargin);

        Text = title;
        AutoScaleMode = AutoScaleMode.None;
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = MinimizeBox = false;
        ShowInTaskbar = false;
        StartPosition = FormStartPosition.CenterParent;
        BackColor = theme.SurfaceStrong;
        DoubleBuffered = true;
        ClientSize = new Size(S(460), S(184));

        var caption = new Label
        {
            Text = label, Font = fonts.Body, ForeColor = theme.TextPrimary, BackColor = Color.Transparent,
            AutoSize = false, Bounds = new Rectangle(S(24), S(24), S(412), S(22)),
        };
        _field = new InputField(theme, fonts) { Bounds = new Rectangle(S(24) - margin, S(52) - margin, S(412) + 2 * margin, S(40) + 2 * margin) };
        Input.Text = initial;
        Input.AccessibleName = label;

        var ok = new PillButton("OK") { Theme = theme, Fonts = fonts, Primary = true };
        var cancel = new PillButton("Cancel") { Theme = theme, Fonts = fonts };
        var okSize = ok.PreferredButtonSize();
        var cancelSize = cancel.PreferredButtonSize();
        var buttonsTop = ClientSize.Height - S(24) - okSize.Height + margin;
        cancel.Bounds = new Rectangle(ClientSize.Width - S(24) + margin - cancelSize.Width, buttonsTop, cancelSize.Width, cancelSize.Height);
        ok.Bounds = new Rectangle(cancel.Left - S(8) + 2 * margin - okSize.Width, buttonsTop, okSize.Width, okSize.Height);
        ok.Click += (_, _) => { DialogResult = DialogResult.OK; Close(); };
        cancel.Click += (_, _) => { DialogResult = DialogResult.Cancel; Close(); };
        Controls.AddRange(new Control[] { caption, _field, ok, cancel });
        KeyPreview = true;
        KeyDown += (_, e) =>
        {
            if (e.KeyCode == Keys.Enter) { DialogResult = DialogResult.OK; Close(); e.SuppressKeyPress = true; }
            else if (e.KeyCode == Keys.Escape) { DialogResult = DialogResult.Cancel; Close(); }
        };
        HandleCreated += (_, _) => Theme.ApplyWindowChrome(this, theme);
        Shown += (_, _) => { Input.Focus(); Input.SelectAll(); };
    }

    protected override void OnPaintBackground(PaintEventArgs e)
    {
        // The atmosphere shows faintly through a strong glass surface, so the modal reads as the top layer.
        var g = e.Graphics;
        _backdrop.Paint(g, e.ClipRectangle, ClientSize, _theme);
        using (var veil = new SolidBrush(Theme.WithAlpha(_theme.SurfaceStrong, 215)))
            g.FillRectangle(veil, e.ClipRectangle);
        using var highlight = new Pen(_theme.GlassHighlight, 1);
        g.DrawLine(highlight, 0, 0, ClientSize.Width, 0);
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
            _backdrop.Dispose();
        base.Dispose(disposing);
    }

    /// <summary>Returns the trimmed text, or null if cancelled or left empty.</summary>
    public static string? Ask(IWin32Window owner, Theme theme, Fonts fonts, string title, string label, string initial = "")
    {
        using var dialog = new PromptDialog(theme, fonts, title, label, initial);
        return ModalScrim.Show(owner, theme, () => dialog.ShowDialog(owner)) == DialogResult.OK && !string.IsNullOrWhiteSpace(dialog.Input.Text)
            ? dialog.Input.Text.Trim()
            : null;
    }

    /// <summary>A rounded input surface around a borderless native text box, with an accent focus ring.</summary>
    private sealed class InputField : PaintedControl
    {
        public InputField(Theme theme, Fonts fonts)
        {
            Theme = theme;
            Fonts = fonts;
            Cursor = Cursors.IBeam;
            // Native edit controls can't be translucent, so the input surface is opaque and the text box matches it.
            TextBox = new TextBox { BorderStyle = BorderStyle.None, Font = fonts.Body, ForeColor = theme.TextPrimary, BackColor = theme.InputSolid };
            TextBox.GotFocus += (_, _) => Invalidate();
            TextBox.LostFocus += (_, _) => Invalidate();
            Controls.Add(TextBox);
        }

        public TextBox TextBox { get; }

        protected override void OnMouseDown(MouseEventArgs e)
        {
            base.OnMouseDown(e);
            TextBox.Focus();
        }

        protected override void OnLayout(LayoutEventArgs levent)
        {
            base.OnLayout(levent);
            var body = Rectangle.Round(BodyRect);
            var height = TextBox.PreferredHeight;
            TextBox.Bounds = new Rectangle(body.X + S(12), body.Y + (body.Height - height) / 2, Math.Max(0, body.Width - S(24)), height);
        }

        protected override void OnPaint(PaintEventArgs e)
        {
            var g = e.Graphics;
            g.SmoothingMode = SmoothingMode.AntiAlias;
            var body = BodyRect;
            var radius = S(Radius.Input);
            var focused = TextBox.Focused;
            using var path = Theme.RoundedRect(body, radius);
            using (var fill = new SolidBrush(Theme.InputSolid))
                g.FillPath(fill, path);
            if (focused)
            {
                Glass.PaintBorder(g, path, body, Theme.AccentBorder, Theme.AccentBorder);
                // Soft ring rather than a glow: visible, not loud.
                var ring = RectangleF.Inflate(body, UiScale * 1.5f, UiScale * 1.5f);
                using var ringPath = Theme.RoundedRect(ring, radius + UiScale * 1.5f);
                using var ringPen = new Pen(Theme.WithAlpha(Theme.Accent, 70), 3 * UiScale);
                g.DrawPath(ringPen, ringPath);
            }
            else
            {
                Glass.PaintBorder(g, path, body, Theme.GlassBorderStrong, Theme.GlassBorder);
            }
        }
    }
}

/// <summary>Right-click menus: a strong (near-opaque) glass surface so what's behind never hurts readability.</summary>
internal static class ThemedMenu
{
    private const string DestructiveTag = "destructive";

    public static void Apply(ContextMenuStrip menu, Theme theme, Font font)
    {
        menu.Renderer = new Renderer(theme);
        menu.BackColor = theme.SurfaceStrong;
        menu.ForeColor = theme.TextPrimary;
        menu.Font = font;
        menu.ShowImageMargin = false;
        menu.Padding = new Padding(4, 4, 4, 4);
        menu.Opened -= OnOpened;
        menu.Opened += OnOpened;
        menu.Tag = theme;
    }

    public static ToolStripMenuItem Item(string text, Theme theme, Action onClick, bool destructive = false) =>
        new(text, null, (_, _) => onClick())
        {
            ForeColor = destructive ? theme.ErrorText : theme.TextPrimary,
            Padding = new Padding(4, 6, 4, 6),
            Tag = destructive ? DestructiveTag : null,
        };

    private static void OnOpened(object? sender, EventArgs e)
    {
        if (sender is ContextMenuStrip { Tag: Theme theme } menu && menu.IsHandleCreated)
            Theme.ApplyPopupChrome(menu.Handle, theme);
    }

    private sealed class Renderer : ToolStripProfessionalRenderer
    {
        private readonly Theme _t;

        public Renderer(Theme theme) : base(new Colors(theme))
        {
            _t = theme;
            RoundedEdges = false;
        }

        protected override void OnRenderToolStripBackground(ToolStripRenderEventArgs e)
        {
            using var brush = new SolidBrush(_t.SurfaceStrong);
            e.Graphics.FillRectangle(brush, e.AffectedBounds);
        }

        protected override void OnRenderToolStripBorder(ToolStripRenderEventArgs e)
        {
            if (Theme.IsWindows11)
                return; // DWM draws a rounded, themed border (see Theme.ApplyPopupChrome)
            using var pen = new Pen(Theme.Over(_t.GlassBorderStrong, _t.SurfaceStrong), 1);
            e.Graphics.DrawRectangle(pen, 0, 0, e.ToolStrip.Width - 1, e.ToolStrip.Height - 1);
        }

        protected override void OnRenderMenuItemBackground(ToolStripItemRenderEventArgs e)
        {
            if (!e.Item.Selected || !e.Item.Enabled)
                return;
            var scale = (e.ToolStrip?.DeviceDpi ?? 96) / 96f;
            var g = e.Graphics;
            g.SmoothingMode = SmoothingMode.AntiAlias;
            var rect = new RectangleF(2 * scale, 1, e.Item.Width - 4 * scale, e.Item.Height - 2);
            using var path = Theme.RoundedRect(rect, 6 * scale);
            var fill = Equals(e.Item.Tag, DestructiveTag) ? _t.ErrorSoft : _t.GlassHover;
            using var brush = new SolidBrush(Theme.Over(fill, _t.SurfaceStrong));
            g.FillPath(brush, path);
        }

        protected override void OnRenderItemText(ToolStripItemTextRenderEventArgs e)
        {
            e.TextColor = !e.Item.Enabled ? _t.TextMuted : Equals(e.Item.Tag, DestructiveTag) ? _t.ErrorText : _t.TextPrimary;
            base.OnRenderItemText(e);
        }

        protected override void OnRenderSeparator(ToolStripSeparatorRenderEventArgs e)
        {
            var scale = (e.ToolStrip?.DeviceDpi ?? 96) / 96f;
            var y = e.Item.Height / 2;
            using var pen = new Pen(Theme.Over(_t.Separator, _t.SurfaceStrong), 1);
            e.Graphics.DrawLine(pen, 8 * scale, y, e.Item.Width - 8 * scale, y);
        }
    }

    private sealed class Colors : ProfessionalColorTable
    {
        private readonly Theme _t;
        public Colors(Theme theme) { _t = theme; UseSystemColors = false; }
        public override Color ToolStripDropDownBackground => _t.SurfaceStrong;
        public override Color MenuBorder => Theme.Over(_t.GlassBorderStrong, _t.SurfaceStrong);
        public override Color MenuItemBorder => Color.Transparent;
        public override Color SeparatorDark => Theme.Over(_t.Separator, _t.SurfaceStrong);
        public override Color SeparatorLight => _t.SurfaceStrong;
        public override Color ImageMarginGradientBegin => _t.SurfaceStrong;
        public override Color ImageMarginGradientMiddle => _t.SurfaceStrong;
        public override Color ImageMarginGradientEnd => _t.SurfaceStrong;
    }
}
