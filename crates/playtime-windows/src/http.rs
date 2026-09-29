//! HTTPS GETs for online artwork over WinHTTP, Windows' own HTTP stack: the system proxy settings apply, and
//! certificates are validated by Windows exactly as for any other app (nothing here relaxes that).
//!
//! Every URL, including each redirect target, must pass `playtime_artwork::http::check_url` (HTTPS to a known
//! artwork host). WinHTTP's automatic redirects are turned off so that check can't be bypassed.

use playtime_artwork::http::{
    check_url, redact, HttpClient, HttpError, Request, Response, MAX_RESPONSE_BYTES,
};
use std::ffi::c_void;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetOption, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_FLAG_SECURE, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2,
    WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3, WINHTTP_OPTION_REDIRECT_POLICY,
    WINHTTP_OPTION_REDIRECT_POLICY_NEVER, WINHTTP_OPTION_SECURE_PROTOCOLS,
    WINHTTP_QUERY_CONTENT_TYPE, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_LOCATION,
    WINHTTP_QUERY_STATUS_CODE,
};

const MAX_REDIRECTS: usize = 3;
const TIMEOUT_MS: i32 = 15_000;

/// An owned WinHTTP handle, closed on drop.
struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void) -> Result<Self, HttpError> {
        if raw.is_null() {
            Err(HttpError::Transport(last_error()))
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: we own the handle and close it exactly once.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

// SAFETY: WinHTTP session handles may be used from any thread; each request uses its own connect/request handles.
unsafe impl Send for Handle {}
// SAFETY: as above; the session handle is only read after creation.
unsafe impl Sync for Handle {}

fn last_error() -> String {
    windows::core::Error::from_win32().message()
}

pub struct WinHttpClient {
    session: Handle,
}

impl WinHttpClient {
    pub fn new(user_agent: &str) -> Result<Self, HttpError> {
        let agent = HSTRING::from(user_agent);
        // SAFETY: `agent` outlives the call; the returned handle is owned by `Handle`.
        let session = Handle::new(unsafe {
            WinHttpOpen(
                PCWSTR(agent.as_ptr()),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            )
        })?;
        // SAFETY: valid session handle; option buffers are plain u32s that live for the call.
        unsafe {
            let modern = (WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2
                | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3)
                .to_ne_bytes();
            if WinHttpSetOption(
                Some(session.0),
                WINHTTP_OPTION_SECURE_PROTOCOLS,
                Some(&modern),
            )
            .is_err()
            {
                // Windows 10 before TLS 1.3 support: TLS 1.2 only (never anything older).
                let tls12 = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2.to_ne_bytes();
                WinHttpSetOption(
                    Some(session.0),
                    WINHTTP_OPTION_SECURE_PROTOCOLS,
                    Some(&tls12),
                )
                .map_err(|e| HttpError::Transport(e.message()))?;
            }
            let never = WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_ne_bytes();
            WinHttpSetOption(
                Some(session.0),
                WINHTTP_OPTION_REDIRECT_POLICY,
                Some(&never),
            )
            .map_err(|e| HttpError::Transport(e.message()))?;
            WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS)
                .map_err(|e| HttpError::Transport(e.message()))?;
        }
        Ok(Self { session })
    }

    fn get_once(
        &self,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<(Response, Option<String>), HttpError> {
        let host = check_url(url)?;
        let path = &url["https://".len() + host.len()..];
        let path = if path.is_empty() { "/" } else { path };
        let transport = |e: windows::core::Error| {
            HttpError::Transport(format!("{}: {}", redact(url), e.message()))
        };

        let host_w = HSTRING::from(host);
        // SAFETY: valid session; `host_w` outlives the call.
        let connect = Handle::new(unsafe {
            WinHttpConnect(self.session.0, PCWSTR(host_w.as_ptr()), 443, 0)
        })?;
        let verb = HSTRING::from("GET");
        let object = HSTRING::from(path);
        // SAFETY: valid connect handle; strings outlive the call; no accept-types array.
        let request = Handle::new(unsafe {
            WinHttpOpenRequest(
                connect.0,
                PCWSTR(verb.as_ptr()),
                PCWSTR(object.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        })?;
        for (name, value) in headers {
            if name.contains(['\r', '\n', ':']) || value.contains(['\r', '\n']) {
                return Err(HttpError::Transport("invalid header".into()));
            }
            let line: Vec<u16> = format!("{name}: {value}").encode_utf16().collect();
            // SAFETY: valid request handle; `line` lives for the call.
            unsafe { WinHttpAddRequestHeaders(request.0, &line, WINHTTP_ADDREQ_FLAG_ADD) }
                .map_err(transport)?;
        }
        // SAFETY: valid request handle; no body.
        unsafe {
            WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(transport)?;
            WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(transport)?;
        }

        let status = query_number(&request, WINHTTP_QUERY_STATUS_CODE).unwrap_or(0);
        let status = u16::try_from(status).unwrap_or(0);
        if (300..400).contains(&status) {
            return Ok((
                Response {
                    status,
                    content_type: None,
                    body: Vec::new(),
                },
                query_string(&request, WINHTTP_QUERY_LOCATION),
            ));
        }
        let content_type = query_string(&request, WINHTTP_QUERY_CONTENT_TYPE);
        let mut body = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let mut read: u32 = 0;
            // SAFETY: `chunk` has `chunk.len()` bytes; WinHTTP writes at most that and reports how many.
            unsafe {
                WinHttpReadData(
                    request.0,
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    &mut read,
                )
            }
            .map_err(transport)?;
            if read == 0 {
                break;
            }
            if body.len() + read as usize > MAX_RESPONSE_BYTES {
                return Err(HttpError::TooLarge);
            }
            body.extend_from_slice(&chunk[..read as usize]);
        }
        Ok((
            Response {
                status,
                content_type,
                body,
            },
            None,
        ))
    }
}

impl HttpClient for WinHttpClient {
    fn get(&self, request: &Request) -> Result<Response, HttpError> {
        let mut url = request.url.clone();
        let mut headers = request.headers.clone();
        for _ in 0..=MAX_REDIRECTS {
            let original_host = check_url(&url)?.to_ascii_lowercase();
            let (response, location) = self.get_once(&url, &headers)?;
            let Some(location) = location else {
                return Ok(response);
            };
            // Only absolute HTTPS redirects to allowed hosts are followed.
            let next_host = check_url(&location)?.to_ascii_lowercase();
            if next_host != original_host {
                headers.clear(); // never forward credentials to another host
            }
            url = location;
        }
        Err(HttpError::Transport(format!(
            "too many redirects from {}",
            redact(&request.url)
        )))
    }
}

fn query_number(request: &Handle, level: u32) -> Option<u32> {
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let mut index = 0;
    // SAFETY: `value` is a u32 buffer of `size` bytes.
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            level | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut value as *mut u32).cast()),
            &mut size,
            &mut index,
        )
    }
    .ok()?;
    Some(value)
}

fn query_string(request: &Handle, level: u32) -> Option<String> {
    let mut buffer = vec![0u16; 2048];
    let mut size = (buffer.len() * 2) as u32;
    let mut index = 0;
    // SAFETY: `buffer` has `size` bytes; WinHTTP writes at most that and updates `size`.
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            level,
            PCWSTR::null(),
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
            &mut index,
        )
    }
    .ok()?;
    buffer.truncate(size as usize / 2);
    String::from_utf16(&buffer).ok()
}
