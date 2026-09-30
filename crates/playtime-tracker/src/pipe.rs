//! The dashboard's API: a named pipe `\\.\pipe\PlaytimeTracker.<user SID>` speaking `playtime_core::ipc`.
//!
//! Only the current Windows user can connect: the pipe's DACL grants access to that user's SID alone, remote
//! (network) clients are rejected, and the first instance is created with `FILE_FLAG_FIRST_PIPE_INSTANCE` so another
//! program can't squat the name first. A connection that sends `subscribe` becomes an event stream.

use crate::service::{EventHub, Service};
use playtime_core::ipc::{self, Event, Response};
use std::fs::File;
use std::io::BufReader;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{LocalFree, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PeekNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

const BUFFER_BYTES: u32 = 64 * 1024;
/// More simultaneous dashboard connections than this are refused (one dashboard uses two).
const MAX_CLIENTS: usize = 8;
const HEARTBEAT: Duration = Duration::from_secs(30);
const DISCONNECT_CHECK: Duration = Duration::from_secs(1);

/// False once the other end of the pipe has closed (PeekNamedPipe then fails with a broken pipe).
fn peer_connected(pipe: &File) -> bool {
    let mut available = 0u32;
    // SAFETY: peeks without reading; the handle belongs to `pipe`, which outlives the call.
    unsafe {
        PeekNamedPipe(
            HANDLE(pipe.as_raw_handle()),
            None,
            0,
            None,
            Some(&mut available),
            None,
        )
    }
    .is_ok()
}

/// Security attributes granting full access to `sid` only (no inheritance, no one else).
struct PipeSecurity {
    descriptor: PSECURITY_DESCRIPTOR,
    attributes: SECURITY_ATTRIBUTES,
}

impl PipeSecurity {
    fn for_user(sid: &str) -> Option<Self> {
        // D:P = protected DACL; one ACE: allow Generic All to the user.
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{sid})"));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: parses our SDDL string into a system-allocated descriptor, freed in Drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .ok()?;
        Some(Self {
            descriptor,
            attributes: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor.0,
                bInheritHandle: false.into(),
            },
        })
    }
}

// SAFETY: the descriptor is allocated once, never modified, and freed only in Drop; the pipe thread owns it.
unsafe impl Send for PipeSecurity {}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        // SAFETY: freeing the descriptor the system allocated.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
        }
    }
}

/// Starts the pipe server on its own thread. Returns `false` if it couldn't (logged by the caller).
pub fn start(service: Arc<Mutex<Service>>, events: EventHub) -> Result<(), String> {
    let sid =
        playtime_windows::pipe::current_user_sid().ok_or("couldn't read the current user's SID")?;
    let name = ipc::pipe_name(&sid);
    let security =
        PipeSecurity::for_user(&sid).ok_or("couldn't build the pipe's security descriptor")?;
    // Created here so a failure (e.g. the name is taken) is reported; passed as an integer because HANDLE isn't Send.
    let first = create_instance(&name, &security, true)?.0 as isize;
    std::thread::Builder::new()
        .name("pipe-server".into())
        .spawn(move || serve(name, security, first, service, events))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn create_instance(name: &str, security: &PipeSecurity, first: bool) -> Result<HANDLE, String> {
    let name = HSTRING::from(name);
    let mut open_mode = PIPE_ACCESS_DUPLEX;
    if first {
        open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    // SAFETY: `name` and `security` outlive the call; the handle is owned by the caller.
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            BUFFER_BYTES,
            BUFFER_BYTES,
            0,
            Some(&security.attributes),
        )
    };
    if handle.is_invalid() {
        Err(format!(
            "couldn't create the dashboard pipe: {}",
            windows::core::Error::from_win32().message()
        ))
    } else {
        Ok(handle)
    }
}

fn serve(
    name: String,
    security: PipeSecurity,
    first: isize,
    service: Arc<Mutex<Service>>,
    events: EventHub,
) {
    let clients = Arc::new(AtomicUsize::new(0));
    let mut next = Some(HANDLE(first as *mut _));
    loop {
        let handle = match next.take() {
            Some(handle) => handle,
            None => match create_instance(&name, &security, false) {
                Ok(handle) => handle,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }
            },
        };
        // SAFETY: waits for a client on our pipe instance.
        let connected = match unsafe { ConnectNamedPipe(handle, None) } {
            Ok(()) => true,
            Err(e) => e.code() == ERROR_PIPE_CONNECTED.to_hresult(),
        };
        // SAFETY: we own the handle; `File` takes ownership and closes it.
        let pipe = unsafe { File::from_raw_handle(handle.0) };
        if !connected || clients.load(Ordering::SeqCst) >= MAX_CLIENTS {
            continue; // `pipe` is dropped, disconnecting the client
        }
        clients.fetch_add(1, Ordering::SeqCst);
        let (service, events, count) = (service.clone(), events.clone(), clients.clone());
        let spawned = std::thread::Builder::new()
            .name("pipe-client".into())
            .spawn(move || {
                client(pipe, &service, &events);
                count.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            clients.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn client(pipe: File, service: &Mutex<Service>, events: &EventHub) {
    let Ok(mut writer) = pipe.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(pipe);
    loop {
        let line = match ipc::read_line(&mut reader) {
            Ok(Some(line)) => line,
            Ok(None) | Err(_) => return,
        };
        let response = match ipc::parse_request(&line) {
            Ok(ipc::Request::Subscribe) => {
                if ipc::write_message(&mut writer, &Response::Ok).is_err() {
                    return;
                }
                let receiver = events.subscribe();
                let mut quiet_since = Instant::now();
                loop {
                    // Check every second whether the dashboard went away (it may have been closed or crashed),
                    // so its connection slot is freed promptly; a heartbeat also goes out when nothing happens.
                    let event = match receiver.recv_timeout(DISCONNECT_CHECK) {
                        Ok(event) => event,
                        Err(RecvTimeoutError::Timeout) => {
                            if !peer_connected(&writer) {
                                return;
                            }
                            if quiet_since.elapsed() < HEARTBEAT {
                                continue;
                            }
                            Event::Heartbeat
                        }
                        Err(RecvTimeoutError::Disconnected) => return,
                    };
                    quiet_since = Instant::now();
                    if ipc::write_message(&mut writer, &Response::Event { event }).is_err() {
                        return;
                    }
                }
            }
            // These go online: prepare under the lock, then run without it so tracking never waits on the network.
            Ok(ipc::Request::ListArtworkChoices { game, kind }) => {
                match service.lock().map(|mut s| s.artwork_job(&game, &kind)) {
                    Ok(Ok(task)) => task.list_choices(),
                    Ok(Err(response)) => response,
                    Err(_) => Response::Error {
                        message: "the tracker is shutting down".into(),
                    },
                }
            }
            Ok(ipc::Request::ApplyArtworkChoice { game, kind, index }) => {
                match service.lock().map(|mut s| s.artwork_job(&game, &kind)) {
                    Ok(Ok(task)) => task.apply_choice(index),
                    Ok(Err(response)) => response,
                    Err(_) => Response::Error {
                        message: "the tracker is shutting down".into(),
                    },
                }
            }
            Ok(request) => match service.lock() {
                Ok(mut service) => service.handle(request),
                Err(_) => Response::Error {
                    message: "the tracker is shutting down".into(),
                },
            },
            Err(message) => Response::Error { message },
        };
        if ipc::write_message(&mut writer, &response).is_err() {
            return;
        }
    }
}
