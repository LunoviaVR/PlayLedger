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

    /// <summary>Deletes something: tinted in the error colour so it can't be mistaken for a normal action.</summary>
    public bool Destructive { get; set; }

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
        else if (Destructive)
        {
            var fill = _pressed ? Theme.WithAlpha(Theme.Error, Theme.ErrorSoft.A * 2) : Glass.Lerp(Theme.ErrorSoft, Theme.WithAlpha(Theme.Error, Theme.ErrorSoft.A * 3 / 2), hover);
            using (var brush = new SolidBrush(fill))
                g.FillPath(brush, path);
            Glass.PaintBorder(g, path, body, Theme.WithAlpha(Theme.Error, 110), Theme.WithAlpha(Theme.Error, 60));
            text = Theme.ErrorText;
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

/// <summary>A small segmented control: pick one of a few options (Left/Right with the keyboard).</summary>
internal sealed class ChoiceSegments : PaintedControl
{
    private readonly List<Rectangle> _bounds = new();
    private int _selected;
    private int _hot = -1;

    public ChoiceSegments(params string[] options)
    {
        Options = options;
        SetStyle(ControlStyles.Selectable, true);
        TabStop = true;
        AccessibleRole = AccessibleRole.PageTabList;
    }

    public string[] Options { get; }

    public event EventHandler? SelectedIndexChanged;

    public int SelectedIndex
    {
        get => _selected;
        set
        {
            value = Math.Clamp(value, 0, Options.Length - 1);
            if (value == _selected)
                return;
            _selected = value;
            AccessibleDescription = Options[value];
            Invalidate();
        }
    }

    private int SegmentWidth(string text) =>
        (Fonts is null ? S(56) : TextRenderer.MeasureText(text, Fonts.BodyStrong, Size.Empty, TextFormatFlags.NoPrefix | TextFormatFlags.NoPadding).Width) + S(28);

    public Size LogicalSize => new(Options.Sum(SegmentWidth) + S(8) + S(2 * Glass.FocusMargin), S(36 + 2 * Glass.FocusMargin));

    private void Select(int index)
    {
        if (index == _selected || index < 0 || index >= Options.Length)
            return;
        SelectedIndex = index;
        SelectedIndexChanged?.Invoke(this, EventArgs.Empty);
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);
        var hot = _bounds.FindIndex(r => r.Contains(e.Location));
        if (hot == _hot)
            return;
        _hot = hot;
        Cursor = hot >= 0 ? Cursors.Hand : Cursors.Default;
        Invalidate();
    }

    protected override void OnMouseLeave(EventArgs e) { base.OnMouseLeave(e); _hot = -1; Invalidate(); }

    protected override void OnMouseDown(MouseEventArgs e)
    {
        base.OnMouseDown(e);
        Focus();
        if (e.Button == MouseButtons.Left)
            Select(_bounds.FindIndex(r => r.Contains(e.Location)));
    }

    protected override bool IsInputKey(Keys keyData) => keyData is Keys.Left or Keys.Right || base.IsInputKey(keyData);

    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        if (e.KeyCode == Keys.Left)
            Select(_selected - 1);
        else if (e.KeyCode == Keys.Right)
            Select(_selected + 1);
    }

    protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
    protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        if (Fonts is null)
            return;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        var body = BodyRect;
        Glass.PaintSurface(g, body, S(Radius.Control), Theme.GlassControl, Theme, sheen: false);
        _bounds.Clear();
        var inset = S(4);
        var x = (int)body.X + inset;
        for (var i = 0; i < Options.Length; i++)
        {
            var r = new Rectangle(x, (int)body.Y + inset, SegmentWidth(Options[i]), (int)body.Height - inset * 2);
            _bounds.Add(r);
            var rf = new RectangleF(r.X + 0.5f, r.Y + 0.5f, r.Width - 1, r.Height - 1);
            using var path = Theme.RoundedRect(rf, S(Radius.Small - 2));
            if (i == _selected)
            {
                using (var fill = new SolidBrush(Theme.AccentSoft))
                    g.FillPath(fill, path);
                Glass.PaintBorder(g, path, rf, Theme.AccentBorder, Theme.WithAlpha(Theme.AccentBorder, Theme.AccentBorder.A / 3));
                if (ShowFocusRing)
                    Glass.PaintFocusRing(g, rf, S(Radius.Small - 2), Theme, UiScale);
            }
            else if (i == _hot)
            {
                using var fill = new SolidBrush(Theme.GlassHover);
                g.FillPath(fill, path);
            }
            var color = i == _selected ? Theme.AccentText : i == _hot ? Theme.TextPrimary : Theme.TextSecondary;
            DrawLabel(g, Options[i], i == _selected ? Fonts.BodyStrong : Fonts.Body, color, r, TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
            x += r.Width;
        }
    }
}

/// <summary>Accent colour swatches, plus a last swatch that opens a colour picker for any colour.</summary>
internal sealed class SwatchPicker : PaintedControl
{
    private readonly ToolTip _tip = new();
    private readonly List<RectangleF> _bounds = new();
    private int _hot = -1;
    private int _cursor; // keyboard position
    private string _selectedKey = "blue";

    public SwatchPicker()
    {
        SetStyle(ControlStyles.Selectable, true);
        TabStop = true;
        AccessibleRole = AccessibleRole.List;
    }

    public event Action<string>? SelectionChanged;
    public event Action? CustomRequested;

    private int Count => Accents.Presets.Count + 1; // + custom
    private int CustomIndex => Accents.Presets.Count;
    private float Diameter => S(24);
    private float Step => S(36);

    public Size LogicalSize => new((int)(Step * Count) + S(2 * Glass.FocusMargin), S(36 + 2 * Glass.FocusMargin));

    /// <summary>A preset key or a custom "#rrggbb".</summary>
    public string SelectedKey
    {
        get => _selectedKey;
        set
        {
            _selectedKey = value;
            _cursor = SelectedIndex;
            AccessibleDescription = Accents.Parse(value).Name;
            Invalidate();
        }
    }

    private int SelectedIndex
    {
        get
        {
            var i = Accents.Presets.ToList().FindIndex(p => p.Key == _selectedKey);
            return i >= 0 ? i : CustomIndex;
        }
    }

    private void Activate(int index)
    {
        if (index < 0 || index >= Count)
            return;
        _cursor = index;
        if (index == CustomIndex)
        {
            CustomRequested?.Invoke();
            return;
        }
        var key = Accents.Presets[index].Key;
        if (key == _selectedKey)
            return;
        SelectedKey = key;
        SelectionChanged?.Invoke(key);
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);
        var hot = _bounds.FindIndex(r => RectangleF.Inflate(r, S(4), S(4)).Contains(e.Location));
        if (hot == _hot)
            return;
        _hot = hot;
        Cursor = hot >= 0 ? Cursors.Hand : Cursors.Default;
        _tip.SetToolTip(this, hot < 0 ? null : hot == CustomIndex ? "Custom colour..." : Accents.Presets[hot].Name);
        Invalidate();
    }

    protected override void OnMouseLeave(EventArgs e) { base.OnMouseLeave(e); _hot = -1; Invalidate(); }

    protected override void OnMouseDown(MouseEventArgs e)
    {
        base.OnMouseDown(e);
        Focus();
        if (e.Button == MouseButtons.Left)
            Activate(_bounds.FindIndex(r => RectangleF.Inflate(r, S(4), S(4)).Contains(e.Location)));
    }

    protected override bool IsInputKey(Keys keyData) => keyData is Keys.Left or Keys.Right || base.IsInputKey(keyData);

    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        switch (e.KeyCode)
        {
            case Keys.Left when _cursor > 0:
                _cursor--;
                if (_cursor != CustomIndex) Activate(_cursor); else Invalidate();
                break;
            case Keys.Right when _cursor < Count - 1:
                _cursor++;
                if (_cursor != CustomIndex) Activate(_cursor); else Invalidate();
                break;
            case Keys.Space or Keys.Enter:
                Activate(_cursor);
                break;
        }
    }

    protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
    protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
            _tip.Dispose();
        base.Dispose(disposing);
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        var g = e.Graphics;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        _bounds.Clear();
        var body = BodyRect;
        var d = Diameter;
        var y = body.Y + (body.Height - d) / 2;
        var selected = SelectedIndex;
        for (var i = 0; i < Count; i++)
        {
            var x = body.X + (Step - d) / 2 + i * Step;
            var r = new RectangleF(x, y, d, d);
            _bounds.Add(r);
            if (i == CustomIndex)
            {
                // Custom: the picked colour if one is active, otherwise a spectrum.
                if (selected == CustomIndex)
                {
                    using var brush = new SolidBrush(Accents.Parse(_selectedKey).GradientStart);
                    g.FillEllipse(brush, r);
                }
                else
                {
                    using var spectrum = new LinearGradientBrush(RectangleF.Inflate(r, 1, 1), Color.Red, Color.Blue, 45f)
                    {
                        InterpolationColors = new ColorBlend
                        {
                            Colors = new[] { Theme.Hex("#f43f5e"), Theme.Hex("#f59e0b"), Theme.Hex("#10b981"), Theme.Hex("#3b82f6"), Theme.Hex("#a855f7") },
                            Positions = new[] { 0f, 0.25f, 0.5f, 0.75f, 1f },
                        },
                    };
                    g.FillEllipse(spectrum, r);
                }
                if (Fonts is not null)
                    DrawLabel(g, "+", Fonts.BodyStrong, Color.White, Rectangle.Round(r), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
            }
            else
            {
                var p = Accents.Presets[i];
                using var brush = new LinearGradientBrush(RectangleF.Inflate(r, 1, 1), p.GradientStart, p.GradientEnd, LinearGradientMode.ForwardDiagonal);
                g.FillEllipse(brush, r);
            }
            using (var edge = new Pen(Theme.GlassBorderStrong, 1))
                g.DrawEllipse(edge, r);

            if (i == selected || i == _hot)
            {
                // Selected: a ring with a gap around the swatch. Hover: a fainter one.
                var ring = RectangleF.Inflate(r, S(4), S(4));
                using var pen = new Pen(i == selected ? Theme.TextPrimary : Theme.GlassBorderStrong, i == selected ? 2 * UiScale : 1.5f * UiScale);
                g.DrawEllipse(pen, ring);
            }
            if (i == _cursor && ShowFocusRing)
            {
                var ring = RectangleF.Inflate(r, S(7), S(7));
                using var pen = new Pen(Theme.FocusRing, 2 * UiScale);
                g.DrawEllipse(pen, ring);
            }
        }
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
                ChoiceSegments c => c.LogicalSize,
                SwatchPicker p => p.LogicalSize,
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
        _list.ForeColor = Theme.TextMuted;
        _list.ItemHeight = RowHeight;
        _list.EmptyFont = Fonts?.Body;
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
