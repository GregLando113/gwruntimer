//! PID-targeted DLL injection.
//!
//! hudhook 0.9's `inject::Process` can only be built `by_name`/`by_title` and its
//! `HANDLE` field is private, so it cannot inject into a handle we open for a
//! user-selected PID. We therefore replicate its (standard) remote-thread
//! `LoadLibraryW` flow here against a PID we open ourselves.

use std::ffi::c_void;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, INFINITE, OpenProcess, PROCESS_CREATE_THREAD,
    PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    WaitForSingleObject,
};
use windows::core::{s, w};

/// Loads `dll_path` into the process identified by `pid` via a remote `LoadLibraryW`.
pub fn inject_pid(pid: u32, dll_path: &Path) -> Result<(), String> {
    // Resolve to an absolute path the target can open, then widen (NUL-terminated).
    let path = dll_path
        .canonicalize()
        .map_err(|e| format!("DLL not found at {}: {e}", dll_path.display()))?;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * mem::size_of::<u16>();

    unsafe {
        let handle = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_VM_READ,
            false,
            pid,
        )
        .map_err(|e| format!("OpenProcess({pid}) failed: {e}"))?;

        let result = write_and_load(handle, &wide, byte_len);
        let _ = CloseHandle(handle);
        result
    }
}

/// Allocates the path in the target, writes it, and runs LoadLibraryW on it.
///
/// # Safety
/// `handle` must be a live process handle with the injection access rights.
unsafe fn write_and_load(handle: HANDLE, wide: &[u16], byte_len: usize) -> Result<(), String> {
    // LoadLibraryW lives at the same address in every process, so our kernel32
    // copy's address is valid in the target too.
    let load_library = unsafe {
        GetProcAddress(
            GetModuleHandleW(w!("kernel32")).map_err(|e| format!("GetModuleHandleW failed: {e}"))?,
            s!("LoadLibraryW"),
        )
    }
    .ok_or_else(|| "GetProcAddress(LoadLibraryW) returned null".to_string())?;

    let remote =
        unsafe { VirtualAllocEx(handle, None, byte_len, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE) };
    if remote.is_null() {
        return Err("VirtualAllocEx failed in target process".to_string());
    }

    // Ensure the remote allocation is freed on every early return past this point.
    let cleanup = |ok: Result<(), String>| -> Result<(), String> {
        unsafe {
            let _ = VirtualFreeEx(handle, remote, 0, MEM_RELEASE);
        }
        ok
    };

    let mut written = 0usize;
    let write_ok = unsafe {
        WriteProcessMemory(
            handle,
            remote,
            wide.as_ptr() as *const c_void,
            byte_len,
            Some(&mut written),
        )
    };
    if write_ok.is_err() || written != byte_len {
        return cleanup(Err("WriteProcessMemory failed".to_string()));
    }

    let start = unsafe {
        mem::transmute::<
            unsafe extern "system" fn() -> isize,
            unsafe extern "system" fn(*mut c_void) -> u32,
        >(load_library)
    };

    let thread =
        match unsafe { CreateRemoteThread(handle, None, 0, Some(start), Some(remote), 0, None) } {
            Ok(t) => t,
            Err(e) => return cleanup(Err(format!("CreateRemoteThread failed: {e}"))),
        };

    if unsafe { WaitForSingleObject(thread, INFINITE) } != WAIT_OBJECT_0 {
        unsafe {
            let _ = CloseHandle(thread);
        }
        return cleanup(Err("Waiting on remote thread failed".to_string()));
    }

    // LoadLibraryW returns the module HMODULE (0 => the DLL failed to load).
    let mut exit_code = 0u32;
    unsafe {
        let _ = GetExitCodeThread(thread, &mut exit_code);
        let _ = CloseHandle(thread);
    }

    if exit_code == 0 {
        return cleanup(Err(
            "Remote LoadLibraryW returned NULL — the DLL failed to load in the target".to_string(),
        ));
    }

    cleanup(Ok(()))
}
