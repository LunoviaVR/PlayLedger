//! The Windows "Open" and "Save As" dialogs, for picking artwork and saving an export. Each runs on its own thread,
//! so the caller's event loop is never re-entered while the dialog is up; the owner window is disabled meanwhile,
//! as with any modal dialog.

use std::path::PathBuf;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog,
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST,
    SIGDN_FILESYSPATH,
};

/// A file type in the dialog's list: its name ("Pictures") and patterns ("*.png;*.jpg").
pub struct Filter {
    pub name: &'static str,
    pub patterns: &'static str,
}

/// Asks for a file to open. `owner` is the window's handle (`HWND` as an integer), or 0 for none.
pub fn open(owner: isize, title: &str, filters: &[Filter]) -> Option<PathBuf> {
    let title = title.to_string();
    let filters = owned(filters);
    std::thread::spawn(move || {
        with_com(|| {
            // SAFETY: COM is initialised on this thread; the dialog is released before COM is.
            let dialog: IFileOpenDialog =
                unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }.ok()?;
            // SAFETY: plain calls on a live dialog.
            unsafe {
                let options = dialog.GetOptions().ok()?;
                dialog
                    .SetOptions(
                        options | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST,
                    )
                    .ok()?;
            }
            show(&dialog.into(), owner, &title, &filters)
        })
    })
    .join()
    .ok()
    .flatten()
}

/// Asks where to save a file, suggesting `name`; Windows asks before replacing an existing file.
pub fn save(
    owner: isize,
    title: &str,
    name: &str,
    extension: &str,
    filters: &[Filter],
) -> Option<PathBuf> {
    let (title, name, extension) = (title.to_string(), name.to_string(), extension.to_string());
    let filters = owned(filters);
    std::thread::spawn(move || {
        with_com(|| {
            // SAFETY: COM is initialised on this thread; the dialog is released before COM is.
            let dialog: IFileSaveDialog =
                unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER) }.ok()?;
            // SAFETY: plain calls on a live dialog with strings that outlive them.
            unsafe {
                let options = dialog.GetOptions().ok()?;
                dialog
                    .SetOptions(
                        options | FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT | FOS_PATHMUSTEXIST,
                    )
                    .ok()?;
                dialog.SetFileName(&HSTRING::from(name.as_str())).ok()?;
                dialog
                    .SetDefaultExtension(&HSTRING::from(extension.as_str()))
                    .ok()?;
            }
            show(&dialog.into(), owner, &title, &filters)
        })
    })
    .join()
    .ok()
    .flatten()
}

fn owned(filters: &[Filter]) -> Vec<(HSTRING, HSTRING)> {
    filters
        .iter()
        .map(|f| (HSTRING::from(f.name), HSTRING::from(f.patterns)))
        .collect()
}

fn with_com<T>(f: impl FnOnce() -> Option<T>) -> Option<T> {
    // SAFETY: this thread is new and only used for the dialog; every successful initialise is paired with an
    // uninitialise.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)
            .ok()
            .ok()?;
        let result = f();
        CoUninitialize();
        result
    }
}

fn show(
    dialog: &IFileDialog,
    owner: isize,
    title: &str,
    filters: &[(HSTRING, HSTRING)],
) -> Option<PathBuf> {
    let specs: Vec<COMDLG_FILTERSPEC> = filters
        .iter()
        .map(|(name, spec)| COMDLG_FILTERSPEC {
            pszName: PCWSTR(name.as_ptr()),
            pszSpec: PCWSTR(spec.as_ptr()),
        })
        .collect();
    // SAFETY: `specs` points into `filters`, which outlive the dialog; the owner handle is only passed through.
    // Windows returns an error for Cancel, which becomes `None`.
    unsafe {
        if !specs.is_empty() {
            dialog.SetFileTypes(&specs).ok()?;
        }
        dialog.SetTitle(&HSTRING::from(title)).ok()?;
        let owner = (owner != 0).then_some(HWND(owner as *mut core::ffi::c_void));
        dialog.Show(owner).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const core::ffi::c_void));
        path.map(PathBuf::from)
    }
}
