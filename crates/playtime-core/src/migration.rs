//! One-time data migrations, run before the tracker opens its files. Every step follows *verify, then switch*:
//! nothing old is removed until its replacement has been written and read back, and anything unreadable is kept
//! under a new name rather than deleted. Each step is recorded in `migration.log` in the data folder.
//!
//! - The data folder moves from `Documents\Game Session Tracker` to `Documents\Playtime Tracker` (1.x name).
//! - 1.x plain-text `sessions.json` / `settings.json` become protected `sessions.dat` / `settings.dat`.
//! - Before a version change (and before the Rust tracker first touches the files), the protected files are copied
//!   to `Backups\<date> before <version>\` and the copies compared byte for byte.

use crate::model::TrackerData;
use crate::protected::{self, backup_path, DataProtector, GenerationStore, Purpose};
use crate::settings::Settings;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const DATA_FOLDER_NAME: &str = "Playtime Tracker";
pub const LEGACY_DATA_FOLDER_NAME: &str = "Game Session Tracker";
pub const MIGRATION_LOG: &str = "migration.log";
pub const BACKUPS_FOLDER: &str = "Backups";
/// Written once the Rust tracker has made its first-run backup.
const FIRST_RUN_MARKER: &str = "rust-tracker-first-run.txt";
/// Older automatic backups beyond this many are removed (newest kept).
pub const KEEP_BACKUPS: usize = 10;

/// What the migrations did, for the log and (for `notices`) the user.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub data_folder: PathBuf,
    /// Plain-language messages worth showing (e.g. an old file couldn't be read and was kept aside).
    pub notices: Vec<String>,
    /// Everything that happened, also appended to `migration.log`.
    pub log: Vec<String>,
}

impl MigrationReport {
    fn note(&mut self, line: impl Into<String>) {
        self.log.push(line.into());
    }
}

/// Appends lines to `migration.log`.
pub fn write_log(folder: &Path, stamp: &str, lines: &[String]) -> io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join(MIGRATION_LOG))?;
    for line in lines {
        writeln!(file, "{stamp}  {line}")?;
    }
    Ok(())
}

/// Picks the data folder under `documents`, moving the 1.x folder to the current name if only it exists.
/// If the move fails (a file is open, or the folder is syncing), the old folder is used as is and the move is
/// retried next time, so the history is never left behind.
pub fn resolve_data_folder(documents: &Path, report: &mut MigrationReport) -> PathBuf {
    let current = documents.join(DATA_FOLDER_NAME);
    let legacy = documents.join(LEGACY_DATA_FOLDER_NAME);
    if current.exists() || !legacy.exists() {
        return current;
    }
    match fs::rename(&legacy, &current) {
        Ok(()) => {
            report.note(format!(
                "Moved the data folder from \"{LEGACY_DATA_FOLDER_NAME}\" to \"{DATA_FOLDER_NAME}\"."
            ));
            current
        }
        Err(e) => {
            report.note(format!(
                "Could not move \"{LEGACY_DATA_FOLDER_NAME}\" yet ({e}); using it as is and trying again next time."
            ));
            legacy
        }
    }
}

fn unreadable_name(path: &Path, stamp: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!("{stem}.unreadable-{stamp}.json"))
}

/// What every import step needs.
struct ImportContext<'a> {
    protector: &'a dyn DataProtector,
    generations: &'a dyn GenerationStore,
    folder: &'a Path,
    stamp: &'a str,
}

/// Imports a 1.x plain-text JSON file into its protected replacement, if the protected file doesn't exist yet.
/// The plain file is deleted only after the protected copy reads back equal to what was imported.
fn import_legacy<T: PartialEq>(
    ctx: &ImportContext<'_>,
    purpose: Purpose,
    names: (&str, &str),
    parse: impl Fn(&str) -> Result<T, serde_json::Error>,
    serialize: impl Fn(&T) -> Result<String, serde_json::Error>,
    report: &mut MigrationReport,
) {
    let ImportContext {
        protector,
        generations,
        folder,
        stamp,
    } = *ctx;
    let (legacy_name, protected_name) = names;
    let legacy = folder.join(legacy_name);
    let target = folder.join(protected_name);
    if !legacy.exists() || target.exists() || backup_path(&target).exists() {
        return;
    }
    let value = match fs::read_to_string(&legacy)
        .map_err(|e| e.to_string())
        .and_then(|text| parse(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string()))
    {
        Ok(value) => value,
        Err(e) => {
            // Never throw away someone's history: keep the unreadable file under a new name.
            let aside = unreadable_name(&legacy, stamp);
            match fs::rename(&legacy, &aside) {
                Ok(()) => {
                    let kept = aside
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    report.note(format!(
                        "{legacy_name} couldn't be read ({e}); kept it as {kept}."
                    ));
                    report.notices.push(format!(
                        "Your old {legacy_name} couldn't be read, so it was kept as {kept} and a new one was started."
                    ));
                }
                Err(rename) => report.note(format!(
                    "{legacy_name} couldn't be read ({e}) or set aside ({rename})."
                )),
            }
            return;
        }
    };
    let result = serialize(&value)
        .map_err(|e| e.to_string())
        .and_then(|json| {
            protected::save(protector, generations, purpose, &target, 0, &json)
                .map_err(|e| e.to_string())
        })
        .and_then(|_| protected::read_file(protector, purpose, &target).map_err(|e| e.to_string()))
        .and_then(|(contents, _)| parse(&contents).map_err(|e| e.to_string()));
    match result {
        Ok(read_back) if read_back == value => match fs::remove_file(&legacy) {
            Ok(()) => report.note(format!("Imported {legacy_name} into {protected_name} and removed the plain-text copy.")),
            Err(e) => report.note(format!("Imported {legacy_name} into {protected_name}; the plain copy couldn't be removed yet ({e}).")),
        },
        Ok(_) => report.note(format!("{protected_name} didn't read back the same as {legacy_name}; kept {legacy_name}.")),
        Err(e) => report.note(format!("Couldn't import {legacy_name} ({e}); kept it for the next try.")),
    }
}

/// Imports 1.x `sessions.json` and `settings.json`.
pub fn import_legacy_files(
    protector: &dyn DataProtector,
    generations: &dyn GenerationStore,
    folder: &Path,
    stamp: &str,
    report: &mut MigrationReport,
) {
    let ctx = ImportContext {
        protector,
        generations,
        folder,
        stamp,
    };
    import_legacy(
        &ctx,
        Purpose::Sessions,
        ("sessions.json", "sessions.dat"),
        TrackerData::from_json,
        TrackerData::to_json,
        report,
    );
    import_legacy(
        &ctx,
        Purpose::Settings,
        ("settings.json", "settings.dat"),
        Settings::from_json,
        Settings::to_json,
        report,
    );
}

/// Copies the protected files into `Backups\<stamp> before <reason>\`, verifies the copies byte for byte, and
/// prunes old automatic backups beyond [`KEEP_BACKUPS`]. Returns the backup folder.
pub fn backup(
    folder: &Path,
    stamp: &str,
    reason: &str,
    report: &mut MigrationReport,
) -> io::Result<Option<PathBuf>> {
    let files: Vec<PathBuf> = ["sessions.dat", "settings.dat"]
        .iter()
        .flat_map(|name| {
            let path = folder.join(name);
            [backup_path(&path), path]
        })
        .filter(|p| p.is_file())
        .collect();
    if files.is_empty() {
        return Ok(None);
    }
    let safe_reason: String = reason
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == ' ' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let backups = folder.join(BACKUPS_FOLDER);
    let target = backups.join(format!("{stamp} before {safe_reason}"));
    fs::create_dir_all(&target)?;
    for file in &files {
        let Some(name) = file.file_name() else {
            continue;
        };
        let copy = target.join(name);
        fs::copy(file, &copy)?;
        if fs::read(file)? != fs::read(&copy)? {
            return Err(io::Error::other(format!(
                "the backup of {} doesn't match",
                name.to_string_lossy()
            )));
        }
    }
    report.note(format!(
        "Backed up {} file(s) to {}\\{}.",
        files.len(),
        BACKUPS_FOLDER,
        target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    prune_backups(&backups, report);
    Ok(Some(target))
}

/// Removes the oldest automatic backups beyond [`KEEP_BACKUPS`]. Only folders named like ours (`<stamp> before …`)
/// are considered; anything else in `Backups` is left alone.
fn prune_backups(backups: &Path, report: &mut MigrationReport) {
    let Ok(entries) = fs::read_dir(backups) else {
        return;
    };
    let mut ours: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.len() > 16
                    && n.as_bytes()[..8].iter().all(u8::is_ascii_digit)
                    && n.contains(" before ")
            })
        })
        .collect();
    ours.sort(); // names start with the timestamp, so this is oldest first
    while ours.len() > KEEP_BACKUPS {
        let oldest = ours.remove(0);
        match fs::remove_dir_all(&oldest) {
            Ok(()) => report.note(format!("Removed the old backup {}.", oldest.display())),
            Err(e) => {
                report.note(format!(
                    "Couldn't remove the old backup {} ({e}).",
                    oldest.display()
                ));
                break;
            }
        }
    }
}

/// Everything before the tracker opens the data folder: folder move, legacy import, and a verified backup when
/// the version changed or the Rust tracker runs for the first time. `last_run_version` is from the settings of the
/// previous run (either app writes it); `None` if unknown.
pub fn run(
    protector: &dyn DataProtector,
    generations: &dyn GenerationStore,
    documents: &Path,
    current_version: &str,
    last_run_version: impl FnOnce(&Path) -> Option<String>,
    stamp: &str,
) -> MigrationReport {
    let mut report = MigrationReport::default();
    let folder = resolve_data_folder(documents, &mut report);
    if let Err(e) = fs::create_dir_all(&folder) {
        report.note(format!("Couldn't create the data folder ({e})."));
    }
    import_legacy_files(protector, generations, &folder, stamp, &mut report);

    let first_rust_run = !folder.join(BACKUPS_FOLDER).join(FIRST_RUN_MARKER).exists();
    let previous = last_run_version(&folder);
    let version_changed = previous
        .as_deref()
        .is_some_and(|v| !v.is_empty() && v != current_version);
    if first_rust_run || version_changed {
        let reason = match (&previous, first_rust_run) {
            (Some(v), false) if !v.is_empty() => format!("{current_version} (from {v})"),
            _ => format!("{current_version} (first run of the new tracker)"),
        };
        match backup(&folder, stamp, &reason, &mut report) {
            Ok(_) => {
                if first_rust_run {
                    let marker = folder.join(BACKUPS_FOLDER).join(FIRST_RUN_MARKER);
                    let _ = fs::create_dir_all(folder.join(BACKUPS_FOLDER));
                    let _ = fs::write(
                        marker,
                        format!("The Rust tracker {current_version} first ran on {stamp}; a backup was made before it did.\r\n"),
                    );
                }
            }
            Err(e) => report.note(format!(
                "Couldn't back up the data before this version ({e})."
            )),
        }
    }
    report.data_folder = folder;
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protected::testing::{FakeProtector, MemoryGenerations};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("pt-migration-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("temp dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const SESSIONS_1X: &str = r#"{"sessions":[{"game":"Elden Ring","start":"2026-09-27T20:00:00+00:00","end":"2026-09-27T21:35:00+00:00","executable":"D:\\eldenring.exe"}],"active":[]}"#;

    #[test]
    fn moves_the_old_folder_and_imports_1x_files() {
        let docs = TempDir::new("import");
        let legacy = docs.0.join(LEGACY_DATA_FOLDER_NAME);
        fs::create_dir_all(&legacy).expect("dir");
        fs::write(legacy.join("sessions.json"), SESSIONS_1X).expect("write");
        fs::write(legacy.join("settings.json"), r#"{"pollIntervalSeconds":7}"#).expect("write");
        let gens = MemoryGenerations::default();

        let report = run(
            &FakeProtector,
            &gens,
            &docs.0,
            "3.0.0",
            |_| None,
            "20260929-120000",
        );
        let folder = docs.0.join(DATA_FOLDER_NAME);
        assert_eq!(report.data_folder, folder);
        assert!(!legacy.exists());
        assert!(!folder.join("sessions.json").exists() && !folder.join("settings.json").exists());

        let (json, _) = protected::read_file(
            &FakeProtector,
            Purpose::Sessions,
            &folder.join("sessions.dat"),
        )
        .expect("verifies");
        assert_eq!(
            TrackerData::from_json(&json)
                .expect("parses")
                .sessions
                .len(),
            1
        );
        let (json, _) = protected::read_file(
            &FakeProtector,
            Purpose::Settings,
            &folder.join("settings.dat"),
        )
        .expect("verifies");
        assert_eq!(
            Settings::from_json(&json)
                .expect("parses")
                .poll_interval_seconds,
            7
        );

        // First run of the new tracker: a verified backup was made.
        let backups: Vec<_> = fs::read_dir(folder.join(BACKUPS_FOLDER))
            .expect("backups")
            .flatten()
            .collect();
        assert!(backups
            .iter()
            .any(|e| e.file_name().to_string_lossy().contains("before 3.0.0")));
        assert!(
            fs::read_to_string(folder.join(MIGRATION_LOG)).is_err(),
            "run() doesn't write the log itself"
        );
        assert!(report
            .log
            .iter()
            .any(|l| l.contains("Imported sessions.json")));

        // Nothing more to do on the next run of the same version.
        let again = run(
            &FakeProtector,
            &gens,
            &docs.0,
            "3.0.0",
            |_| Some("3.0.0".into()),
            "20260929-130000",
        );
        assert!(again.log.is_empty(), "{:?}", again.log);
    }

    #[test]
    fn unreadable_legacy_files_are_kept_aside() {
        let docs = TempDir::new("unreadable");
        let folder = docs.0.join(DATA_FOLDER_NAME);
        fs::create_dir_all(&folder).expect("dir");
        fs::write(folder.join("sessions.json"), "{ not json").expect("write");
        let mut report = MigrationReport::default();
        import_legacy_files(
            &FakeProtector,
            &MemoryGenerations::default(),
            &folder,
            "20260929-120000",
            &mut report,
        );
        assert!(folder
            .join("sessions.unreadable-20260929-120000.json")
            .exists());
        assert!(!folder.join("sessions.dat").exists());
        assert_eq!(report.notices.len(), 1);
    }

    #[test]
    fn existing_protected_data_wins_over_a_stray_json() {
        let docs = TempDir::new("stray");
        let folder = docs.0.join(DATA_FOLDER_NAME);
        fs::create_dir_all(&folder).expect("dir");
        let gens = MemoryGenerations::default();
        protected::save(
            &FakeProtector,
            &gens,
            Purpose::Sessions,
            &folder.join("sessions.dat"),
            0,
            r#"{"sessions":[],"active":[]}"#,
        )
        .expect("saved");
        fs::write(folder.join("sessions.json"), SESSIONS_1X).expect("write");
        let mut report = MigrationReport::default();
        import_legacy_files(&FakeProtector, &gens, &folder, "x", &mut report);
        assert!(
            folder.join("sessions.json").exists(),
            "left alone, never merged over newer data"
        );
        assert!(report.log.is_empty());
    }

    #[test]
    fn version_change_backs_up_and_prunes_only_our_folders() {
        let docs = TempDir::new("backup");
        let folder = docs.0.join(DATA_FOLDER_NAME);
        let backups = folder.join(BACKUPS_FOLDER);
        fs::create_dir_all(&backups).expect("dir");
        fs::write(backups.join(FIRST_RUN_MARKER), "done").expect("marker");
        fs::write(folder.join("sessions.dat"), b"protected bytes").expect("write");
        fs::create_dir_all(backups.join("my own stuff")).expect("user folder");
        for i in 0..KEEP_BACKUPS {
            fs::create_dir_all(backups.join(format!("2026010{}-000000 before 1.0.{i}", i % 10)))
                .expect("old");
        }

        let report = run(
            &FakeProtector,
            &MemoryGenerations::default(),
            &docs.0,
            "3.0.0",
            |_| Some("2.2.0".into()),
            "20260929-120000",
        );
        let backup = backups.join("20260929-120000 before 3.0.0 -from 2.2.0-");
        assert_eq!(
            fs::read(backup.join("sessions.dat")).expect("copied"),
            b"protected bytes"
        );
        let remaining = fs::read_dir(&backups)
            .expect("list")
            .flatten()
            .filter(|e| e.path().is_dir())
            .count();
        assert_eq!(
            remaining,
            KEEP_BACKUPS + 1,
            "ten automatic backups plus the user's own folder"
        );
        assert!(backups.join("my own stuff").exists());
        assert!(report
            .log
            .iter()
            .any(|l| l.contains("Removed the old backup")));
    }
}
