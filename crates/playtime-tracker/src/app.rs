//! Start-up: command-line arguments, single instance, then the service, the dashboard pipe and the tray.
//!
//! Arguments (as the C# app): `--startup` / `--updated` start quietly in the tray; `--exit` asks a running copy to
//! save and quit (used by the installer); otherwise the dashboard opens.

use crate::service::{EventHub, Service};
use crate::{instance, pipe, tray, updater};
use std::sync::{Arc, Mutex};

pub fn run(args: Vec<String>) -> i32 {
    let has = |flag: &str| args.iter().any(|a| a.eq_ignore_ascii_case(flag));

    if has("--exit") {
        return if instance::request_exit(15_000) { 0 } else { 1 };
    }

    let Some(instance) = instance::acquire() else {
        // Already running: show its dashboard instead (unless this was the quiet start-up launch).
        if !has("--startup") && !has("--updated") {
            instance::signal_show();
        }
        return 0;
    };

    let events = EventHub::default();
    let (service, notifications) = Service::start(events.clone());
    let data_folder = service.data_folder().clone();
    let service = Arc::new(Mutex::new(service));

    if let Err(e) = pipe::start(service.clone(), events) {
        playtime_windows::store::log_to(
            &data_folder,
            &format!("The dashboard connection isn't available: {e}"),
        );
    }

    updater::spawn(service.clone());

    let quiet = has("--startup") || has("--updated");
    if !quiet && notifications.is_empty() && tray::dashboard_path().is_some() {
        tray::open_dashboard(None, &data_folder);
    }
    tray::run(service, instance, notifications)
}
