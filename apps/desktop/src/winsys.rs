//! Small Win32 helpers for the UI app. UNSAFE MODULE: safe wrappers only leave it.
//!
//! - `attach_parent_console`: the app is built with the Windows GUI subsystem (no console
//!   window of its own). When it is started from a terminal, this attaches to that terminal's
//!   console so `--help`, errors and `--headless` output and Ctrl+C keep working (Task 202).
//!   Handles that were redirected (pipes, files) are left alone.
//! - `local_utc_offset_minutes`: the current local UTC offset, for the "paused until HH:MM"
//!   tooltip.

#![allow(unsafe_code)]

use windows::core::w;
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Console::{
    AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_HANDLE,
    STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};

/// Value `GetTimeZoneInformation` returns while daylight saving time is in effect.
const TIME_ZONE_ID_DAYLIGHT: u32 = 2;

fn is_missing(h: &HANDLE) -> bool {
    h.is_invalid() || h.0.is_null()
}

fn fill_std_handle(which: STD_HANDLE, console_file: windows::core::PCWSTR) {
    // SAFETY: `console_file` is a `w!` literal, so it is static and NUL-terminated. The calls
    // only query a standard handle and, if it is missing (null or invalid), open the console
    // device and install it; a handle the parent redirected is never replaced. The opened
    // handle is leaked on purpose: it becomes the process's standard handle and must stay open
    // until exit.
    unsafe {
        let current = GetStdHandle(which).unwrap_or(INVALID_HANDLE_VALUE);
        if !is_missing(&current) {
            return; // redirected by the parent: keep it
        }
        let opened = CreateFileW(
            console_file,
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        );
        if let Ok(h) = opened {
            let _ = SetStdHandle(which, h);
        }
    }
}

/// Attaches to the parent process's console, if it has one, and points any missing standard
/// handle at it. Does nothing when started from Explorer (no parent console).
pub fn attach_parent_console() {
    // SAFETY: takes no pointers; fails harmlessly when there is no parent console.
    let attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_ok();
    if !attached {
        return;
    }
    fill_std_handle(STD_OUTPUT_HANDLE, w!("CONOUT$"));
    fill_std_handle(STD_ERROR_HANDLE, w!("CONOUT$"));
    fill_std_handle(STD_INPUT_HANDLE, w!("CONIN$"));
}

/// Current local offset from UTC in minutes (east is positive); 0 when it cannot be read.
pub fn local_utc_offset_minutes() -> i32 {
    let mut info = TIME_ZONE_INFORMATION::default();
    // SAFETY: `info` is a valid, writable TIME_ZONE_INFORMATION.
    let id = unsafe { GetTimeZoneInformation(&mut info) };
    if id == u32::MAX {
        return 0; // TIME_ZONE_ID_INVALID
    }
    let extra = if id == TIME_ZONE_ID_DAYLIGHT {
        info.DaylightBias
    } else {
        info.StandardBias
    };
    -(info.Bias + extra)
}
