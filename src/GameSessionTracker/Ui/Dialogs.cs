using System.Diagnostics;
using System.Drawing.Drawing2D;

namespace GameSessionTracker.Ui;

/// <summary>Row drawing shared by every list that shows sessions, so they all look and behave the same.</summary>
internal static class RowPainter
{
    private const TextFormatFlags Flags = TextFormatFlags.NoPrefix | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine |
                                          TextFormatFlags.NoPadding | TextFormatFlags.VerticalCenter;

    public const TextFormatFlags MeasureFlags = TextFormatFlags.NoPrefix | TextFormatFlags.SingleLine | TextFormatFlags.NoPadding;

    /// <summary>Hover: a quiet glass fill. Selected: an accent tint, a thin inner highlight and a marker on the leading edge.</summary>
    public static void Highlight(Graphics g, Rectangle bounds, bool selected, bool hot, Theme t, float scale)
    {
        if (!selected && !hot)
            return;
        int S(float v) => (int)Math.Round(v * scale);
        g.SmoothingMode = SmoothingMode.AntiAlias;
        var rect = new RectangleF(bounds.X + S(2), bounds.Y + S(2), bounds.Width - S(4), bounds.Height - S(4));
        using var path = Theme.RoundedRect(rect, S(Radius.Small));
        using (var brush = new SolidBrush(selected ? t.AccentSoft : t.GlassControl))
            g.FillPath(brush, path);
        if (!selected)
            return;
        Glass.PaintBorder(g, path, rect, Theme.WithAlpha(t.AccentBorder, t.AccentBorder.A * 2 / 3), Theme.WithAlpha(t.AccentBorder, t.AccentBorder.A / 4));
        var markerHeight = Math.Max(S(12), rect.Height - S(24));
        using var marker = Theme.RoundedRect(new RectangleF(rect.X + S(1), rect.Y + (rect.Height - markerHeight) / 2, S(3), markerHeight), S(1.5f));
        using var markerBrush = new SolidBrush(t.Accent);
        g.FillPath(markerBrush, marker);
    }

    /// <summary>"›" at the right edge of a hovered row that opens something when clicked.</summary>
    public static void Chevron(Graphics g, Rectangle bounds, bool visible, Theme t, Fonts f, float scale)
    {
        if (!visible)
            return;
        var w = (int)(20 * scale);
        TextRenderer.DrawText(g, "›", f.BodyStrong, new Rectangle(bounds.Right - w - (int)(6 * scale), bounds.Y, w, bounds.Height), t.TextSecondary,
            Flags | TextFormatFlags.HorizontalCenter);
    }

    public static int ChevronSpace(float scale) => (int)(24 * scale);

    /// <summary>One session: [date] [● game] [time range] [duration] ›.</summary>
    public static void Session(Graphics g, Rectangle bounds, SessionView s, Theme t, Fonts f, float scale,
        bool showDate, bool showGame, bool hot, bool selected, bool separator)
    {
        int S(float v) => (int)Math.Round(v * scale);
        g.SmoothingMode = SmoothingMode.AntiAlias;
        Highlight(g, bounds, selected, hot, t, scale);

        var x = bounds.X + S(12);
        var right = bounds.Right - S(12) - ChevronSpace(scale) + S(6);
        var row = new Rectangle(0, bounds.Y, 0, bounds.Height);

        var duration = ReportWriter.FormatDuration(s.Duration);
        var durationWidth = S(76);
        TextRenderer.DrawText(g, duration, f.BodyStrong, row with { X = right - durationWidth, Width = durationWidth }, t.TextPrimary, Flags | TextFormatFlags.Right);

        var time = Format.TimeRange(s);
        var cursor = x;
        if (showDate)
        {
            var dateWidth = S(104);
            TextRenderer.DrawText(g, Format.Day(s.Start), f.Body, row with { X = x, Width = dateWidth }, t.TextPrimary, Flags);
            cursor = x + dateWidth + S(12);
        }

        var timeWidth = TextRenderer.MeasureText(g, time, f.Body, Size.Empty, MeasureFlags).Width + S(4);
        var timeX = right - durationWidth - S(12) - timeWidth;
        if (showGame)
        {
            var gameX = cursor;
            var gameWidth = Math.Max(0, timeX - S(12) - gameX);
            if (s.IsLive)
            {
                using var dot = new SolidBrush(t.Success);
                g.FillEllipse(dot, gameX, bounds.Y + (bounds.Height - S(7)) / 2f, S(7), S(7));
                gameX += S(13);
                gameWidth -= S(13);
            }
            TextRenderer.DrawText(g, s.Game, f.Body, row with { X = gameX, Width = Math.Max(0, gameWidth) }, t.TextPrimary, Flags);
        }
        else
        {
            timeX = cursor;
            if (s.IsLive)
            {
                using var dot = new SolidBrush(t.Success);
                g.FillEllipse(dot, timeX, bounds.Y + (bounds.Height - S(7)) / 2f, S(7), S(7));
                timeX += S(13);
            }
            timeWidth = right - durationWidth - S(12) - timeX;
        }
        TextRenderer.DrawText(g, time, f.Body, row with { X = timeX, Width = Math.Max(0, timeWidth) }, s.IsLive ? t.SuccessText : t.TextSecondary, Flags);

        Chevron(g, bounds, hot || selected, t, f, scale);

        if (separator && !hot && !selected)
        {
            using var pen = new Pen(t.Separator, 1);
            g.SmoothingMode = SmoothingMode.None;
            g.DrawLine(pen, x, bounds.Bottom - 1, bounds.Right - S(12), bounds.Bottom - 1);
        }
    }

    /// <summary>A thin rounded bar: track plus an accent-gradient fill for <paramref name="share"/> (0–1).</summary>
    public static void ShareBar(Graphics g, RectangleF r, double share, Theme t)
    {
        g.SmoothingMode = SmoothingMode.AntiAlias;
        using (var track = new SolidBrush(t.Track))
        using (var trackPath = Theme.RoundedRect(r, r.Height / 2f))
            g.FillPath(track, trackPath);
        if (share <= 0)
            return;
        var fillRect = r with { Width = Math.Max(r.Height, (float)(r.Width * Math.Min(1, share))) };
        using var fill = new LinearGradientBrush(new RectangleF(r.X - 1, r.Y, r.Width + 2, r.Height), t.Accent, t.AccentSecondary, LinearGradientMode.Horizontal);
        using var fillPath = Theme.RoundedRect(fillRect, r.Height / 2f);
        g.FillPath(fill, fillPath);
    }
}

/// <summary>
/// Base for the app's modal dialogs: a strong glass surface over the atmospheric backdrop, a themed title bar,
/// Esc to close, and a darkened owner window while it's open (<see cref="ShowOver"/>).
/// </summary>
internal abstract class GlassDialog : Form
{
    private readonly Backdrop _backdrop = new();

    protected GlassDialog(Theme theme, Fonts fonts, string title)
    {
        Theme = theme;
        Fonts = fonts;
        Text = title;
        AutoScaleMode = AutoScaleMode.None;
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = MinimizeBox = false;
        ShowInTaskbar = false;
        StartPosition = FormStartPosition.CenterParent;
        BackColor = theme.SurfaceStrong;
        DoubleBuffered = true;
        KeyPreview = true;
        HandleCreated += (_, _) => Theme.ApplyWindowChrome(this, theme);
    }

    protected Theme Theme { get; }
    protected Fonts Fonts { get; }
    protected float UiScale => DeviceDpi / 96f;
    protected int S(float v) => (int)Math.Round(v * UiScale);

    /// <summary>Shows the dialog modally, dimming <paramref name="owner"/> behind it.</summary>
    public DialogResult ShowOver(IWin32Window owner) => ModalScrim.Show(owner, Theme, () => ShowDialog(owner));

    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        if (e.KeyCode == Keys.Escape && !e.Handled)
        {
            DialogResult = DialogResult.Cancel;
            Close();
        }
    }

    protected override void OnPaintBackground(PaintEventArgs e)
    {
        // The atmosphere shows faintly through a strong glass surface, so the modal reads as the top layer.
        var g = e.Graphics;
        _backdrop.Paint(g, e.ClipRectangle, ClientSize, Theme);
        using (var veil = new SolidBrush(Theme.WithAlpha(Theme.SurfaceStrong, 215)))
            g.FillRectangle(veil, e.ClipRectangle);
        using (var highlight = new Pen(Theme.GlassHighlight, 1))
            g.DrawLine(highlight, 0, 0, ClientSize.Width, 0);
        Glass.PaintChildShadows(g, this, e.ClipRectangle, Theme);
    }

    /// <summary>Right-aligned button row at the bottom; buttons are passed left to right. Returns the row's top.</summary>
    protected int PlaceButtons(params PillButton[] buttons)
    {
        var margin = S(Glass.FocusMargin);
        var right = ClientSize.Width - S(24) + margin;
        var top = 0;
        foreach (var button in buttons.Reverse())
        {
            var size = button.PreferredButtonSize();
            top = ClientSize.Height - S(24) - size.Height + margin;
            button.Bounds = new Rectangle(right - size.Width, top, size.Width, size.Height);
            right -= size.Width + S(8) - 2 * margin;
        }
        return top + margin;
    }

    protected PillButton Button(string text, bool primary = false, bool destructive = false)
    {
        var button = new PillButton(text) { Theme = Theme, Fonts = Fonts, Primary = primary, Destructive = destructive };
        Controls.Add(button);
        return button;
    }

    protected void DrawText(Graphics g, string text, Font font, Color color, Rectangle bounds, TextFormatFlags flags = TextFormatFlags.EndEllipsis) =>
        TextRenderer.DrawText(g, text, font, bounds, color, flags | TextFormatFlags.NoPrefix | TextFormatFlags.SingleLine | TextFormatFlags.NoPadding);

    protected override void Dispose(bool disposing)
    {
        if (disposing)
            _backdrop.Dispose();
        base.Dispose(disposing);
    }
}

/// <summary>Themed modal asking for one line of text, shown over a darkened window.</summary>
internal sealed class PromptDialog : GlassDialog
{
    private readonly InputField _field;

    private TextBox Input => _field.TextBox;

    private PromptDialog(Theme theme, Fonts fonts, string title, string label, string initial) : base(theme, fonts, title)
    {
        var margin = S(Glass.FocusMargin);
        ClientSize = new Size(S(460), S(184));

        var caption = new Label
        {
            Text = label, Font = fonts.Body, ForeColor = theme.TextPrimary, BackColor = Color.Transparent,
            AutoSize = false, Bounds = new Rectangle(S(24), S(24), S(412), S(22)),
        };
        _field = new InputField(theme, fonts) { Bounds = new Rectangle(S(24) - margin, S(52) - margin, S(412) + 2 * margin, S(40) + 2 * margin) };
        Input.Text = initial;
        Input.AccessibleName = label;
        Controls.AddRange(new Control[] { caption, _field });

        var ok = Button("OK", primary: true);
        var cancel = Button("Cancel");
        PlaceButtons(ok, cancel);
        ok.Click += (_, _) => { DialogResult = DialogResult.OK; Close(); };
        cancel.Click += (_, _) => { DialogResult = DialogResult.Cancel; Close(); };
        KeyDown += (_, e) =>
        {
            if (e.KeyCode == Keys.Enter) { DialogResult = DialogResult.OK; Close(); e.SuppressKeyPress = true; }
        };
        Shown += (_, _) => { Input.Focus(); Input.SelectAll(); };
    }

    /// <summary>Returns the trimmed text, or null if cancelled or left empty.</summary>
    public static string? Ask(IWin32Window owner, Theme theme, Fonts fonts, string title, string label, string initial = "")
    {
        using var dialog = new PromptDialog(theme, fonts, title, label, initial);
        return dialog.ShowOver(owner) == DialogResult.OK && !string.IsNullOrWhiteSpace(dialog.Input.Text)
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
            TextBox = new TextBox { BorderStyle = BorderStyle.None, Font = fonts.Body, ForeColor = theme.TextPrimary, BackColor = theme.InputSolid, MaxLength = 260 };
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

/// <summary>
/// Everything about one play session: when the game was opened and closed, how long it ran, the program that was
/// tracked, and where the session sits in that game's history. Live sessions keep updating while the dialog is open.
/// </summary>
internal sealed class SessionDetailsDialog : GlassDialog
{
    private readonly ITrackerHost _host;
    private readonly System.Windows.Forms.Timer _timer;
    private readonly PillButton _delete;
    private readonly PillButton _showProgram;
    private SessionView _session;
    private int _number;
    private int _count;
    private TimeSpan _gameTotal;
    private TimeSpan _dayShare;
    private int _fieldsTop;

    private const int FieldCount = 7;

    /// <summary>True if the session was deleted, so the caller should refresh.</summary>
    public bool Changed { get; private set; }

    public SessionDetailsDialog(ITrackerHost host, Theme theme, Fonts fonts, SessionView session) : base(theme, fonts, "Session details")
    {
        _host = host;
        _session = session;
        ClientSize = new Size(S(500), S(108 + FieldCount * 38 + 24 + 40 + 24));
        _fieldsTop = S(108);

        _delete = Button("Delete session", destructive: true);
        _showProgram = Button("Show program");
        var close = Button("Close", primary: true);
        PlaceButtons(_showProgram, close);
        // Delete sits on the left, apart from the everyday actions.
        var margin = S(Glass.FocusMargin);
        var deleteSize = _delete.PreferredButtonSize();
        _delete.Bounds = new Rectangle(S(24) - margin, close.Top, deleteSize.Width, deleteSize.Height);

        close.Click += (_, _) => Close();
        _delete.Click += (_, _) => Delete();
        _showProgram.Click += (_, _) => ShowProgram();
        Shown += (_, _) => close.Focus();

        _timer = new System.Windows.Forms.Timer { Interval = 1000 };
        _timer.Tick += (_, _) => RefreshSession();
        _timer.Start();
        RefreshSession();
    }

    private void RefreshSession()
    {
        var model = _host.GetModel();
        // A live session may have finished (or been recovered) since the dialog opened; follow it by game + start time.
        var current = model.Sessions.FirstOrDefault(s => s.SameSessionAs(_session));
        if (current is not null)
            _session = current;
        var history = model.SessionsFor(_session.Game).OrderBy(s => s.Start).ToList();
        _count = history.Count;
        _number = history.FindIndex(s => s.SameSessionAs(_session)) + 1;
        _gameTotal = TimeSpan.FromTicks(history.Sum(s => s.Duration.Ticks));
        var day = _session.Start.ToLocalTime().Date;
        _dayShare = model.History().FirstOrDefault(d => d.Day == day)?.Total ?? TimeSpan.Zero;

        _delete.Visible = _session.Source is not null; // sessions in progress can't be deleted yet
        _showProgram.Visible = ProgramPath is not null;
        AccessibleDescription = string.Join(". ", Fields().Select(f => $"{f.Label}: {f.Value}"));
        if (!_session.IsLive)
            _timer.Stop();
        Invalidate();
    }

    private string? ProgramPath =>
        // Local, fully-qualified paths only: probing a UNC path (\\server\...) would make Windows connect to that server.
        _session.Executable is { Length: > 3 } exe && char.IsAsciiLetter(exe[0]) && exe[1] == ':' && exe[2] == '\\' && File.Exists(exe) ? exe : null;

    private IEnumerable<(string Label, string Value)> Fields()
    {
        var start = _session.Start.ToLocalTime();
        var end = _session.End.ToLocalTime();
        yield return ("Opened", $"{Format.LongDay(start.Date)} at {Format.TimeWithSeconds(start)}");
        yield return ("Closed", _session.IsLive
            ? "Still running"
            : start.Date == end.Date ? Format.TimeWithSeconds(end) : $"{Format.LongDay(end.Date)} at {Format.TimeWithSeconds(end)}");
        yield return ("Played for", ReportWriter.FormatDuration(_session.Duration) + (_session.IsLive ? " so far" : ""));
        yield return ("Program", _session.Executable ?? "Not recorded");
        yield return ("Session", _number > 0 ? $"#{_number} of {_count} for this game" : "—");
        yield return ("Game total", $"{ReportWriter.FormatDuration(_gameTotal)} across {Format.Plural(_count, "session")}");
        yield return ("That day", _dayShare > TimeSpan.Zero ? $"{ReportWriter.FormatDuration(_dayShare)} of play in total" : "—");
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        var g = e.Graphics;
        var x = S(24);
        var w = ClientSize.Width - S(48);

        DrawText(g, _session.Game, Fonts.Subtitle, Theme.TextPrimary, new Rectangle(x, S(22), w, S(30)));
        var badgeHeight = S(26);
        var status = _session.IsLive ? "Playing now" : "Finished";
        var size = Glass.MeasureBadge(g, status, Fonts.Small, badgeHeight, UiScale, dot: true);
        var badge = new Rectangle(x, S(62), size.Width, badgeHeight);
        if (_session.IsLive)
            Glass.PaintBadge(g, status, Fonts.Small, Theme.SuccessSoft, Theme.SuccessText, badge, UiScale, Theme.Success);
        else
            Glass.PaintBadge(g, status, Fonts.Small, Theme.NeutralSoft, Theme.TextSecondary, badge, UiScale, Theme.TextMuted);

        // Label / value pairs on a glass panel.
        var panel = new RectangleF(x - S(8), _fieldsTop - S(6), w + S(16), FieldCount * S(38) + S(12));
        Glass.PaintSurface(g, panel, S(Radius.Card), Theme.GlassCard, Theme);
        var y = _fieldsTop;
        var labelWidth = S(112);
        using var separator = new Pen(Theme.Separator, 1);
        var i = 0;
        foreach (var (label, value) in Fields())
        {
            var row = new Rectangle(x + S(8), y, w - S(16), S(38));
            DrawText(g, label, Fonts.Small, Theme.TextSecondary, row with { Width = labelWidth }, TextFormatFlags.VerticalCenter);
            var valueRect = row with { X = row.X + labelWidth, Width = row.Width - labelWidth };
            var color = label == "Closed" && _session.IsLive ? Theme.SuccessText : Theme.TextPrimary;
            DrawText(g, value, label == "Played for" ? Fonts.BodyStrong : Fonts.Body, color, valueRect,
                TextFormatFlags.VerticalCenter | (label == "Program" ? TextFormatFlags.PathEllipsis : TextFormatFlags.EndEllipsis));
            if (++i < FieldCount)
            {
                g.SmoothingMode = SmoothingMode.None;
                g.DrawLine(separator, row.X, row.Bottom, row.Right, row.Bottom);
            }
            y += S(38);
        }
    }

    private void ShowProgram()
    {
        if (ProgramPath is not { } exe)
            return;
        try
        {
            // Full path to Explorer (no search-path lookup). The argument is a path that exists on disk, and Windows paths
            // can't contain quotes, so it can't break out of the quoted /select argument.
            var explorer = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Windows), "explorer.exe");
            Process.Start(new ProcessStartInfo(explorer, $"/select,\"{exe}\"") { UseShellExecute = false });
        }
        catch (Exception ex)
        {
            ErrorLog.Write("Could not open the program's folder", ex);
        }
    }

    private void Delete()
    {
        if (_session.Source is not { } record)
            return;
        var when = $"{Format.Day(record.Start)}, {Format.Time(record.Start)}";
        var answer = ModalScrim.Show(this, Theme, () => MessageBox.Show(this,
            $"Delete the {ReportWriter.FormatDuration(record.Duration)} session of {record.Game} from {when}?",
            "Delete session", MessageBoxButtons.OKCancel, MessageBoxIcon.Warning, MessageBoxDefaultButton.Button2));
        if (answer != DialogResult.OK)
            return;
        _host.DeleteSessions(new[] { record });
        Changed = true;
        Close();
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
            _timer.Dispose();
        base.Dispose(disposing);
    }

    /// <summary>Opens the details for <paramref name="session"/>; returns true if something changed (it was deleted).</summary>
    public static bool Show(IWin32Window owner, ITrackerHost host, Theme theme, Fonts fonts, SessionView session)
    {
        using var dialog = new SessionDetailsDialog(host, theme, fonts, session);
        dialog.ShowOver(owner);
        return dialog.Changed;
    }
}

/// <summary>One day of the history: its total, the games played and every session (click one for its details).</summary>
internal sealed class DayDetailsDialog : GlassDialog
{
    private readonly ITrackerHost _host;
    private readonly DateTime _day;
    private readonly string? _game;
    private readonly Card _card = new() { Title = "Sessions" };
    private readonly RowList _list = new();
    private readonly System.Windows.Forms.Timer _timer;
    private List<SessionView> _sessions = new();
    private DayHistory? _history;

    public bool Changed { get; private set; }

    public DayDetailsDialog(ITrackerHost host, Theme theme, Fonts fonts, DateTime day, string? game)
        : base(theme, fonts, Format.LongDay(day))
    {
        _host = host;
        _day = day.Date;
        _game = game;
        ClientSize = new Size(S(560), S(536));

        _card.Theme = theme;
        _card.Fonts = fonts;
        _card.ApplyPadding();
        _card.Bounds = new Rectangle(S(24), S(128), ClientSize.Width - S(48), ClientSize.Height - S(128) - S(24) - S(40) - S(20));
        _list.Dock = DockStyle.Fill;
        _list.ItemHeight = S(44);
        _list.EmptyFont = fonts.Body;
        _list.ForeColor = theme.TextMuted;
        _list.EmptyText = "No sessions on this day.";
        _list.DrawRow += (g, bounds, index, selected, hot) =>
        {
            if (index < _sessions.Count)
                RowPainter.Session(g, bounds, _sessions[index], Theme, Fonts, UiScale, showDate: false, showGame: _game is null,
                    hot, selected && _list.Focused, separator: index < _sessions.Count - 1);
        };
        _list.MouseClick += (_, e) =>
        {
            var index = _list.IndexFromPoint(e.Location);
            if (e.Button == MouseButtons.Left && index >= 0 && index < _sessions.Count)
                OpenSession(_sessions[index]);
        };
        _list.MouseMove += (_, e) => _list.Cursor = _list.HotIndex >= 0 ? Cursors.Hand : Cursors.Default;
        _list.KeyDown += (_, e) =>
        {
            if (e.KeyCode == Keys.Enter && _list.SelectedIndex >= 0 && _list.SelectedIndex < _sessions.Count)
            {
                OpenSession(_sessions[_list.SelectedIndex]);
                e.Handled = true;
            }
        };
        _list.GotFocus += (_, _) => _list.Invalidate();
        _list.LostFocus += (_, _) => _list.Invalidate();
        _card.Controls.Add(_list);
        Controls.Add(_card);

        var close = Button("Close", primary: true);
        PlaceButtons(close);
        close.Click += (_, _) => Close();
        HandleCreated += (_, _) => Theme.ApplyScrollbarTheme(_list, theme.IsDark);

        _timer = new System.Windows.Forms.Timer { Interval = 1000 };
        _timer.Tick += (_, _) => Reload();
        _timer.Start();
        Reload();
    }

    private void Reload()
    {
        var model = _host.GetModel();
        var sessions = model.SessionsOn(_day, _game).ToList();
        var structureChanged = sessions.Count != _sessions.Count || sessions.Where((s, i) => !s.SameSessionAs(_sessions[i]) || s.IsLive != _sessions[i].IsLive).Any();
        _sessions = sessions;
        _history = model.History().FirstOrDefault(d => d.Day == _day);
        if (structureChanged)
        {
            var top = _list.Items.Count > 0 ? _list.TopIndex : 0;
            _list.BeginUpdate();
            _list.Items.Clear();
            for (var i = 0; i < _sessions.Count; i++)
                _list.Items.Add(i);
            if (_sessions.Count > 0)
                _list.TopIndex = Math.Min(top, _sessions.Count - 1);
            _list.EndUpdate();
        }
        _card.Detail = Format.Plural(_sessions.Count, "session");
        if (_sessions.Any(s => s.IsLive))
            _list.Invalidate();
        else if (!structureChanged && _day != DateTime.Now.Date)
            _timer.Stop(); // a past day with nothing running won't change on its own
        _card.Invalidate(false);
        Invalidate(new Rectangle(0, 0, ClientSize.Width, S(124)));
    }

    private void OpenSession(SessionView session)
    {
        if (SessionDetailsDialog.Show(this, _host, Theme, Fonts, session))
        {
            Changed = true;
            _timer.Start();
            Reload();
        }
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        var g = e.Graphics;
        var x = S(24);
        var w = ClientSize.Width - S(48);
        DrawText(g, Format.LongDay(_day), Fonts.Subtitle, Theme.TextPrimary, new Rectangle(x, S(22), w, S(30)));

        string summary;
        if (_game is not null)
        {
            var total = TimeSpan.FromTicks(_sessions.Sum(s => ModelDayPart(s).Ticks));
            summary = $"{_game} · {ReportWriter.FormatDuration(total)}";
        }
        else if (_history is { Total.Ticks: > 0 } h)
        {
            summary = h.Games.Count == 1
                ? $"{h.Games[0].Game} · {ReportWriter.FormatDuration(h.Total)}"
                : $"{ReportWriter.FormatDuration(h.Total)} in total · " + string.Join(" · ", h.Games.Select(p => $"{p.Game} {ReportWriter.FormatDuration(p.Time)}"));
        }
        else
        {
            summary = "No play on this day";
        }
        DrawText(g, summary, Fonts.Body, Theme.TextSecondary, new Rectangle(x, S(62), w, S(22)));
        DrawText(g, "Click a session for when the game was opened and closed.", Fonts.Small, Theme.TextMuted, new Rectangle(x, S(92), w, S(18)));
    }

    private TimeSpan ModelDayPart(SessionView s) =>
        DashboardModel.SplitByDay(s).Where(p => p.Day == _day).Aggregate(TimeSpan.Zero, (sum, p) => sum + p.Time);

    protected override void Dispose(bool disposing)
    {
        if (disposing)
            _timer.Dispose();
        base.Dispose(disposing);
    }

    /// <summary>Opens the day; returns true if a session was deleted from it.</summary>
    public static bool Show(IWin32Window owner, ITrackerHost host, Theme theme, Fonts fonts, DateTime day, string? game)
    {
        using var dialog = new DayDetailsDialog(host, theme, fonts, day, game);
        dialog.ShowOver(owner);
        return dialog.Changed;
    }
}
