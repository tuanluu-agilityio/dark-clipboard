//! Thin safe wrappers over the Win32 calls this crate needs. All `unsafe` of the crate lives
//! here; every block has a `// SAFETY:` line. Every OS handle is owned by an RAII guard and
//! released on every path, errors included (GC-13).
//!
//! Nothing here reads clipboard content or window titles (GC-06).

use darkclip_platform::PlatformError;
use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use windows::core::PCWSTR;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, GetClipboardOwner, GetClipboardSequenceNumber,
    RemoveClipboardFormatListener,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowThreadProcessId, PostMessageW, PostQuitMessage, RegisterClassExW, UnregisterClassW,
    HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLIPBOARDUPDATE, WNDCLASSEXW,
};

/// Private message that ends the message loop (posted by `RawWindow::post_stop`).
const WM_DARKCLIP_STOP: u32 = WM_APP + 0x0D1;

/// Called on the window's thread for every `WM_CLIPBOARDUPDATE`; `false` ends the loop.
pub(crate) type UpdateHandler = Box<dyn FnMut() -> bool>;

thread_local! {
    /// The update handler of the message window owned by this thread (at most one).
    static HANDLER: RefCell<Option<UpdateHandler>> = const { RefCell::new(None) };
}

/// Unique class names: tests (and restarts) create several windows in one process.
static CLASS_COUNTER: AtomicU64 = AtomicU64::new(0);

fn backend(e: &windows::core::Error) -> PlatformError {
    PlatformError::Backend {
        code: i64::from(e.code().0),
    }
}

/// `GetClipboardSequenceNumber`: never fails, needs no clipboard open (F-03).
pub(crate) fn clipboard_sequence() -> u32 {
    // SAFETY: no arguments, no preconditions; reads a per-session counter.
    unsafe { GetClipboardSequenceNumber() }
}

/// A process handle; closed on drop.
struct ProcessHandle(HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by `OpenProcess` and is closed exactly once, here.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Longest path `QueryFullProcessImageNameW` can return (32 767 characters plus NUL).
const MAX_IMAGE_PATH: usize = 32_768;

/// Full image path (UTF-16, no NUL) of the process that owns the clipboard, or `None` when
/// there is no owner window or the process cannot be opened (elevated, protected) or queried.
/// Only `source.rs` calls this and it keeps the file name only; the path never leaves the
/// crate. Does not open the clipboard and never reads a window title.
pub(crate) fn clipboard_owner_image_path() -> Option<Vec<u16>> {
    // SAFETY: no arguments; returns a window handle or null, and does not open the clipboard.
    let owner = unsafe { GetClipboardOwner() }.ok()?;
    let mut pid = 0u32;
    // SAFETY: `owner` is a handle value (it may be stale, which only makes the call fail);
    // `pid` is a valid out-pointer.
    let thread = unsafe { GetWindowThreadProcessId(owner, Some(&raw mut pid)) };
    if thread == 0 || pid == 0 {
        return None;
    }
    // SAFETY: plain call; the right requested is the least that allows the image name query.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let process = ProcessHandle(process);
    let mut buf = vec![0u16; MAX_IMAGE_PATH];
    let mut len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
    // SAFETY: the process handle is open (owned by `process`); `buf` holds `len` writable
    // u16s; `len` is a valid in/out pointer.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buf.as_mut_ptr()),
            &raw mut len,
        )
    }
    .ok()?;
    buf.truncate(usize::try_from(len).unwrap_or(0));
    Some(buf)
}

/// Window procedure of the monitor class. Runs only on the thread that owns the window.
extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        // A reentrant notification (the handler pumped messages) is dropped: the next one
        // carries the newer sequence number anyway.
        let keep = HANDLER.with(|h| match h.try_borrow_mut() {
            Ok(mut slot) => slot.as_mut().is_none_or(|f| f()),
            Err(_) => true,
        });
        if !keep {
            // SAFETY: no preconditions; posts WM_QUIT to this thread's queue.
            unsafe { PostQuitMessage(0) };
        }
        return LRESULT(0);
    }
    // SAFETY: forwarding the unmodified arguments the system passed to this window procedure.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// A registered window class; unregistered on drop (after its window is destroyed).
struct ClassGuard {
    name: Vec<u16>,
    hinstance: HINSTANCE,
}

impl ClassGuard {
    fn register() -> Result<Self, PlatformError> {
        // SAFETY: `None` asks for the handle of the current executable; nothing is borrowed.
        let module = unsafe { GetModuleHandleW(PCWSTR::null()) }.map_err(|e| backend(&e))?;
        let hinstance = HINSTANCE(module.0);
        let n = CLASS_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name: Vec<u16> = format!("DarkClipboardMonitor.{}.{n}", std::process::id())
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let wc = WNDCLASSEXW {
            cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>()).unwrap_or(u32::MAX),
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            lpszClassName: PCWSTR(name.as_ptr()),
            ..Default::default()
        };
        // SAFETY: `wc` is fully initialized and `name` (NUL-terminated) outlives the call; the
        // class keeps its own copy of the name.
        let atom = unsafe { RegisterClassExW(&raw const wc) };
        if atom == 0 {
            return Err(backend(&windows::core::Error::from_thread()));
        }
        Ok(Self { name, hinstance })
    }
}

impl Drop for ClassGuard {
    fn drop(&mut self) {
        // SAFETY: the name is the NUL-terminated name this guard registered; the window of the
        // class is destroyed first (field order in `MessageWindow`). A failure leaves the class
        // registered until process exit, which the OS cleans up.
        let _ = unsafe { UnregisterClassW(PCWSTR(self.name.as_ptr()), Some(self.hinstance)) };
    }
}

/// A message-only window; destroyed on drop.
struct WindowGuard(HWND);

impl WindowGuard {
    fn create(class: &ClassGuard) -> Result<Self, PlatformError> {
        // SAFETY: the class is registered by `class` (alive for the call); `HWND_MESSAGE` makes
        // a message-only window; no creation parameter is passed. The empty title is a static
        // NUL-terminated literal.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class.name.as_ptr()),
                windows::core::w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(class.hinstance),
                None,
            )
        }
        .map_err(|e| backend(&e))?;
        Ok(Self(hwnd))
    }
}

impl Drop for WindowGuard {
    fn drop(&mut self) {
        // SAFETY: the window was created by this thread and is destroyed exactly once, here.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

/// Clipboard format listener registration; removed on drop.
struct ListenerGuard(HWND);

impl ListenerGuard {
    fn add(window: &WindowGuard) -> Result<Self, PlatformError> {
        // SAFETY: the window is alive (owned by `window`) and belongs to this thread.
        unsafe { AddClipboardFormatListener(window.0) }.map_err(|e| backend(&e))?;
        Ok(Self(window.0))
    }
}

impl Drop for ListenerGuard {
    fn drop(&mut self) {
        // SAFETY: the window is still alive (it is dropped after this guard) and listening.
        let _ = unsafe { RemoveClipboardFormatListener(self.0) };
    }
}

/// This thread's update handler; cleared on drop.
struct HandlerGuard {
    installed: bool,
}

impl HandlerGuard {
    fn install(on_update: Option<UpdateHandler>) -> Result<Self, PlatformError> {
        let Some(f) = on_update else {
            return Ok(Self { installed: false });
        };
        HANDLER.with(|h| {
            let mut slot = h
                .try_borrow_mut()
                .map_err(|_| PlatformError::Unsupported("monitor window busy"))?;
            if slot.is_some() {
                return Err(PlatformError::Unsupported("one monitor window per thread"));
            }
            *slot = Some(f);
            Ok(Self { installed: true })
        })
    }
}

impl Drop for HandlerGuard {
    fn drop(&mut self) {
        if self.installed {
            HANDLER.with(|h| {
                if let Ok(mut slot) = h.try_borrow_mut() {
                    *slot = None;
                }
            });
        }
    }
}

/// A message-only window registered as a clipboard format listener. Not `Send`: it lives
/// and dies on the thread that created it. Fields drop in declaration order: listener
/// removed, window destroyed, class unregistered, handler cleared.
pub(crate) struct MessageWindow {
    listener: ListenerGuard,
    window: WindowGuard,
    _class: ClassGuard,
    _handler: HandlerGuard,
}

impl MessageWindow {
    /// Registers a class, creates a message-only window and adds the clipboard listener.
    /// `on_update` (if any) runs on this thread for every `WM_CLIPBOARDUPDATE`. On error every
    /// step already done is undone.
    pub(crate) fn create(on_update: Option<UpdateHandler>) -> Result<Self, PlatformError> {
        let handler = HandlerGuard::install(on_update)?;
        let class = ClassGuard::register()?;
        let window = WindowGuard::create(&class)?;
        let listener = ListenerGuard::add(&window)?;
        Ok(Self {
            listener,
            window,
            _class: class,
            _handler: handler,
        })
    }

    pub(crate) fn raw(&self) -> RawWindow {
        RawWindow(self.window.0 .0 as usize)
    }

    /// Runs the message loop until the stop message, `WM_QUIT` or a `GetMessageW` error.
    pub(crate) fn run(&self) {
        let mut msg = MSG::default();
        loop {
            // SAFETY: `msg` is a valid out-pointer; `None` reads every message of this thread.
            let r = unsafe { GetMessageW(&raw mut msg, None, 0, 0) };
            if r.0 == 0 || r.0 == -1 {
                return;
            }
            if msg.message == WM_DARKCLIP_STOP && msg.hwnd == self.listener.0 {
                return;
            }
            // SAFETY: `msg` was just filled by `GetMessageW` on this thread.
            unsafe { DispatchMessageW(&raw const msg) };
        }
    }
}

/// The window handle as a plain value, so another thread can post the stop message. The
/// caller guarantees (`monitor.rs`, under a mutex) that the window is alive when posting.
#[derive(Clone, Copy)]
pub(crate) struct RawWindow(usize);

impl RawWindow {
    /// Posts the private stop message. `false` if the post failed.
    pub(crate) fn post_stop(self) -> bool {
        let hwnd = HWND(self.0 as *mut c_void);
        // SAFETY: the caller guarantees the window is alive; posting to a window of another
        // thread is allowed and does not block.
        unsafe { PostMessageW(Some(hwnd), WM_DARKCLIP_STOP, WPARAM(0), LPARAM(0)) }.is_ok()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Test-only clipboard writer for the ignored `real_clipboard_write_*` tests.

    use super::{backend, clipboard_sequence, MessageWindow};
    use darkclip_platform::PlatformError;
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    /// `CF_UNICODETEXT` (standard clipboard format 13).
    const CF_UNICODETEXT: u32 = 13;

    /// An open clipboard; closed on drop. One attempt only (S-5).
    struct ClipboardGuard;

    impl ClipboardGuard {
        fn open(owner: HWND) -> Result<Self, PlatformError> {
            // SAFETY: `owner` is a live window of this thread.
            unsafe { OpenClipboard(Some(owner)) }.map_err(|_| PlatformError::Busy)?;
            Ok(Self)
        }
    }

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            // SAFETY: this guard opened the clipboard on this thread.
            let _ = unsafe { CloseClipboard() };
        }
    }

    /// A global memory block; freed on drop unless ownership passed to the clipboard.
    struct GlobalGuard(Option<HGLOBAL>);

    impl Drop for GlobalGuard {
        fn drop(&mut self) {
            if let Some(h) = self.0.take() {
                // SAFETY: the block was allocated here and the clipboard does not own it.
                let _ = unsafe { GlobalFree(Some(h)) };
            }
        }
    }

    /// Writes `text` as `CF_UNICODETEXT`; returns the sequence number after the write.
    pub(crate) fn write_text(text: &str) -> Result<u32, PlatformError> {
        let owner = MessageWindow::create(None)?;
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = wide.len() * 2;
        // SAFETY: plain allocation call.
        let mem = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }.map_err(|e| backend(&e))?;
        let mut block = GlobalGuard(Some(mem));
        // SAFETY: `mem` is a live moveable block of `bytes` bytes.
        let ptr = unsafe { GlobalLock(mem) }.cast::<u16>();
        if ptr.is_null() {
            return Err(backend(&windows::core::Error::from_thread()));
        }
        // SAFETY: `ptr` points to `bytes` writable bytes (`wide.len()` u16s); no overlap.
        unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len()) };
        // SAFETY: balanced with the lock above. Returns an "error" when the count reaches 0.
        let _ = unsafe { GlobalUnlock(mem) };
        let _open = ClipboardGuard::open(HWND(owner.raw().0 as *mut _))?;
        // SAFETY: the clipboard is open by this thread with a window owner.
        unsafe { EmptyClipboard() }.map_err(|e| backend(&e))?;
        // SAFETY: the clipboard is open and emptied by us; on success the system owns `mem`.
        unsafe { SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0))) }
            .map_err(|e| backend(&e))?;
        block.0 = None;
        drop(_open);
        Ok(clipboard_sequence())
    }
}
