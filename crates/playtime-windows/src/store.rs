//! The data folder: protected `sessions.dat` / `settings.dat` (+ `.bak`), the read-only
//! `Game Stats.txt` and `Sessions.csv` reports, and `errors.log`. Files are held open read-only while the tracker
//! runs and released only around its own saves.

use crate::dpapi::Dpapi;
use crate::folders::{CSV_FILE, ERROR_LOG_FILE, SESSIONS_FILE, SETTINGS_FILE, STATS_FILE};
use crate::locks::FileLocks;
use crate::registry::RegistryGenerations;
use playtime_core::protected::{self, backup_path, Purpose, Recovery, StoreError};
use playtime_core::reports;
use playtime_core::{Settings, Timestamp, TrackerData};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// `errors.log` is rotated to `errors.log.old` past this size.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

pub struct Store {
    folder: PathBuf,
    locks: FileLocks,
    sessions_generation: u64,
    settings_generation: u64,
}

/// What opening the store found.
pub struct Opened {
    pub store: Store,
    pub data: TrackerData,
    pub settings: Settings,
    /// Messages for the user (e.g. "sessions.dat was changed outside PlayLedger…").
    pub warnings: Vec<String>,
    /// Settings didn't exist yet (first run) or had to be started fresh.
    pub settings_created: bool,
}

fn stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

fn warning(file: &str, recovery: &Recovery) -> Option<String> {
    let name = |p: &Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    match recovery {
        Recovery::None => None,
        Recovery::RestoredFromBackup { set_aside } => Some(format!(
            "{file} was changed outside PlayLedger. The changed file was kept as {}, and the last saved copy was restored.",
            name(set_aside)
        )),
        Recovery::StartedFresh { set_aside: Some(set_aside) } => Some(format!(
            "{file} was changed outside PlayLedger. The changed file was kept as {} and a new one was started.",
            name(set_aside)
        )),
        Recovery::StartedFresh { set_aside: None } => Some(format!(
            "{file} couldn't be verified (it comes from another Windows account or PC), so a new one was started."
        )),
        Recovery::RolledBackCopyReplaced { .. } => Some(format!(
            "An older copy of {file} was put back; the newer saved copy was restored."
        )),
        Recovery::NewerDataMissing { .. } => Some(format!(
            "An older copy of {file} was put back, and the newer data couldn't be found. The older copy is in use."
        )),
    }
}

impl Store {
    /// Opens (creating if needed) the data folder and loads both protected files with recovery.
    pub fn open(folder: &Path) -> Result<Opened, StoreError> {
        fs::create_dir_all(folder).map_err(|source| StoreError::Io {
            file: folder.display().to_string(),
            source,
        })?;
        let mut warnings = Vec::new();
        let now = stamp();

        let settings_path = folder.join(SETTINGS_FILE);
        let loaded = protected::load_with_recovery(
            &Dpapi,
            &RegistryGenerations,
            Purpose::Settings,
            &settings_path,
            Settings::from_json,
            &now,
        )?;
        warnings.extend(warning(SETTINGS_FILE, &loaded.recovery));
        let settings_created = loaded.value.is_none();
        let settings = loaded.value.unwrap_or_default();
        let settings_generation = loaded.generation;

        let sessions_path = folder.join(SESSIONS_FILE);
        let loaded = protected::load_with_recovery(
            &Dpapi,
            &RegistryGenerations,
            Purpose::Sessions,
            &sessions_path,
            TrackerData::from_json,
            &now,
        )?;
        warnings.extend(warning(SESSIONS_FILE, &loaded.recovery));
        let data = loaded.value.unwrap_or_default();

        let mut store = Store {
            folder: folder.to_path_buf(),
            locks: FileLocks::default(),
            sessions_generation: loaded.generation,
            settings_generation,
        };
        for warning in &warnings {
            store.log(warning);
        }
        store.hold_all();
        Ok(Opened {
            store,
            data,
            settings,
            warnings,
            settings_created,
        })
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    fn protected_paths(&self, file: &str) -> [PathBuf; 2] {
        let path = self.folder.join(file);
        [backup_path(&path), path]
    }

    fn hold_all(&mut self) {
        for file in [SESSIONS_FILE, SETTINGS_FILE] {
            for path in self.protected_paths(file) {
                self.locks.hold(&path);
            }
        }
        for file in [STATS_FILE, CSV_FILE] {
            self.locks.hold(&self.folder.join(file));
        }
    }

    fn save_protected(
        &mut self,
        purpose: Purpose,
        file: &str,
        contents: &str,
    ) -> Result<(), StoreError> {
        let [backup, path] = self.protected_paths(file);
        self.locks.release(&path);
        self.locks.release(&backup);
        let known = match purpose {
            Purpose::Sessions => self.sessions_generation,
            Purpose::Settings => self.settings_generation,
        };
        let result = protected::save(
            &Dpapi,
            &RegistryGenerations,
            purpose,
            &path,
            known,
            contents,
        );
        self.locks.hold(&path);
        self.locks.hold(&backup);
        let generation = result?;
        match purpose {
            Purpose::Sessions => self.sessions_generation = generation,
            Purpose::Settings => self.settings_generation = generation,
        }
        Ok(())
    }

    /// Saves the play history and regenerates the reports.
    pub fn save_sessions(&mut self, data: &TrackerData, now: Timestamp) -> Result<(), StoreError> {
        let json = data.to_json().map_err(|e| StoreError::Io {
            file: SESSIONS_FILE.into(),
            source: std::io::Error::other(e),
        })?;
        self.save_protected(Purpose::Sessions, SESSIONS_FILE, &json)?;
        if let Err(e) = self.write_reports(data, now) {
            self.log(&format!("Could not write the reports: {e}"));
        }
        Ok(())
    }

    pub fn save_settings(&mut self, settings: &Settings) -> Result<(), StoreError> {
        let json = settings.to_json().map_err(|e| StoreError::Io {
            file: SETTINGS_FILE.into(),
            source: std::io::Error::other(e),
        })?;
        self.save_protected(Purpose::Settings, SETTINGS_FILE, &json)
    }

    /// `Game Stats.txt` and `Sessions.csv`, read-only, with a UTF-8 BOM (as earlier versions wrote them).
    pub fn write_reports(&mut self, data: &TrackerData, now: Timestamp) -> std::io::Result<()> {
        let stats = reports::stats_text(&data.sessions, &data.active, now, &chrono::Local);
        self.write_read_only(STATS_FILE, &format!("\u{feff}{stats}"))?;
        let offset = *now.as_datetime().with_timezone(&chrono::Local).offset();
        // sessions_csv already starts with the BOM.
        self.write_read_only(CSV_FILE, &reports::sessions_csv(&data.sessions, offset))
    }

    fn write_read_only(&mut self, file: &str, text: &str) -> std::io::Result<()> {
        let path = self.folder.join(file);
        self.locks.release(&path);
        let result = (|| {
            if let Ok(meta) = fs::metadata(&path) {
                let mut permissions = meta.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                // Windows only: clears the read-only attribute
                permissions.set_readonly(false);
                fs::set_permissions(&path, permissions)?;
            }
            protected::write_atomic(&path, text.as_bytes(), None)?;
            let mut permissions = fs::metadata(&path)?.permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&path, permissions)
        })();
        self.locks.hold(&path);
        result
    }

    /// Appends to `errors.log` (rotated at 1 MB). Never contains secrets: callers log only messages they build.
    pub fn log(&self, message: &str) {
        log_to(&self.folder, message);
    }
}

pub fn log_to(folder: &Path, message: &str) {
    let path = folder.join(ERROR_LOG_FILE);
    if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let mut old = path.as_os_str().to_owned();
        old.push(".old");
        let _ = fs::rename(&path, PathBuf::from(old));
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            file,
            "{}  {message}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        );
    }
}
