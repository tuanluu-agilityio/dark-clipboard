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

/// A monitor: its full rectangle (names it for the history window's memory), its work area and
/// its effective DPI, in physical pixels.
pub struct MonitorGeometry {
    pub full: crate::popup::place::Rect,
    pub work: crate::popup::place::Rect,
    pub dpi: u32,
}

fn rect_of(r: windows::Win32::Foundation::RECT) -> crate::popup::place::Rect {
    crate::popup::place::Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

/// The monitor nearest to the physical point `pt`; `None` when Windows cannot tell.
pub fn monitor_at(x: i32, y: i32) -> Option<MonitorGeometry> {
    let pt = POINT { x, y };
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
    Some(MonitorGeometry {
        full: rect_of(info.rcMonitor),
        work: rect_of(info.rcWork),
        dpi: dpi_x,
    })
}

/// The cursor position in physical pixels.
pub fn cursor_pos() -> Option<(i32, i32)> {
    let mut pt = POINT::default();
    // SAFETY: `pt` is a valid, writable POINT.
    unsafe { GetCursorPos(&mut pt) }.ok()?;
    Some((pt.x, pt.y))
}

/// Where the cursor is and which monitor it is on; `None` when Windows cannot tell.
pub fn monitor_at_cursor() -> Option<MonitorAtCursor> {
    let cursor = cursor_pos()?;
    let m = monitor_at(cursor.0, cursor.1)?;
    Some(MonitorAtCursor {
        cursor,
        work: m.work,
        dpi: m.dpi,
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

/// The image (file) names of the running programs, for "pick running app" (Task 207). Process
/// names only: no window is touched and no window title is read (SR-11). Empty when the
/// snapshot cannot be taken. Not sorted or de-duplicated (see `prefs::sys::unique_sorted`).
pub fn running_image_names() -> Vec<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    // SAFETY: plain value arguments; the returned snapshot handle is owned here and closed
    // below on every path.
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(0),
        ..Default::default()
    };
    // SAFETY: `snap` is the open snapshot; `entry` is a valid, writable PROCESSENTRY32W whose
    // `dwSize` is set as the API requires.
    let mut ok = unsafe { Process32FirstW(snap, &mut entry) }.is_ok();
    while ok {
        let len = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        if let Some(name) = entry.szExeFile.get(..len) {
            out.push(String::from_utf16_lossy(name));
        }
        // SAFETY: as for `Process32FirstW`.
        ok = unsafe { Process32NextW(snap, &mut entry) }.is_ok();
    }
    // SAFETY: `snap` is owned here and closed exactly once.
    let _ = unsafe { CloseHandle(snap) };
    out
}

/// The Windows folder (for example `C:\\Windows`), from `GetWindowsDirectoryW`.
pub fn windows_directory() -> Option<std::path::PathBuf> {
    use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
    let mut buf = [0u16; 260];
    // SAFETY: `buf` is a valid, writable buffer; the call reports how many characters it wrote.
    let n = unsafe { GetWindowsDirectoryW(Some(&mut buf)) } as usize;
    (n > 0 && n < buf.len()).then(|| std::path::PathBuf::from(String::from_utf16_lossy(&buf[..n])))
}

/// Free bytes available to this user on the volume that holds `dir`, from
/// `GetDiskFreeSpaceExW` (quotas included). `None` when the call fails.
pub fn free_disk_bytes(dir: &std::path::Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is NUL-terminated and outlives the call; `free` is a valid, writable u64;
    // the two optional out-pointers are not requested.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None) }
        .ok()
        .map(|()| free)
}

/// The shell's file dialog (Task 405): `IFileSaveDialog` when `save`, else `IFileOpenDialog`,
/// limited to file-system paths and one file type (`*.ext`, `ext` without the dot). Blocks
/// until the user closes it; call it on its own thread (it initialises COM there as a
/// single-threaded apartment and uninitialises it before returning). `None` when cancelled or
/// when the dialog cannot be shown. No owner window: the app's windows stay usable.
pub fn pick_file(
    save: bool,
    title: &str,
    filter_name: &str,
    ext: &str,
    default_name: Option<&str>,
) -> Option<std::path::PathBuf> {
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };

    /// Uninitialises COM on drop, after every interface has been released.
    struct ComGuard;
    impl Drop for ComGuard {
        fn drop(&mut self) {
            // SAFETY: paired with the successful `CoInitializeEx` on this same thread.
            unsafe { CoUninitialize() };
        }
    }

    // SAFETY: no reserved pointer; a fresh thread, so the apartment cannot clash. S_FALSE
    // (already initialised) also needs the matching uninitialise, which `ok()` keeps.
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    if hr.is_err() {
        return None;
    }
    let _com = ComGuard;
    // Every COM object lives inside this call and is released before `_com` drops.
    show_file_dialog(save, title, filter_name, ext, default_name).ok()
}

fn show_file_dialog(
    save: bool,
    title: &str,
    filter_name: &str,
    ext: &str,
    default_name: Option<&str>,
) -> windows::core::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog,
        FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_OVERWRITEPROMPT,
        FOS_PATHMUSTEXIST, SIGDN_FILESYSPATH,
    };

    let title = HSTRING::from(title);
    let name = HSTRING::from(filter_name);
    let spec = HSTRING::from(format!("*.{ext}"));
    let ext = HSTRING::from(ext);
    let filters = [COMDLG_FILTERSPEC {
        pszName: PCWSTR(name.as_ptr()),
        pszSpec: PCWSTR(spec.as_ptr()),
    }];
    // SAFETY: plain COM calls on interfaces this function owns; every string passed is an
    // `HSTRING` (NUL-terminated) that outlives the dialog; the result string is freed with
    // `CoTaskMemFree` after it was copied.
    unsafe {
        let dlg: IFileDialog = if save {
            let d: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
            windows::core::Interface::cast(&d)?
        } else {
            let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            windows::core::Interface::cast(&d)?
        };
        let mut opts =
            dlg.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR;
        opts |= if save {
            FOS_OVERWRITEPROMPT
        } else {
            FOS_FILEMUSTEXIST
        };
        dlg.SetOptions(opts)?;
        dlg.SetTitle(&title)?;
        dlg.SetFileTypes(&filters)?;
        dlg.SetDefaultExtension(&ext)?;
        if let Some(n) = default_name {
            dlg.SetFileName(&HSTRING::from(n))?;
        }
        // Cancelling returns an error (ERROR_CANCELLED): no path.
        dlg.Show(None)?;
        let item = dlg.GetResult()?;
        let raw = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = std::ffi::OsString::from_wide(raw.as_wide());
        CoTaskMemFree(Some(raw.0 as *const core::ffi::c_void));
        Ok(std::path::PathBuf::from(path))
    }
}

/// `WerAddExcludedApplication(exe_name, bAllUsers = FALSE)` (Task 413, F407-01): adds the
/// executable's file name to the current user's WER exclusion list (HKCU, no admin needed,
/// permanent until `WerRemoveExcludedApplication`). `true` on `S_OK`.
pub fn wer_add_excluded_application(exe_name: &std::ffi::OsStr) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::System::ErrorReporting::WerAddExcludedApplication;
    let name = HSTRING::from(exe_name);
    // SAFETY: `name` is a NUL-terminated wide string that outlives the call; the function
    // copies it into the registry and keeps no pointer.
    unsafe { WerAddExcludedApplication(&name, false) }.is_ok()
}

/// `WerSetFlags(WER_FAULT_REPORTING_FLAG_NOHEAP)` (Task 413, F407-01): no heap memory in any
/// WER report of this process. `true` on `S_OK`.
pub fn wer_set_no_heap() -> bool {
    use windows::Win32::System::ErrorReporting::{WerSetFlags, WER_FAULT_REPORTING_FLAG_NOHEAP};
    // SAFETY: takes a constant flag value, no pointers; affects only this process.
    unsafe { WerSetFlags(WER_FAULT_REPORTING_FLAG_NOHEAP) }.is_ok()
}

fn wide_path(path: &std::path::Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// The owner and DACL of the file or folder `path` as SDDL text (`O:<sid>D:<flags>(...)`),
/// from `GetNamedSecurityInfoW` and `ConvertSecurityDescriptorToStringSecurityDescriptorW`.
/// `None` when either call fails. Security data only; nothing of the folder's content.
pub fn security_sddl(path: &std::path::Path) -> Option<String> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
        SDDL_REVISION_1, SE_FILE_OBJECT,
    };
    use windows::Win32::Security::{
        DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    };
    let wide = wide_path(path);
    let info = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `wide` is NUL-terminated and outlives the call; the owner and DACL out-pointers
    // are not requested (they would point into `sd`); `sd` receives a descriptor allocated by
    // the system, released below with `LocalFree` on every path.
    let got = unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(wide.as_ptr()),
            SE_FILE_OBJECT,
            info,
            None,
            None,
            None,
            None,
            &mut sd,
        )
    };
    if got != ERROR_SUCCESS || sd.0.is_null() {
        return None;
    }
    let mut text = PWSTR::null();
    // SAFETY: `sd` is the valid descriptor from above; `text` receives a string allocated by
    // the system, copied and then released with `LocalFree`; `sd` is released afterwards.
    unsafe {
        let converted = ConvertSecurityDescriptorToStringSecurityDescriptorW(
            sd,
            SDDL_REVISION_1,
            info,
            &mut text,
            None,
        );
        let out = if converted.is_ok() && !text.is_null() {
            text.to_string().ok()
        } else {
            None
        };
        if !text.is_null() {
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        }
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        out
    }
}

/// Replaces the DACL of the folder `path` with the one in `sddl` and marks it protected (no
/// entries inherited from the parent), with `SetNamedSecurityInfoW(DACL |
/// PROTECTED_DACL)`. The system propagates the new inheritable entries to existing children.
/// Needs `WRITE_DAC` or ownership. `true` on success.
pub fn set_protected_dacl(path: &std::path::Path, sddl: &str) -> bool {
    use windows::core::{BOOL, PCWSTR};
    use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW,
        SDDL_REVISION_1, SE_FILE_OBJECT,
    };
    use windows::Win32::Security::{
        GetSecurityDescriptorDacl, ACL, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    };
    let wide = wide_path(path);
    let sddl_w: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `sddl_w` is NUL-terminated and outlives the call; `sd` receives a descriptor
    // allocated by the system and released with `LocalFree` below on every path.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_w.as_ptr()),
            SDDL_REVISION_1,
            &mut sd,
            None,
        )
    }
    .is_err()
    {
        return false;
    }
    let (mut present, mut defaulted) = (BOOL::default(), BOOL::default());
    let mut dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: `sd` is the valid descriptor from above; the out-pointers are live locals; the
    // DACL pointer points into `sd` and is used only before `sd` is released.
    unsafe {
        let got = GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted);
        let set = if got.is_ok() && present.as_bool() && !dacl.is_null() {
            SetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(dacl.cast_const()),
                None,
            ) == ERROR_SUCCESS
        } else {
            false
        };
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        set
    }
}

/// A modal message window with no owner (start-up problems, Task 413). With `yes_no` it has
/// Yes and No buttons (No is the default) and returns true for Yes; otherwise an OK button
/// and it returns false. Blocks the calling thread until it is closed.
pub fn message_box(title: &str, text: &str, yes_no: bool) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_DEFBUTTON2, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND,
        MB_YESNO,
    };
    let (title, text) = (HSTRING::from(title), HSTRING::from(text));
    let style = if yes_no {
        MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2
    } else {
        MB_OK | MB_ICONERROR
    } | MB_SETFOREGROUND;
    // SAFETY: both strings are NUL-terminated `HSTRING`s that outlive the call; no owner
    // window; the call only returns the button pressed.
    unsafe { MessageBoxW(None, &text, &title, style) == IDYES }
}
