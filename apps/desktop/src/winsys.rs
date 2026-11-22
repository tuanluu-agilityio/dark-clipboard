//! Small Win32 helpers for the UI app. UNSAFE MODULE: safe wrappers only leave it.
//!
//! - `attach_parent_console`: the app is built with the Windows GUI subsystem (no console
//!   window of its own). When it is started from a terminal, this attaches to that terminal's
//!   console so `--help`, errors and `--headless` output and Ctrl+C keep working (Task 202).
//!   Handles that were redirected (pipes, files) are left alone.
//! - `local_utc_offset_minutes`: the current local UTC offset, for the "paused until HH:MM"
//!   tooltip.
//! - Popup placement (Task 204): per-monitor DPI v2 awareness, the monitor under the cursor
//!   (work area and DPI), and the foreground window recorded at show time (handle only).

#![allow(unsafe_code)]

use windows::core::w;
use windows::Win32::Foundation::POINT;
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Console::{
    AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_HANDLE,
    STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId,
};

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

/// Makes the process per-monitor DPI aware (v2), so the popup renders crisp on every monitor
/// of a mixed-DPI setup (Task 204). Must run before the first window is created; winit asks for
/// the same mode, so a failure here (already set) is harmless.
pub fn set_per_monitor_dpi_awareness() {
    // SAFETY: takes a constant context value, no pointers; fails harmlessly when already set.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// The cursor position and the monitor under it, in physical pixels.
pub struct MonitorAtCursor {
    pub cursor: (i32, i32),
    /// Work area (without the taskbar) of the monitor under the cursor.
    pub work: crate::popup::place::Rect,
    /// Effective DPI of that monitor (96 = 100 %).
    pub dpi: u32,
}

/// Where the cursor is and which monitor it is on; `None` when Windows cannot tell.
pub fn monitor_at_cursor() -> Option<MonitorAtCursor> {
    let mut pt = POINT::default();
    // SAFETY: `pt` is a valid, writable POINT.
    unsafe { GetCursorPos(&mut pt) }.ok()?;
    // SAFETY: plain value arguments; DEFAULTTONEAREST always returns a monitor handle, which is
    // not owned by the caller (nothing to release).
    let monitor = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>()).ok()?,
        ..Default::default()
    };
    // SAFETY: `info` is a valid MONITORINFO with `cbSize` set, as the API requires.
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }
    let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
    // SAFETY: both out pointers are valid, writable u32s; failure leaves them 0 (treated as 96).
    let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    let w = info.rcWork;
    Some(MonitorAtCursor {
        cursor: (pt.x, pt.y),
        work: crate::popup::place::Rect {
            left: w.left,
            top: w.top,
            right: w.right,
            bottom: w.bottom,
        },
        dpi: dpi_x,
    })
}

/// The foreground window when it belongs to another process (the window to return focus to,
/// Task 205), as its handle value and process id. Only these are kept: no title, no class name
/// (SR-11). Whether it is a window that may receive a paste is decided later by the platform
/// injector (`darkclip-windows`), which checks the handle and the process id again.
pub fn foreground_window_of_other_process() -> Option<(u64, u32)> {
    // SAFETY: no arguments; returns a handle that is not owned by the caller.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: `hwnd` came from the OS just now (a stale handle only makes the call fail); `pid`
    // is a valid, writable u32.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    // SAFETY: no arguments.
    let own = unsafe { GetCurrentProcessId() };
    let value = u64::try_from(hwnd.0 as usize).ok()?;
    (pid != 0 && pid != own).then_some((value, pid))
}
