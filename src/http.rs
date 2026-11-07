//! A plain HTTPS GET through Windows' own WinHTTP: proxies and certificates come from the system, and no crate is needed.

use std::ffi::c_void;
use std::time::{Duration, Instant};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_OPEN_REQUEST_FLAGS,
};
use windows::core::{w, HSTRING, PCWSTR};

const QUERY_STATUS_CODE: u32 = 19;
const QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
const LIMIT: usize = 256 * 1024;
const PATIENCE: Duration = Duration::from_secs(30); // The timeouts below only cover one read each.

// Closes on every way out, so a failed request can't leave a handle behind.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn handle(raw: *mut c_void) -> Option<Handle> {
    (!raw.is_null()).then_some(Handle(raw))
}

pub fn get(host: &str, path: &str) -> Option<String> {
    request(host, INTERNET_DEFAULT_HTTPS_PORT, path, WINHTTP_FLAG_SECURE)
}

/// Plain HTTP to any port. Only for checking that failures clean up after themselves.
pub fn get_insecure(host: &str, port: u16, path: &str) -> Option<String> {
    request(host, port, path, WINHTTP_OPEN_REQUEST_FLAGS(0))
}

fn request(host: &str, port: u16, path: &str, flags: WINHTTP_OPEN_REQUEST_FLAGS) -> Option<String> {
    unsafe {
        let session = handle(WinHttpOpen(
            w!("DesktopWidgets"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, 5_000, 5_000, 10_000, 10_000).ok()?;
        let host = HSTRING::from(host);
        let connection = handle(WinHttpConnect(session.0, PCWSTR(host.as_ptr()), port, 0))?;
        let path = HSTRING::from(path);
        let request = handle(WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        ))?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).ok()?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).ok()?;

        let mut status = 0u32;
        let mut size = 4u32;
        WinHttpQueryHeaders(
            request.0,
            QUERY_STATUS_CODE | QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&raw mut status).cast()),
            &mut size,
            std::ptr::null_mut(),
        )
        .ok()?;
        if status != 200 {
            return None;
        }

        let started = Instant::now();
        let mut body = Vec::new();
        loop {
            if started.elapsed() > PATIENCE {
                return None;
            }
            let mut available = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut available).ok()?;
            if available == 0 {
                break;
            }
            let start = body.len();
            if start + available as usize > LIMIT {
                return None;
            }
            body.resize(start + available as usize, 0);
            let mut read = 0u32;
            WinHttpReadData(request.0, body[start..].as_mut_ptr().cast(), available, &mut read).ok()?;
            body.truncate(start + read as usize);
        }
        String::from_utf8(body).ok()
    }
}
