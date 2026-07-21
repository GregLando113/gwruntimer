//! Enumerates running Guild Wars clients for the selector.
//!
//! v1 display source: ToolHelp process list (exe name + PID) enriched with each
//! process's top-level window title so multiboxed instances can be told apart.
//! Reading the logged-in character name out of client memory is a deferred v2
//! enhancement (see the plan) and deliberately not done here.

use std::collections::HashMap;

use windows::Win32::Foundation::{BOOL, CloseHandle, HWND, LPARAM, TRUE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
};

/// Executable name to match, case-insensitively.
const GW_EXE: &str = "Gw.exe";

/// A running Guild Wars client discovered by [`enumerate`].
#[derive(Clone)]
pub struct GwClient {
    pub pid: u32,
    pub exe: String,
    pub title: Option<String>,
}

impl GwClient {
    /// Label shown in the selector list, e.g. `"Guild Wars — Gw.exe (pid 1234)"`.
    pub fn label(&self) -> String {
        match &self.title {
            Some(t) if !t.is_empty() => format!("{t} — {} (pid {})", self.exe, self.pid),
            _ => format!("{} (pid {})", self.exe, self.pid),
        }
    }
}

/// Returns every running process whose exe matches [`GW_EXE`], newest snapshot each call.
pub fn enumerate() -> Result<Vec<GwClient>, String> {
    let titles = window_titles_by_pid();

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|e| format!("CreateToolhelp32Snapshot failed: {e}"))?;

    let mut clients = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    unsafe {
        // Process32FirstW fails on an empty snapshot; treat that as "no processes".
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let exe = wide_to_string(&entry.szExeFile);
                if exe.eq_ignore_ascii_case(GW_EXE) {
                    let pid = entry.th32ProcessID;
                    clients.push(GwClient {
                        pid,
                        exe,
                        title: titles.get(&pid).cloned(),
                    });
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }

    Ok(clients)
}

/// Maps PID -> title for every visible top-level window with a non-empty title.
fn window_titles_by_pid() -> HashMap<u32, String> {
    let mut map: HashMap<u32, String> = HashMap::new();
    // SAFETY: `map` outlives the EnumWindows call, which is synchronous.
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
        let _ = EnumWindows(Some(enum_windows_proc), LPARAM(&mut map as *mut _ as isize));
    }
    map
}

extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam carries the &mut HashMap passed by window_titles_by_pid.
    let map = unsafe { &mut *(lparam.0 as *mut HashMap<u32, String>) };

    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return TRUE;
        }

        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return TRUE;
        }

        let mut buf = vec![0u16; len as usize + 1];
        let copied = GetWindowTextW(hwnd, &mut buf);
        if copied <= 0 {
            return TRUE;
        }
        buf.truncate(copied as usize);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 {
            // First visible titled window per PID wins.
            map.entry(pid).or_insert_with(|| String::from_utf16_lossy(&buf));
        }
    }

    TRUE
}

/// Decodes a null-terminated UTF-16 buffer (e.g. `szExeFile`) into a `String`.
fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
