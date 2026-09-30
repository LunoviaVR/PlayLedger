use playtime_core::dashboard::SessionView;
use playtime_core::ipc::{self, DashboardSnapshot, Event, Request, Response};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

type Outcome<T = ()> = Result<T, String>;

const GAME: &str = "Smoke Game";
const DASHBOARD_EXE: &str = "PlaytimeTracker.Dashboard.exe";

fn check(condition: bool, message: &str) -> Outcome {
    if condition {
        Ok(())
    } else {
        Err(message.to_string())
    }
}

/// Waits for `probe` to give a value, trying every half second.
fn wait_for<T>(what: &str, timeout: Duration, mut probe: impl FnMut() -> Option<T>) -> Outcome<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = probe() {
            return Ok(value);
        }
        if Instant::now() > deadline {
            return Err(format!("timed out waiting for {what}"));
        }
        sleep(Duration::from_millis(500));
    }
}

/// One connection to the tracker's pipe (the same checks as the dashboard: current user, the real tracker).
struct Client {
    reader: BufReader<File>,
    writer: File,
}

impl Client {
    fn connect() -> Outcome<Self> {
        let pipe = playtime_windows::pipe::connect().map_err(|e| format!("connect: {e}"))?;
        let writer = pipe.try_clone().map_err(|e| e.to_string())?;
        Ok(Self {
            reader: BufReader::new(pipe),
            writer,
        })
    }

    fn receive(&mut self) -> Outcome<Response> {
        let line = ipc::read_line(&mut self.reader)
            .map_err(|e| e.to_string())?
            .ok_or("the tracker closed the connection")?;
        serde_json::from_str(&line).map_err(|e| format!("unreadable answer: {e}"))
    }

    fn request(&mut self, request: &Request) -> Outcome<Response> {
        ipc::write_message(&mut self.writer, request).map_err(|e| e.to_string())?;
        loop {
            match self.receive()? {
                Response::Event { .. } => continue,
                Response::Error { message } => {
                    return Err(format!("the tracker refused: {message}"))
                }
                other => return Ok(other),
            }
        }
    }

    fn snapshot(&mut self) -> Outcome<DashboardSnapshot> {
        match self.request(&Request::GetDashboard)? {
            Response::Dashboard { snapshot } => Ok(*snapshot),
            other => Err(format!("unexpected answer {other:?}")),
        }
    }

    fn settings(&mut self) -> Outcome<(playtime_core::settings::Settings, bool)> {
        match self.request(&Request::GetSettings)? {
            Response::Settings {
                settings,
                is_installed_copy,
                ..
            } => Ok((*settings, is_installed_copy)),
            other => Err(format!("unexpected answer {other:?}")),
        }
    }

    fn save(&mut self, settings: playtime_core::settings::Settings) -> Outcome {
        self.request(&Request::UpdateSettings {
            settings: Box::new(settings),
        })
        .map(|_| ())
    }
}

struct Test {
    install: PathBuf,
    screenshots: Option<PathBuf>,
    tracker: Option<Child>,
    client: Option<Client>,
    events: Arc<Mutex<Vec<Event>>>,
    games_root: PathBuf,
    game: Option<Child>,
    data: PathBuf,
}

impl Test {
    fn tracker_exe(&self) -> PathBuf {
        self.install.join("playtime-tracker.exe")
    }

    fn dashboard_exe(&self) -> PathBuf {
        self.install.join("Dashboard").join(DASHBOARD_EXE)
    }

    fn client(&mut self) -> Outcome<&mut Client> {
        self.client
            .as_mut()
            .ok_or_else(|| "not connected".to_string())
    }

    fn start_tracker(&mut self) -> Outcome {
        let exe = self.tracker_exe();
        check(exe.is_file(), &format!("{} isn't installed", exe.display()))?;
        self.tracker = Some(
            Command::new(&exe)
                .arg("--startup")
                .spawn()
                .map_err(|e| format!("the tracker didn't start: {e}"))?,
        );
        let mut last_error = String::new();
        let mut client = wait_for("the tracker's pipe", Duration::from_secs(30), || {
            Client::connect().map_err(|e| last_error = e).ok()
        })
        .map_err(|e| format!("{e} (last error: {last_error})"))?;
        match client.request(&Request::Hello {
            protocol: ipc::PROTOCOL_VERSION,
        })? {
            Response::Hello { version, .. } => println!("      tracker {version}"),
            other => return Err(format!("unexpected hello {other:?}")),
        }
        self.client = Some(client);
        // Events come over their own connection, as in the dashboard.
        let events = self.events.clone();
        std::thread::spawn(move || {
            let Ok(mut connection) = Client::connect() else {
                return;
            };
            if ipc::write_message(&mut connection.writer, &Request::Subscribe).is_err() {
                return;
            }
            while let Ok(response) = connection.receive() {
                if let Response::Event { event } = response {
                    if let Ok(mut list) = events.lock() {
                        list.push(event);
                    }
                }
            }
        });
        Ok(())
    }

    fn stop_tracker(&mut self) -> Outcome {
        self.client = None;
        let status = Command::new(self.tracker_exe())
            .arg("--exit")
            .status()
            .map_err(|e| format!("couldn't run --exit: {e}"))?;
        check(status.success(), "--exit failed")?;
        let mut tracker = self.tracker.take().ok_or("the tracker wasn't started")?;
        wait_for("the tracker to exit", Duration::from_secs(30), || {
            tracker.try_wait().ok().flatten()
        })?;
        Ok(())
    }

    fn seen(&self, matches: impl Fn(&Event) -> bool) -> Option<()> {
        self.events
            .lock()
            .ok()
            .and_then(|list| list.iter().any(matches).then_some(()))
    }
}

/// The folder's full long path. The temp folder often comes as an 8.3 short name (`C:\Users\RUNNER~1\…`), which
/// wouldn't match the long path Windows reports for the running game.
fn long_path(dir: &Path) -> PathBuf {
    let _ = std::fs::create_dir_all(dir);
    match std::fs::canonicalize(dir) {
        Ok(full) => {
            let text = full.to_string_lossy();
            PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
        }
        Err(_) => dir.to_path_buf(),
    }
}

fn run_step(failed: &mut bool, name: &str, step: impl FnOnce() -> Outcome) {
    if *failed {
        return;
    }
    let clock = Instant::now();
    match step() {
        Ok(()) => println!("PASS  {name} ({:.1}s)", clock.elapsed().as_secs_f32()),
        Err(message) => {
            *failed = true;
            println!("FAIL  {name}: {message}");
        }
    }
}

pub fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(install) = args.get(1) else {
        eprintln!("usage: playtime-smoke-test <install folder> [screenshot folder]");
        return ExitCode::from(2);
    };
    let absolute = |p: &str| std::path::absolute(p).unwrap_or_else(|_| PathBuf::from(p));
    let Some(documents) = playtime_windows::folders::documents() else {
        eprintln!("no Documents folder");
        return ExitCode::from(2);
    };
    let mut test = Test {
        install: absolute(install),
        screenshots: args.get(2).map(|p| absolute(p)),
        tracker: None,
        client: None,
        events: Arc::default(),
        games_root: long_path(&std::env::temp_dir().join("PlaytimeTracker-SmokeGames")),
        game: None,
        data: playtime_windows::folders::data_folder(&documents),
    };
    let mut failed = false;
    let f = &mut failed;
    let t = &mut test;

    run_step(
        f,
        "the exes carry their name, version and icon (Task Manager shows these)",
        || {
            for exe in [t.tracker_exe(), t.dashboard_exe()] {
                let info = native::version_info(&exe);
                check(
                    info.get("FileDescription").map(String::as_str) == Some("Playtime Tracker"),
                    &format!(
                        "{}: FileDescription is {:?}",
                        exe.display(),
                        info.get("FileDescription")
                    ),
                )?;
                check(
                    info.get("ProductName").map(String::as_str) == Some("Playtime Tracker"),
                    &format!(
                        "{}: ProductName is {:?}",
                        exe.display(),
                        info.get("ProductName")
                    ),
                )?;
                check(
                    info.get("ProductVersion").is_some_and(|v| !v.is_empty()),
                    &format!("{}: no ProductVersion", exe.display()),
                )?;
                check(
                    native::has_icon(&exe),
                    &format!("{}: no icon", exe.display()),
                )?;
            }
            Ok(())
        },
    );

    run_step(f, "tracker starts and answers over its pipe", || {
        t.start_tracker()
    });

    run_step(
        f,
        "settings round-trip (fast polling, a game folder)",
        || {
            // The folder must exist when it's added: discovery skips folders that don't.
            std::fs::create_dir_all(t.games_root.join(GAME)).map_err(|e| e.to_string())?;
            let root = t.games_root.to_string_lossy().into_owned();
            let client = t.client()?;
            let (mut settings, installed) = client.settings()?;
            check(
                installed,
                "the installed tracker doesn't recognise itself as installed",
            )?;
            settings.poll_interval_seconds = 1;
            settings.grace_period_seconds = 0;
            settings.minimum_session_seconds = 0;
            if !settings.extra_game_folders.contains(&root) {
                settings.extra_game_folders.push(root.clone());
            }
            client.save(settings)?;
            let (saved, _) = client.settings()?;
            check(
                saved.poll_interval_seconds == 1 && saved.extra_game_folders.contains(&root),
                "settings weren't saved",
            )
        },
    );

    run_step(f, "a running game is tracked live", || {
        // Any real program in a game folder counts; a copy of ping.exe waits quietly for two minutes.
        let exe = t.games_root.join(GAME).join("smoke-game.exe");
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        std::fs::copy(Path::new(&system).join("System32").join("PING.EXE"), &exe)
            .map_err(|e| format!("copying ping.exe: {e}"))?;
        t.game = Some(
            Command::new(&exe)
                .args(["-n", "120", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .spawn()
                .map_err(|e| format!("couldn't start the test game: {e}"))?,
        );
        let client = t.client()?;
        wait_for(
            "the game to show as playing",
            Duration::from_secs(40),
            || {
                client.snapshot().ok().and_then(|s| {
                    s.model
                        .live
                        .iter()
                        .find(|l| l.game == GAME && l.is_live)
                        .cloned()
                })
            },
        )
        .map(|_| ())
    });

    run_step(f, "closing the game records a session", || {
        if let Some(mut game) = t.game.take() {
            let _ = game.kill();
            let _ = game.wait();
        }
        let client = t.client()?;
        let session: SessionView =
            wait_for("the finished session", Duration::from_secs(30), || {
                client.snapshot().ok().and_then(|s| {
                    s.model
                        .sessions
                        .into_iter()
                        .find(|x| x.game == GAME && !x.is_live)
                })
            })?;
        check(
            session
                .executable
                .as_deref()
                .is_some_and(|e| e.to_ascii_lowercase().ends_with("smoke-game.exe")),
            "wrong executable recorded",
        )?;
        wait_for("the sessionStarted event", Duration::from_secs(10), || {
            t.seen(|e| matches!(e, Event::SessionStarted { game } if game == GAME))
        })?;
        wait_for("the sessionEnded event", Duration::from_secs(10), || {
            t.seen(|e| matches!(e, Event::SessionEnded { session } if session.game == GAME))
        })
    });

    run_step(
        f,
        "history is protected on disk and reports are read-only",
        || {
            let sessions = t.data.join("sessions.dat");
            let bytes = std::fs::read(&sessions).map_err(|e| format!("sessions.dat: {e}"))?;
            check(
                bytes.starts_with(b"PTDATA2\n"),
                "sessions.dat isn't in the protected format",
            )?;
            let stats = t.data.join("Game Stats.txt");
            let text =
                std::fs::read_to_string(&stats).map_err(|e| format!("Game Stats.txt: {e}"))?;
            check(text.contains(GAME), "Game Stats.txt doesn't list the game")?;
            let readonly = std::fs::metadata(&stats)
                .map(|m| m.permissions().readonly())
                .unwrap_or(false);
            check(readonly, "Game Stats.txt isn't read-only")?;
            let csv = std::fs::read_to_string(t.data.join("Sessions.csv")).unwrap_or_default();
            check(csv.contains(GAME), "Sessions.csv doesn't list the game")?;
            // While the tracker runs, other programs can't change the history.
            let writable = std::fs::OpenOptions::new()
                .write(true)
                .open(&sessions)
                .is_ok();
            check(
                !writable,
                "sessions.dat could be opened for writing while the tracker runs",
            )
        },
    );

    run_step(f, "the tracker exits cleanly with --exit", || {
        t.stop_tracker()
    });

    run_step(f, "the history survives a restart", || {
        t.start_tracker()?;
        let s = t.client()?.snapshot()?;
        check(
            s.model.sessions.iter().any(|x| x.game == GAME),
            "the session was lost",
        )?;
        check(
            s.model.games.iter().any(|g| g.name == GAME),
            "the game was lost",
        )
    });

    if let Some(screenshots) = t.screenshots.clone() {
        run_step(
            f,
            "dashboard pages render at several sizes (screenshots)",
            || {
                let exe = t.dashboard_exe();
                check(exe.is_file(), &format!("{} isn't installed", exe.display()))?;
                std::fs::create_dir_all(&screenshots).map_err(|e| e.to_string())?;
                for page in ["overview", "games", "history", "statistics", "settings"] {
                    let window = native::DashboardWindow::open(&exe, page)?;
                    for (name, width, height) in
                        [("", 1180, 800), ("-narrow", 520, 700), ("-wide", 1800, 900)]
                    {
                        window.resize(width, height);
                        sleep(Duration::from_secs(3));
                        check(
                            window.alive(),
                            &format!("the dashboard closed on the {page} page"),
                        )?;
                        window.capture(&screenshots.join(format!("{page}{name}.png")))?;
                    }
                    window.close();
                }
                Ok(())
            },
        );

        run_step(
            f,
            "the accent colour setting changes the dashboard's colours",
            || {
                // Rose is far from any Windows default accent, so its shades on screen can only come from the setting.
                let client = t.client()?;
                let (mut settings, _) = client.settings()?;
                let before = settings.accent_color.clone();
                settings.accent_color = "rose".into();
                client.save(settings.clone())?;
                let result = (|| {
                    let window = native::DashboardWindow::open(&t.dashboard_exe(), "settings")?;
                    window.resize(1180, 800);
                    sleep(Duration::from_secs(4));
                    let path = screenshots.join("accent.png");
                    let pixels = window.capture(&path)?;
                    window.close();
                    // The accent on light surfaces is WinUI's "dark 1" shade, on dark ones "light 2".
                    let rose = (0xf4u8, 0x3fu8, 0x5eu8);
                    let mix = |target: u8, amount: f32, c: u8| {
                        (c as f32 + (target as f32 - c as f32) * amount).round() as u8
                    };
                    let shades = [
                        (
                            mix(0, 0.2, rose.0),
                            mix(0, 0.2, rose.1),
                            mix(0, 0.2, rose.2),
                        ),
                        (
                            mix(255, 0.45, rose.0),
                            mix(255, 0.45, rose.1),
                            mix(255, 0.45, rose.2),
                        ),
                    ];
                    let count = pixels
                        .chunks_exact(4)
                        .filter(|p| {
                            shades.iter().any(|s| {
                                p[0].abs_diff(s.0) <= 12
                                    && p[1].abs_diff(s.1) <= 12
                                    && p[2].abs_diff(s.2) <= 12
                            })
                        })
                        .count();
                    println!("      {count} pixels in the rose accent");
                    check(
                        count >= 200,
                        "the dashboard doesn't show the chosen accent colour",
                    )
                })();
                settings.accent_color = before;
                t.client()?.save(settings)?;
                result
            },
        );
    }

    run_step(f, "a session can be deleted", || {
        let client = t.client()?;
        let s = client.snapshot()?;
        let session = s
            .model
            .sessions
            .iter()
            .find(|x| x.game == GAME && !x.is_live)
            .cloned()
            .ok_or("no session to delete")?;
        client.request(&Request::DeleteSession {
            game: session.game.clone(),
            start: session.start,
        })?;
        let after = client.snapshot()?;
        check(
            !after
                .model
                .sessions
                .iter()
                .any(|x| x.game == GAME && x.start == session.start),
            "the session is still there",
        )
    });

    run_step(f, "the tracker exits cleanly again", || t.stop_tracker());

    if let Some(mut game) = test.game.take() {
        let _ = game.kill();
    }
    if failed {
        let local = playtime_windows::folders::local_app_data().unwrap_or_default();
        for path in [
            test.data.join("errors.log"),
            test.data.join("migration.log"),
            local.join("Playtime Tracker").join("dashboard-errors.log"),
        ] {
            if let Ok(text) = std::fs::read_to_string(&path) {
                println!("\n--- {} ---\n{text}", path.display());
            }
        }
        println!("\nSMOKE TEST FAILED");
        ExitCode::FAILURE
    } else {
        println!("\nSMOKE TEST PASSED");
        ExitCode::SUCCESS
    }
}

mod native {
    use super::{check, wait_for, Outcome, DASHBOARD_EXE};
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::Duration;
    use windows::core::{BOOL, HSTRING, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        PROCESS_TERMINATE,
    };
    use windows::Win32::UI::Shell::ExtractIconExW;
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, EnumWindows, GetSystemMetrics, GetWindowRect, GetWindowThreadProcessId,
        IsWindowVisible, SetForegroundWindow, SetWindowPos, SM_CXSCREEN, SM_CYSCREEN, SWP_NOZORDER,
    };

    /// The exe's version strings (FileDescription, ProductName, ProductVersion, …).
    pub fn version_info(exe: &Path) -> HashMap<String, String> {
        let mut found = HashMap::new();
        let path = HSTRING::from(exe.as_os_str());
        // SAFETY: the buffer is as large as Windows asked for and outlives every pointer read from it.
        unsafe {
            let size = GetFileVersionInfoSizeW(&path, None);
            if size == 0 {
                return found;
            }
            let mut buffer = vec![0u8; size as usize];
            if GetFileVersionInfoW(&path, None, size, buffer.as_mut_ptr().cast()).is_err() {
                return found;
            }
            for key in ["FileDescription", "ProductName", "ProductVersion"] {
                let query = HSTRING::from(format!("\\StringFileInfo\\040904b0\\{key}"));
                let mut value: *mut core::ffi::c_void = std::ptr::null_mut();
                let mut length = 0u32;
                if VerQueryValueW(buffer.as_ptr().cast(), &query, &mut value, &mut length).as_bool()
                    && !value.is_null()
                    && length > 0
                {
                    let text = std::slice::from_raw_parts(value as *const u16, length as usize - 1);
                    found.insert(key.to_string(), String::from_utf16_lossy(text));
                }
            }
        }
        found
    }

    pub fn has_icon(exe: &Path) -> bool {
        let path = HSTRING::from(exe.as_os_str());
        let mut large = windows::Win32::UI::WindowsAndMessaging::HICON::default();
        // SAFETY: one icon is asked for into `large`, which is destroyed afterwards.
        unsafe {
            let count = ExtractIconExW(&path, 0, Some(&mut large), None, 1);
            if !large.is_invalid() {
                let _ = DestroyIcon(large);
            }
            count > 0
        }
    }

    /// The image path of a process.
    fn image_of(pid: u32) -> Option<String> {
        // SAFETY: the handle is closed before returning; the buffer outlives the call.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buffer = [0u16; 1024];
            let mut size = buffer.len() as u32;
            let result = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut size,
            );
            let _ = CloseHandle(process);
            result.ok()?;
            Some(String::from_utf16_lossy(&buffer[..size as usize]))
        }
    }

    /// The visible dashboard window, whichever process shows it (the dashboard restarts itself on the software
    /// renderer if the GPU one can't run, as on CI machines without a graphics card).
    fn find_dashboard() -> Option<(HWND, u32)> {
        unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> BOOL {
            // SAFETY: `found` is the address of the Option below, alive for the whole enumeration.
            let found = unsafe { &mut *(found.0 as *mut Option<(HWND, u32)>) };
            let mut pid = 0u32;
            // SAFETY: plain queries on a window handle Windows gave us.
            unsafe {
                if !IsWindowVisible(hwnd).as_bool() {
                    return BOOL(1);
                }
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
            }
            if image_of(pid).is_some_and(|p| {
                p.to_ascii_lowercase()
                    .ends_with(&DASHBOARD_EXE.to_ascii_lowercase())
            }) {
                *found = Some((hwnd, pid));
                return BOOL(0);
            }
            BOOL(1)
        }
        let mut found: Option<(HWND, u32)> = None;
        // SAFETY: the callback only runs during this call, while `found` is alive.
        unsafe {
            let _ = EnumWindows(Some(visit), LPARAM(&mut found as *mut _ as isize));
        }
        found
    }

    pub struct DashboardWindow {
        hwnd: HWND,
        pid: u32,
    }

    impl DashboardWindow {
        pub fn open(exe: &Path, page: &str) -> Outcome<Self> {
            std::process::Command::new(exe)
                .args(["--page", page])
                .spawn()
                .map_err(|e| format!("the dashboard didn't start: {e}"))?;
            let (hwnd, pid) = wait_for(
                &format!("the {page} window"),
                Duration::from_secs(45),
                find_dashboard,
            )?;
            // Connect, load data and artwork, settle.
            std::thread::sleep(Duration::from_secs(5));
            Ok(Self { hwnd, pid })
        }

        pub fn alive(&self) -> bool {
            find_dashboard().is_some_and(|(_, pid)| pid == self.pid)
        }

        /// Sets the window's size, as far as the screen allows.
        pub fn resize(&self, width: i32, height: i32) {
            // SAFETY: plain calls on the dashboard's window.
            unsafe {
                // Windows only draws the part of a window that's on screen (CI's is 1024 × 768), so the window is
                // kept within it.
                let width = width.min(GetSystemMetrics(SM_CXSCREEN).max(500));
                let height = height.min((GetSystemMetrics(SM_CYSCREEN) - 48).max(500));
                let _ = SetWindowPos(self.hwnd, None, 0, 0, width, height, SWP_NOZORDER);
                let _ = SetForegroundWindow(self.hwnd);
            }
        }

        /// Saves the window as a PNG; returns its RGBA pixels.
        pub fn capture(&self, path: &Path) -> Outcome<Vec<u8>> {
            let mut rect = RECT::default();
            // SAFETY: the DCs and bitmap are released below; the pixel pointer is read only while the bitmap lives.
            let pixels = unsafe {
                GetWindowRect(self.hwnd, &mut rect).map_err(|e| e.to_string())?;
                let (w, h) = (
                    (rect.right - rect.left).max(1),
                    (rect.bottom - rect.top).max(1),
                );
                let screen = GetDC(None);
                let dc = CreateCompatibleDC(Some(screen));
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w,
                        biHeight: -h, // top-down
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
                let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                    .map_err(|e| e.to_string())?;
                let old = SelectObject(dc, bitmap.into());
                // PW_RENDERFULLCONTENT: include what the GPU drew.
                let printed = PrintWindow(self.hwnd, dc, PRINT_WINDOW_FLAGS(2)).as_bool();
                let bgra = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize);
                let rgba: Vec<u8> = bgra
                    .chunks_exact(4)
                    .flat_map(|p| [p[2], p[1], p[0], 255])
                    .collect();
                SelectObject(dc, old);
                let _ = DeleteObject(bitmap.into());
                let _ = DeleteDC(dc);
                ReleaseDC(None, screen);
                check(printed, "PrintWindow failed")?;
                let png = playtime_artwork::png::encode_rgba(w as u32, h as u32, &rgba)
                    .ok_or("PNG encoding failed")?;
                std::fs::write(path, png).map_err(|e| format!("{}: {e}", path.display()))?;
                rgba
            };
            Ok(pixels)
        }

        /// Ends the dashboard and waits until its process is gone: until then it still holds the single-instance
        /// lock, and the next dashboard would hand over to it and exit.
        pub fn close(self) {
            // SAFETY: the handle is closed after use.
            unsafe {
                if let Ok(process) =
                    OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, self.pid)
                {
                    let _ = TerminateProcess(process, 0);
                    let _ = WaitForSingleObject(process, 15_000);
                    let _ = CloseHandle(process);
                }
            }
            let _ = wait_for("the dashboard to close", Duration::from_secs(15), || {
                find_dashboard().is_none().then_some(())
            });
        }
    }
}
