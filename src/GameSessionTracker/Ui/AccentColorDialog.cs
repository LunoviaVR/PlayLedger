using System.Drawing.Drawing2D;
using System.Globalization;

namespace GameSessionTracker.Ui;

/// <summary>
/// Picks a custom accent colour in the app's own glass style (instead of the classic Windows colour dialog):
/// a saturation/brightness field, a hue slider, a hex box and a live preview of how the accent will look.
/// Everything works from the keyboard too.
/// </summary>
internal sealed class AccentColorDialog : GlassDialog
{
    private readonly SaturationValueField _field;
    private readonly HueSlider _hue;
    private readonly InputField _hex;
    private readonly PillButton _ok;
    private bool _syncing;

    private float _h, _s, _v;

    internal AccentColorDialog(Theme theme, Fonts fonts, Color initial) : base(theme, fonts, "Custom accent colour")
    {
        var margin = S(Glass.FocusMargin);
        ClientSize = new Size(S(520), S(404));
        (_h, _s, _v) = ToHsv(initial);

        _field = new SaturationValueField(this) { Theme = theme, Fonts = fonts, AccessibleName = "Saturation and brightness" };
        _field.Bounds = new Rectangle(S(24) - margin, S(24) - margin, S(300) + 2 * margin, S(200) + 2 * margin);
        _hue = new HueSlider(this) { Theme = theme, Fonts = fonts, AccessibleName = "Hue" };
        _hue.Bounds = new Rectangle(S(24) - margin, S(240) - margin, S(300) + 2 * margin, S(20) + 2 * margin);

        var hexLabel = new Label
        {
            Text = "Hex", Font = fonts.Small, ForeColor = theme.TextSecondary, BackColor = Color.Transparent,
            AutoSize = false, Bounds = new Rectangle(S(24), S(280), S(120), S(18)),
        };
        _hex = new InputField(theme, fonts) { Bounds = new Rectangle(S(24) - margin, S(300) - margin, S(140) + 2 * margin, S(36) + 2 * margin) };
        _hex.TextBox.MaxLength = 7;
        _hex.TextBox.AccessibleName = "Hex colour code";
        _hex.TextBox.TextChanged += (_, _) => OnHexChanged();
        Controls.AddRange(new Control[] { _field, _hue, hexLabel, _hex });

        _ok = Button("Use colour", primary: true);
        var cancel = Button("Cancel");
        PlaceButtons(cancel, _ok);
        _ok.Click += (_, _) => { DialogResult = DialogResult.OK; Close(); };
        cancel.Click += (_, _) => { DialogResult = DialogResult.Cancel; Close(); };
        KeyDown += (_, e) =>
        {
            if (e.KeyCode == Keys.Enter && _ok.Enabled) { DialogResult = DialogResult.OK; Close(); e.SuppressKeyPress = true; }
        };
        Shown += (_, _) => _field.Focus();
        SyncHex();
    }

    public Color Selected => FromHsv(_h, _s, _v);

    /// <summary>Returns the chosen colour, or null if cancelled.</summary>
    public static Color? Pick(IWin32Window owner, Theme theme, Fonts fonts, Color initial)
    {
        using var dialog = new AccentColorDialog(theme, fonts, initial);
        return dialog.ShowOver(owner) == DialogResult.OK ? dialog.Selected : null;
    }

    internal void SetSaturationValue(float s, float v)
    {
        _s = Math.Clamp(s, 0, 1);
        _v = Math.Clamp(v, 0, 1);
        Changed();
    }

    internal void SetHue(float h)
    {
        _h = ((h % 360) + 360) % 360;
        Changed();
    }

    internal float Hue => _h;
    internal float Saturation => _s;
    internal float Value => _v;

    private void Changed()
    {
        SyncHex();
        _field.Invalidate();
        _hue.Invalidate();
        Invalidate(PreviewBounds);
    }

    private void SyncHex()
    {
        _syncing = true;
        _hex.TextBox.Text = Accents.ToKey(Selected).ToUpperInvariant();
        _syncing = false;
        _ok.Enabled = true;
    }

    private void OnHexChanged()
    {
        if (_syncing)
            return;
        var text = _hex.TextBox.Text.Trim().TrimStart('#');
        var valid = text.Length == 6 && int.TryParse(text, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out _);
        _ok.Enabled = valid;
        if (!valid)
            return;
        var rgb = int.Parse(text, NumberStyles.HexNumber, CultureInfo.InvariantCulture);
        (_h, _s, _v) = ToHsv(Color.FromArgb(255, (rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff));
        _field.Invalidate();
        _hue.Invalidate();
        Invalidate(PreviewBounds);
    }

    private Rectangle PreviewBounds => new(S(344), S(24), S(152), S(312));

    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        var g = e.Graphics;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        var area = PreviewBounds;
        DrawText(g, "Preview", Fonts.Small, Theme.TextSecondary, new Rectangle(area.X, area.Y, area.Width, S(18)));

        // The accent family this colour produces, exactly as the app will use it.
        var spec = Accents.FromColor(Selected);
        var swatch = new RectangleF(area.X, area.Y + S(24), area.Width, S(96));
        using (var path = Theme.RoundedRect(swatch, S(Radius.Card)))
        using (var brush = new LinearGradientBrush(RectangleF.Inflate(swatch, 1, 1), spec.GradientStart, spec.GradientEnd, LinearGradientMode.ForwardDiagonal))
        {
            g.FillPath(brush, path);
            Glass.PaintBorder(g, path, swatch, Color.FromArgb(70, 255, 255, 255), Color.FromArgb(10, 255, 255, 255));
        }

        // A sample primary button, selected tab and chart bars.
        var button = new RectangleF(area.X, area.Y + S(136), area.Width, S(34));
        using (var path = Theme.RoundedRect(button, S(Radius.Control)))
        using (var brush = new LinearGradientBrush(RectangleF.Inflate(button, 1, 1), spec.GradientStart, spec.GradientEnd, LinearGradientMode.Horizontal))
            g.FillPath(brush, path);
        var mid = Glass.Lerp(spec.GradientStart, spec.GradientEnd, 0.5f);
        var onAccent = Accents.Luminance(mid) > 0.28 ? Theme.Hex("#0b1220") : Color.White;
        DrawText(g, "Button", Fonts.BodyStrong, onAccent, Rectangle.Round(button), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);

        var accent = Theme.IsDark ? spec.Accent : spec.LightAccent;
        var chip = new RectangleF(area.X, area.Y + S(182), area.Width, S(34));
        using (var path = Theme.RoundedRect(chip, S(Radius.Small)))
        using (var fill = new SolidBrush(Theme.WithAlpha(accent, Theme.IsDark ? 36 : 26)))
        {
            g.FillPath(fill, path);
            Glass.PaintBorder(g, path, chip, Theme.WithAlpha(accent, 115), Theme.WithAlpha(accent, 40));
        }
        DrawText(g, "Selected", Fonts.BodyStrong, Theme.IsDark ? spec.Text : spec.LightAccent, Rectangle.Round(chip), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);

        var barsTop = area.Y + S(232);
        var barsBottom = area.Y + S(304);
        float[] heights = { 0.45f, 0.8f, 0.6f, 1f, 0.7f };
        var barWidth = (area.Width - S(8) * (heights.Length - 1)) / (float)heights.Length;
        using var barBrush = new LinearGradientBrush(new RectangleF(area.X, barsTop - 1, area.Width, barsBottom - barsTop + 2),
            Theme.IsDark ? spec.Secondary : spec.GradientEnd, accent, LinearGradientMode.Vertical);
        for (var i = 0; i < heights.Length; i++)
        {
            var h = (barsBottom - barsTop) * heights[i];
            using var bar = Theme.TopRoundedRect(new RectangleF(area.X + i * (barWidth + S(8)), barsBottom - h, barWidth, h), S(3));
            g.FillPath(barBrush, bar);
        }
    }

    // ---------- HSV ----------

    internal static (float H, float S, float V) ToHsv(Color c)
    {
        float r = c.R / 255f, g = c.G / 255f, b = c.B / 255f;
        var max = Math.Max(r, Math.Max(g, b));
        var min = Math.Min(r, Math.Min(g, b));
        var delta = max - min;
        var h = delta == 0 ? 0 : c.GetHue();
        var s = max == 0 ? 0 : delta / max;
        return (h, s, max);
    }

    internal static Color FromHsv(float h, float s, float v)
    {
        var c = v * s;
        var x = c * (1 - Math.Abs(h / 60 % 2 - 1));
        var m = v - c;
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

    /// <summary>The square: saturation left→right, brightness bottom→top, for the current hue. Drag or use the arrow keys.</summary>
    private sealed class SaturationValueField : PaintedControl
    {
        private readonly AccentColorDialog _owner;
        private bool _dragging;

        public SaturationValueField(AccentColorDialog owner)
        {
            _owner = owner;
            SetStyle(ControlStyles.Selectable, true);
            TabStop = true;
            Cursor = Cursors.Cross;
            AccessibleRole = AccessibleRole.Graphic;
        }

        protected override void OnMouseDown(MouseEventArgs e)
        {
            base.OnMouseDown(e);
            Focus();
            if (e.Button != MouseButtons.Left)
                return;
            _dragging = true;
            Capture = true;
            Pick(e.Location);
        }

        protected override void OnMouseMove(MouseEventArgs e)
        {
            base.OnMouseMove(e);
            if (_dragging)
                Pick(e.Location);
        }

        protected override void OnMouseUp(MouseEventArgs e)
        {
            base.OnMouseUp(e);
            _dragging = false;
            Capture = false;
        }

        private void Pick(Point p)
        {
            var body = BodyRect;
            _owner.SetSaturationValue((p.X - body.X) / body.Width, 1 - (p.Y - body.Y) / body.Height);
        }

        protected override bool IsInputKey(Keys keyData) => keyData is Keys.Left or Keys.Right or Keys.Up or Keys.Down || base.IsInputKey(keyData);

        protected override void OnKeyDown(KeyEventArgs e)
        {
            base.OnKeyDown(e);
            var step = e.Shift ? 0.1f : 0.02f;
            switch (e.KeyCode)
            {
                case Keys.Left: _owner.SetSaturationValue(_owner.Saturation - step, _owner.Value); break;
                case Keys.Right: _owner.SetSaturationValue(_owner.Saturation + step, _owner.Value); break;
                case Keys.Up: _owner.SetSaturationValue(_owner.Saturation, _owner.Value + step); break;
                case Keys.Down: _owner.SetSaturationValue(_owner.Saturation, _owner.Value - step); break;
            }
        }

        protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
        protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }

        protected override void OnPaint(PaintEventArgs e)
        {
            var g = e.Graphics;
            g.SmoothingMode = SmoothingMode.AntiAlias;
            var body = BodyRect;
            var radius = S(Radius.Card);
            using (var path = Theme.RoundedRect(body, radius))
            {
                var state = g.Save();
                g.SetClip(path);
                using (var hue = new SolidBrush(FromHsv(_owner.Hue, 1, 1)))
                    g.FillRectangle(hue, body);
                using (var white = new LinearGradientBrush(RectangleF.Inflate(body, 1, 0), Color.White, Color.FromArgb(0, 255, 255, 255), LinearGradientMode.Horizontal))
                    g.FillRectangle(white, body);
                using (var black = new LinearGradientBrush(RectangleF.Inflate(body, 0, 1), Color.FromArgb(0, 0, 0, 0), Color.Black, LinearGradientMode.Vertical))
                    g.FillRectangle(black, body);
                g.Restore(state);
                Glass.PaintBorder(g, path, body, Theme.GlassBorderStrong, Theme.GlassBorder);
            }
            if (ShowFocusRing)
                Glass.PaintFocusRing(g, body, radius, Theme, UiScale);

            // Thumb: a white ring with a soft dark outline so it reads on any colour.
            var cx = body.X + _owner.Saturation * body.Width;
            var cy = body.Y + (1 - _owner.Value) * body.Height;
            var r = S(8);
            using (var outline = new Pen(Color.FromArgb(90, 0, 0, 0), 4 * UiScale))
                g.DrawEllipse(outline, cx - r, cy - r, 2 * r, 2 * r);
            using (var ring = new Pen(Color.White, 2 * UiScale))
                g.DrawEllipse(ring, cx - r, cy - r, 2 * r, 2 * r);
        }
    }

    /// <summary>Rainbow slider for the hue. Drag, or Left/Right (Shift for bigger steps).</summary>
    private sealed class HueSlider : PaintedControl
    {
        private readonly AccentColorDialog _owner;
        private bool _dragging;

        public HueSlider(AccentColorDialog owner)
        {
            _owner = owner;
            SetStyle(ControlStyles.Selectable, true);
            TabStop = true;
            Cursor = Cursors.Hand;
            AccessibleRole = AccessibleRole.Slider;
        }

        protected override void OnMouseDown(MouseEventArgs e)
        {
            base.OnMouseDown(e);
            Focus();
            if (e.Button != MouseButtons.Left)
                return;
            _dragging = true;
            Capture = true;
            Pick(e.X);
        }

        protected override void OnMouseMove(MouseEventArgs e)
        {
            base.OnMouseMove(e);
            if (_dragging)
                Pick(e.X);
        }

        protected override void OnMouseUp(MouseEventArgs e)
        {
            base.OnMouseUp(e);
            _dragging = false;
            Capture = false;
        }

        private void Pick(int x)
        {
            var body = BodyRect;
            _owner.SetHue(Math.Clamp((x - body.X) / body.Width, 0, 0.9999f) * 360);
        }

        protected override bool IsInputKey(Keys keyData) => keyData is Keys.Left or Keys.Right || base.IsInputKey(keyData);

        protected override void OnKeyDown(KeyEventArgs e)
        {
            base.OnKeyDown(e);
            var step = e.Shift ? 15f : 2f;
            if (e.KeyCode == Keys.Left)
                _owner.SetHue(_owner.Hue - step);
            else if (e.KeyCode == Keys.Right)
                _owner.SetHue(_owner.Hue + step);
        }

        protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
        protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }

        protected override void OnPaint(PaintEventArgs e)
        {
            var g = e.Graphics;
            g.SmoothingMode = SmoothingMode.AntiAlias;
            var body = BodyRect;
            using (var path = Theme.RoundedRect(body, body.Height / 2))
            using (var brush = new LinearGradientBrush(RectangleF.Inflate(body, 1, 0), Color.Red, Color.Red, LinearGradientMode.Horizontal))
            {
                brush.InterpolationColors = new ColorBlend
                {
                    Colors = new[] { FromHsv(0, 1, 1), FromHsv(60, 1, 1), FromHsv(120, 1, 1), FromHsv(180, 1, 1), FromHsv(240, 1, 1), FromHsv(300, 1, 1), FromHsv(359.9f, 1, 1) },
                    Positions = new[] { 0f, 1 / 6f, 2 / 6f, 3 / 6f, 4 / 6f, 5 / 6f, 1f },
                };
                g.FillPath(brush, path);
                Glass.PaintBorder(g, path, body, Theme.GlassBorderStrong, Theme.GlassBorder);
            }
            if (ShowFocusRing)
                Glass.PaintFocusRing(g, body, body.Height / 2, Theme, UiScale);

            var cx = body.X + _owner.Hue / 360f * body.Width;
            var r = body.Height / 2 + S(2);
            var cy = body.Y + body.Height / 2;
            using (var fill = new SolidBrush(FromHsv(_owner.Hue, 1, 1)))
                g.FillEllipse(fill, cx - r, cy - r, 2 * r, 2 * r);
            using (var outline = new Pen(Color.FromArgb(90, 0, 0, 0), 4 * UiScale))
                g.DrawEllipse(outline, cx - r, cy - r, 2 * r, 2 * r);
            using (var ring = new Pen(Color.White, 2 * UiScale))
                g.DrawEllipse(ring, cx - r, cy - r, 2 * r, 2 * r);
        }
    }
}
