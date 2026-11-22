//! Phase 0 spike: Windows clipboard monitoring feasibility for Dark Clipboard.
//!
//! Throwaway code. It answers, with a real run on this machine:
//! 1. Does a message-only window receive WM_CLIPBOARDUPDATE via AddClipboardFormatListener,
//!    for writes from this process and from another process?
//! 2. Can the listener see the exclusion formats Darkshot writes
//!    (ExcludeClipboardContentFromMonitorProcessing, CanIncludeInClipboardHistory=0,
//!    CanUploadToCloudClipboard=0) and skip that content?
//! 3. How are bursts coalesced (events vs. sequence numbers)?
//! 4. Can the source process be identified (clipboard owner, foreground window)?
//! 5. Notification latency.
//!
//! Privacy: never prints clipboard content, only lengths and format names.
//! Side effect: overwrites the clipboard during the run, then restores the previous text (if any).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{w, HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, EnumClipboardFormats,
    GetClipboardData, GetClipboardFormatNameW, GetClipboardOwner, GetClipboardSequenceNumber,
    OpenClipboard, RegisterClipboardFormatW, RemoveClipboardFormatListener, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Threading::{
    GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow,
    GetMessageW, GetWindowThreadProcessId, PostQuitMessage, PostThreadMessageW, RegisterClassW,
    HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLIPBOARDUPDATE, WNDCLASSW,
};

const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;
const WM_STOP: u32 = WM_APP + 1;

#[derive(Debug, Clone)]
struct Event {
    at: Instant,
    seq: u32,
    formats: Vec<String>,
    owner_exe: Option<String>,
    foreground_exe: Option<String>,
    excluded_by: Vec<&'static str>,
    text_len: Option<usize>,
    open_attempts: u32,
}

static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());

fn exe_of_window(hwnd: HWND) -> Option<String> {
    if hwnd.is_invalid() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: valid out pointer for the duration of the call.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    // SAFETY: handle closed below on every path.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: buf outlives the call; len is its capacity.
    let r = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) };
    // SAFETY: h is a process handle we own.
    unsafe {
        let _ = CloseHandle(h);
    }
    r.ok()?;
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    Some(path.rsplit('\\').next().unwrap_or(&path).to_string())
}

fn format_name(f: u32) -> String {
    match f {
        1 => "CF_TEXT".into(),
        2 => "CF_BITMAP".into(),
        7 => "CF_OEMTEXT".into(),
        8 => "CF_DIB".into(),
        13 => "CF_UNICODETEXT".into(),
        15 => "CF_HDROP".into(),
        16 => "CF_LOCALE".into(),
        17 => "CF_DIBV5".into(),
        _ => {
            let mut buf = [0u16; 256];
            // SAFETY: buf outlives the call.
            let n = unsafe { GetClipboardFormatNameW(f, &mut buf) };
            if n > 0 {
                String::from_utf16_lossy(&buf[..n as usize])
            } else {
                format!("#{f}")
            }
        }
    }
}

/// Reads a DWORD-valued registered format (e.g. CanIncludeInClipboardHistory). Clipboard must be open.
fn read_dword(format: u32) -> Option<u32> {
    // SAFETY: clipboard open by caller; handle owned by the system; locked only within GlobalSize.
    unsafe {
        let h = GetClipboardData(format).ok()?;
        let g = HGLOBAL(h.0);
        let p = GlobalLock(g) as *const u8;
        if p.is_null() {
            return None;
        }
        let v = if GlobalSize(g) >= 4 {
            Some(u32::from_le_bytes([*p, *p.add(1), *p.add(2), *p.add(3)]))
        } else {
            None
        };
        let _ = GlobalUnlock(g);
        v
    }
}

fn text_len_open() -> Option<usize> {
    // SAFETY: clipboard open by caller.
    unsafe {
        let h = GetClipboardData(CF_UNICODETEXT).ok()?;
        let g = HGLOBAL(h.0);
        let p = GlobalLock(g) as *const u16;
        if p.is_null() {
            return None;
        }
        let units = std::slice::from_raw_parts(p, GlobalSize(g) / 2);
        let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
        let _ = GlobalUnlock(g);
        Some(end)
    }
}

fn reg(name: &str) -> u32 {
    // SAFETY: HSTRING is NUL-terminated and lives through the call.
    unsafe { RegisterClipboardFormatW(&HSTRING::from(name)) }
}

fn on_update(hwnd: HWND) {
    let at = Instant::now();
    // SAFETY: plain query.
    let seq = unsafe { GetClipboardSequenceNumber() };
    let owner_exe = unsafe { GetClipboardOwner() }.ok().and_then(exe_of_window);
    let foreground_exe = exe_of_window(unsafe { GetForegroundWindow() });
    let mut attempts = 0;
    let mut opened = false;
    while attempts < 10 {
        attempts += 1;
        // SAFETY: hwnd is our window on this thread.
        if unsafe { OpenClipboard(Some(hwnd)) }.is_ok() {
            opened = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut formats = Vec::new();
    let mut excluded_by = Vec::new();
    let mut text_len = None;
    if opened {
        let exclude = reg("ExcludeClipboardContentFromMonitorProcessing");
        let history = reg("CanIncludeInClipboardHistory");
        let cloud = reg("CanUploadToCloudClipboard");
        let viewer_ignore = reg("Clipboard Viewer Ignore");
        let mut f = 0u32;
        loop {
            // SAFETY: clipboard open.
            f = unsafe { EnumClipboardFormats(f) };
            if f == 0 {
                break;
            }
            formats.push(format_name(f));
            if f == exclude {
                excluded_by.push("ExcludeClipboardContentFromMonitorProcessing");
            } else if f == history && read_dword(f) == Some(0) {
                excluded_by.push("CanIncludeInClipboardHistory=0");
            } else if f == cloud && read_dword(f) == Some(0) {
                excluded_by.push("CanUploadToCloudClipboard=0");
            } else if f == viewer_ignore {
                excluded_by.push("Clipboard Viewer Ignore");
            }
        }
        // Policy under test: excluded content is never read.
        if excluded_by.is_empty() {
            text_len = text_len_open();
        }
        // SAFETY: we opened it.
        unsafe {
            let _ = CloseClipboard();
        }
    }
    EVENTS.lock().unwrap().push(Event {
        at,
        seq,
        formats,
        owner_exe,
        foreground_exe,
        excluded_by,
        text_len,
        open_attempts: if opened { attempts } else { 0 },
    });
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        on_update(hwnd);
        return LRESULT(0);
    }
    // SAFETY: default handling.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn message_window(class: windows::core::PCWSTR, proc_: windows::Win32::UI::WindowsAndMessaging::WNDPROC) -> HWND {
    // SAFETY: standard window class registration and message-only window creation.
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap();
        let wc = WNDCLASSW {
            lpfnWndProc: proc_,
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinst.into()),
            None,
        )
        .unwrap()
    }
}

extern "system" fn default_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: default handling.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn listener_thread(ready: Arc<Mutex<Option<u32>>>) {
    let hwnd = message_window(w!("DarkClipboardSpikeListener"), Some(wndproc));
    // SAFETY: hwnd valid on this thread.
    unsafe { AddClipboardFormatListener(hwnd) }.expect("AddClipboardFormatListener");
    *ready.lock().unwrap() = Some(unsafe { GetCurrentThreadId() });
    let mut msg = MSG::default();
    // SAFETY: standard message loop.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_STOP {
                let _ = RemoveClipboardFormatListener(hwnd);
                let _ = DestroyWindow(hwnd);
                PostQuitMessage(0);
                continue;
            }
            DispatchMessageW(&msg);
        }
    }
}

fn alloc(bytes: &[u8]) -> HGLOBAL {
    // SAFETY: allocate, copy, unlock; ownership passes to the clipboard on SetClipboardData success.
    unsafe {
        let g = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).unwrap();
        let p = GlobalLock(g) as *mut u8;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        let _ = GlobalUnlock(g);
        g
    }
}

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect()
}

/// Writes (format, bytes) pairs atomically. Returns the instant just before the write.
fn write(owner: HWND, items: &[(u32, Vec<u8>)]) -> Instant {
    let t = Instant::now();
    for _ in 0..20 {
        // SAFETY: owner is a window on this thread.
        if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
            // SAFETY: clipboard open by us.
            unsafe {
                EmptyClipboard().unwrap();
                for (f, b) in items {
                    let g = alloc(b);
                    if SetClipboardData(*f, Some(HANDLE(g.0))).is_err() {
                        let _ = GlobalFree(Some(g));
                    }
                }
                let _ = CloseClipboard();
            }
            return t;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("could not open clipboard to write");
}

fn read_text(owner: HWND) -> Option<String> {
    // SAFETY: open, read within GlobalSize, close.
    unsafe {
        OpenClipboard(Some(owner)).ok()?;
        let r = (|| {
            let h = GetClipboardData(CF_UNICODETEXT).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let units = std::slice::from_raw_parts(p, GlobalSize(g) / 2);
            let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
            let s = String::from_utf16_lossy(&units[..end]);
            let _ = GlobalUnlock(g);
            Some(s)
        })();
        let _ = CloseClipboard();
        r
    }
}

fn wait_events(n: usize, timeout: Duration) -> Vec<Event> {
    let start = Instant::now();
    loop {
        let len = EVENTS.lock().unwrap().len();
        if len >= n || start.elapsed() > timeout {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(150));
    std::mem::take(&mut *EVENTS.lock().unwrap())
}

fn summarize(name: &str, evs: &[Event], t0: Option<Instant>) {
    println!("--- {name}: {} event(s)", evs.len());
    for e in evs {
        let lat = t0.map(|t| format!("{:.2} ms", e.at.duration_since(t).as_secs_f64() * 1000.0));
        println!(
            "  seq={} latency={} open_attempts={} owner={:?} foreground={:?} excluded_by={:?} text_len={:?}\n  formats={:?}",
            e.seq,
            lat.unwrap_or_else(|| "-".into()),
            e.open_attempts,
            e.owner_exe,
            e.foreground_exe,
            e.excluded_by,
            e.text_len,
            e.formats
        );
    }
}

fn main() {
    let ready = Arc::new(Mutex::new(None));
    let r2 = ready.clone();
    let listener = std::thread::spawn(move || listener_thread(r2));
    let tid = loop {
        if let Some(t) = *ready.lock().unwrap() {
            break t;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let owner = message_window(w!("DarkClipboardSpikeWriter"), Some(default_proc));
    let saved = read_text(owner);
    std::thread::sleep(Duration::from_millis(100));
    EVENTS.lock().unwrap().clear();

    // 1. Plain text from this process.
    let t = write(owner, &[(CF_UNICODETEXT, utf16z("dark clipboard spike: plain"))]);
    summarize("S1 plain text, same process", &wait_events(1, Duration::from_secs(2)), Some(t));

    // 2. Text + Darkshot exclusion set (exactly what darkshot-windows/src/clipboard.rs writes).
    let t = write(
        owner,
        &[
            (CF_UNICODETEXT, utf16z("dark clipboard spike: excluded like darkshot")),
            (reg("ExcludeClipboardContentFromMonitorProcessing"), 0u32.to_le_bytes().to_vec()),
            (reg("CanIncludeInClipboardHistory"), 0u32.to_le_bytes().to_vec()),
            (reg("CanUploadToCloudClipboard"), 0u32.to_le_bytes().to_vec()),
        ],
    );
    summarize("S2 darkshot exclusion set", &wait_events(1, Duration::from_secs(2)), Some(t));

    // 3. Only CanIncludeInClipboardHistory=0 (what password managers commonly set).
    let t = write(
        owner,
        &[
            (CF_UNICODETEXT, utf16z("dark clipboard spike: history=0")),
            (reg("CanIncludeInClipboardHistory"), 0u32.to_le_bytes().to_vec()),
        ],
    );
    summarize("S3 CanIncludeInClipboardHistory=0 only", &wait_events(1, Duration::from_secs(2)), Some(t));

    // 4. Image (tiny 2x2 24-bit DIB).
    let mut dib = Vec::new();
    for v in [40u32, 2, 2] {
        dib.extend_from_slice(&v.to_le_bytes());
    }
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&24u16.to_le_bytes());
    dib.extend_from_slice(&[0u8; 24]);
    dib.extend_from_slice(&[255u8; 16]);
    let t = write(owner, &[(CF_DIB, dib)]);
    summarize("S4 CF_DIB image", &wait_events(1, Duration::from_secs(2)), Some(t));

    // 5. Burst: 20 writes back to back.
    let seq_before = unsafe { GetClipboardSequenceNumber() };
    let t = Instant::now();
    for i in 0..20 {
        write(owner, &[(CF_UNICODETEXT, utf16z(&format!("burst {i}")))]);
    }
    let evs = wait_events(20, Duration::from_secs(3));
    let seq_after = unsafe { GetClipboardSequenceNumber() };
    println!(
        "--- S5 burst of 20 writes in {:.1} ms: {} event(s), sequence advanced by {}, distinct seqs seen {}",
        t.elapsed().as_secs_f64() * 1000.0,
        evs.len(),
        seq_after.wrapping_sub(seq_before),
        {
            let mut s: Vec<u32> = evs.iter().map(|e| e.seq).collect();
            s.dedup();
            s.len()
        }
    );

    // 6. Another process: PowerShell Set-Clipboard (text), then a file list.
    let t = Instant::now();
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", "Set-Clipboard -Value 'dark clipboard spike: from powershell'"])
        .status();
    summarize("S6 text from another process (PowerShell)", &wait_events(1, Duration::from_secs(5)), Some(t));

    let file = std::env::current_exe().unwrap();
    let t = Instant::now();
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &format!("Set-Clipboard -Path '{}'", file.display())])
        .status();
    summarize("S7 file list from another process", &wait_events(1, Duration::from_secs(5)), Some(t));

    // 8. Self-write suppression: a manager must ignore its own "copy back" writes.
    // Strategy tested: remember the sequence number right after our own write.
    write(owner, &[(CF_UNICODETEXT, utf16z("copy back"))]);
    let own_seq = unsafe { GetClipboardSequenceNumber() };
    let evs = wait_events(1, Duration::from_secs(2));
    println!(
        "--- S8 self-write suppression via sequence number: own_seq={own_seq}, event seqs={:?}, suppressible={}",
        evs.iter().map(|e| e.seq).collect::<Vec<_>>(),
        evs.iter().all(|e| e.seq == own_seq)
    );

    // Restore previous text clipboard content (best effort; non-text content is not restored).
    if let Some(s) = saved {
        write(owner, &[(CF_UNICODETEXT, utf16z(&s))]);
        println!("restored previous text clipboard ({} UTF-16 units)", s.encode_utf16().count());
    }
    std::thread::sleep(Duration::from_millis(100));
    // SAFETY: tid is the listener's thread id.
    unsafe {
        let _ = PostThreadMessageW(tid, WM_STOP, WPARAM(0), LPARAM(0));
    }
    listener.join().unwrap();
    // SAFETY: our window.
    unsafe {
        let _ = DestroyWindow(owner);
    }
    println!("SPIKE DONE");
}
