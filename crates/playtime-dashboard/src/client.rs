//! Talking to the tracker over its named pipe with the shared `playtime_core::ipc` types. Requests use one
//! connection (reopened when it breaks); events come over a second, subscribed connection that reconnects by itself
//! if the tracker restarts.

use playtime_core::ipc::{self, Event, Request, Response};
use std::fs::File;
use std::io::{self, BufReader};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// Why a request didn't get an answer.
#[derive(Debug)]
pub enum ClientError {
    /// The tracker isn't running (or the pipe isn't the user's own tracker): show the banner.
    Unavailable(String),
    /// The tracker answered with an error message for the user.
    Tracker(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(m) | Self::Tracker(m) => f.write_str(m),
        }
    }
}

struct Connection {
    reader: BufReader<File>,
    writer: File,
}

impl Connection {
    fn open() -> Result<Self, ClientError> {
        let pipe = connect().map_err(|e| {
            ClientError::Unavailable(if e.kind() == io::ErrorKind::PermissionDenied {
                format!("Couldn't connect safely to PlayLedger: {e}.")
            } else {
                "PlayLedger isn't running.".into()
            })
        })?;
        let writer = pipe
            .try_clone()
            .map_err(|e| ClientError::Unavailable(format!("Couldn't talk to PlayLedger: {e}.")))?;
        Ok(Self {
            reader: BufReader::new(pipe),
            writer,
        })
    }

    fn send(&mut self, request: &Request) -> io::Result<()> {
        ipc::write_message(&mut self.writer, request)
    }

    fn receive(&mut self) -> io::Result<Option<Response>> {
        let Some(line) = ipc::read_line(&mut self.reader)? else {
            return Ok(None);
        };
        serde_json::from_str(&line)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

#[cfg(windows)]
fn connect() -> io::Result<File> {
    playtime_windows::pipe::connect()
}

#[cfg(not(windows))]
fn connect() -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "the tracker runs only on Windows",
    ))
}

/// The request connection; safe to share between threads (one request at a time).
#[derive(Default)]
pub struct Tracker {
    connection: Mutex<Option<Connection>>,
}

impl Tracker {
    /// Sends one request and waits for its answer. A broken connection is reopened once before giving up.
    pub fn request(&self, request: &Request) -> Result<Response, ClientError> {
        let mut slot = self
            .connection
            .lock()
            .map_err(|_| ClientError::Unavailable("The dashboard is closing.".into()))?;
        for attempt in 0..2 {
            if slot.is_none() {
                *slot = Some(Connection::open()?);
            }
            let Some(connection) = slot.as_mut() else {
                continue;
            };
            let answered = connection.send(request).and_then(|()| loop {
                match connection.receive()? {
                    // Events belong to the subscribed connection; never an answer.
                    Some(Response::Event { .. }) => continue,
                    other => break Ok(other),
                }
            });
            match answered {
                Ok(Some(Response::Error { message })) => return Err(ClientError::Tracker(message)),
                Ok(Some(response)) => return Ok(response),
                Ok(None) | Err(_) => {
                    *slot = None;
                    if attempt == 1 {
                        return Err(ClientError::Unavailable(
                            "Lost the connection to PlayLedger.".into(),
                        ));
                    }
                }
            }
        }
        Err(ClientError::Unavailable("PlayLedger isn't running.".into()))
    }
}

/// Runs for the life of the dashboard: subscribes to the tracker's events and calls `on_event` for each (heartbeats
/// excluded), and `on_connection(true/false)` when the stream connects or drops. Reconnects every two seconds.
pub fn watch_events(
    on_event: impl Fn(Event) + Send + 'static,
    on_connection: impl Fn(bool) + Send + 'static,
) {
    thread::Builder::new()
        .name("tracker events".into())
        .spawn(move || loop {
            if let Ok(mut connection) = Connection::open() {
                if connection.send(&Request::Subscribe).is_ok()
                    && matches!(connection.receive(), Ok(Some(Response::Ok)))
                {
                    on_connection(true);
                    while let Ok(Some(response)) = connection.receive() {
                        if let Response::Event { event } = response {
                            if !matches!(event, Event::Heartbeat) {
                                on_event(event);
                            }
                        }
                    }
                    on_connection(false);
                }
            }
            thread::sleep(Duration::from_secs(2));
        })
        .map(drop)
        .unwrap_or_default();
}
