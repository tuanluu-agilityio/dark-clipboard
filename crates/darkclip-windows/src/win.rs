//! Thin safe wrappers over the Win32 calls this crate needs. All `unsafe` of the crate lives
//! here; every block has a `// SAFETY:` line. Every OS handle is owned by an RAII guard and
//! released on every path, errors included (GC-13).
//!
//! `write` (Task 104) goes through `OwnerWindow`, `GlobalBlock` and
//! `ClipboardGuard::{open_for_write, empty, set_block}`; it never writes marker formats
//! (D-012).
//!
//! Nothing here reads window titles (GC-06). Clipboard data is read only by
//! `ClipboardGuard::dword_value` (a 4-byte marker DWORD, `snapshot_meta`) and by `data_size` /
//! `copy_data`, which `access::read_plan` calls only for planned formats after
//! the policy decision (GC-03). Content buffers are wiped on drop and never logged.

use darkclip_platform::PlatformError;
use std::cell::RefCell;
use std::ffi::c_void;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use windows::core::HSTRING;
use windows::core::PCWSTR;
use windows::core::PWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, GlobalFree, SetLastError, ERROR_SUCCESS, HANDLE, HGLOBAL, HINSTANCE,
    HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, EnumClipboardFormats,
    GetClipboardData, GetClipboardFormatNameW, GetClipboardOwner, GetClipboardSequenceNumber,
    OpenClipboard, RegisterClipboardFormatW, RemoveClipboardFormatListener, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowThreadProcessId, PostMessageW, PostQuitMessage, RegisterClassExW, UnregisterClassW,
    HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLIPBOARDUPDATE, WNDCLASSEXW,
};
use zeroize::Zeroizing;

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

/// `RegisterClipboardFormatW`: the id of a registered format name, `None` on failure.
/// Registering an existing name returns its id (atoms compare case-insensitively). Needs no
/// open clipboard.
pub(crate) fn register_clipboard_format(name: &str) -> Option<u32> {
    let wide = HSTRING::from(name);
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call.
    let id = unsafe { RegisterClipboardFormatW(&wide) };
    (id != 0).then_some(id)
}

/// Longest registered format name: atom names are at most 255 characters.
const MAX_FORMAT_NAME: usize = 256;

/// `GetClipboardFormatNameW`: the registered name of `id`, `None` for standard formats,
/// unnamed private formats or failures. A format name is not content.
pub(crate) fn clipboard_format_name(id: u32) -> Option<String> {
    let mut buf = [0u16; MAX_FORMAT_NAME];
    // SAFETY: `buf` is a writable buffer; the wrapper passes its length as the maximum count.
    let n = unsafe { GetClipboardFormatNameW(id, &mut buf) };
    let n = usize::try_from(n).ok().filter(|&n| n > 0)?;
    String::from_utf16(buf.get(..n)?).ok()
}

/// The clipboard, open by this thread; closed on drop. One `OpenClipboard` attempt only
/// (S-5): failure is `Busy`, retries belong to the capture actor. Not `Send`: the clipboard
/// is opened per thread and must be closed by the same thread.
pub(crate) struct ClipboardGuard {
    _thread_bound: PhantomData<*const ()>,
}

/// Upper bound on enumerated formats (ids are 16-bit), so a misbehaving enumeration can never
/// loop forever.
const MAX_FORMATS: usize = 0x1_0000;

impl ClipboardGuard {
    /// Opens the clipboard with `owner` as the window that would own it after
    /// `EmptyClipboard` (`None` for reading).
    pub(crate) fn open(owner: Option<HWND>) -> Result<Self, PlatformError> {
        // SAFETY: `owner` is `None` or a live window of this thread.
        unsafe { OpenClipboard(owner) }.map_err(|_| PlatformError::Busy)?;
        Ok(Self {
            _thread_bound: PhantomData,
        })
    }

    /// Opens the clipboard for reading (no owner window).
    pub(crate) fn open_for_read() -> Result<Self, PlatformError> {
        Self::open(None)
    }

    /// Opens the clipboard for writing with `owner` as the window that owns it after
    /// `empty` (S-2: `OpenClipboard(NULL)` would make `SetClipboardData` fail). One attempt,
    /// `Busy` otherwise. `owner` was created on this thread (it is not `Send`).
    pub(crate) fn open_for_write(owner: &OwnerWindow) -> Result<Self, PlatformError> {
        Self::open(Some(owner.window.0))
    }

    /// `EmptyClipboard`: frees the previous data and makes our owner window the owner.
    pub(crate) fn empty(&self) -> Result<(), PlatformError> {
        // SAFETY: the clipboard is open by this thread (owned by `self`) with an owner window.
        unsafe { EmptyClipboard() }.map_err(|e| backend(&e))
    }

    /// `SetClipboardData(id, block)`. On success the system owns the block (it is not freed
    /// here); on failure `block` is dropped, which wipes and frees it.
    pub(crate) fn set_block(&self, id: u32, mut block: GlobalBlock) -> Result<(), PlatformError> {
        let Some(mem) = block.mem else {
            return Err(PlatformError::Backend { code: -1 });
        };
        // SAFETY: the clipboard is open by this thread and was emptied with our owner window;
        // `mem` is an unlocked GMEM_MOVEABLE block we own. On success ownership passes to the
        // system and the block is never touched again.
        unsafe { SetClipboardData(id, Some(HANDLE(mem.0))) }.map_err(|e| backend(&e))?;
        block.mem = None;
        Ok(())
    }

    /// `EnumClipboardFormats` until the end. A failure (not the end of the list) is an error,
    /// never a shortened list.
    pub(crate) fn enum_formats(&self) -> Result<Vec<u32>, PlatformError> {
        let mut out = Vec::new();
        let mut prev = 0u32;
        while out.len() < MAX_FORMATS {
            // SAFETY: plain call; clears the thread's last-error so the end of the list
            // (0 with ERROR_SUCCESS) can be told from a failure.
            unsafe { SetLastError(ERROR_SUCCESS) };
            // SAFETY: the clipboard is open by this thread (owned by `self`); `prev` is 0 or a
            // format returned by the previous call.
            let next = unsafe { EnumClipboardFormats(prev) };
            if next == 0 {
                // SAFETY: plain call; reads this thread's last-error value.
                let err = unsafe { GetLastError() };
                if err == ERROR_SUCCESS {
                    return Ok(out);
                }
                return Err(PlatformError::Backend {
                    code: i64::from(err.0),
                });
            }
            out.push(next);
            prev = next;
        }
        Err(PlatformError::Backend { code: -1 })
    }

    /// The DWORD stored in a marker format: the data handle's `GlobalSize` must be exactly 4
    /// bytes, else `None`. Only for the two DWORD marker formats; never call it for a content
    /// format. The handle belongs to the clipboard and is not freed; the lock is released on
    /// every path. If the owner offered the marker with delayed rendering, `GetClipboardData`
    /// blocks until the owner renders it, with the clipboard held open; this runs on the
    /// capture actor's thread, never a UI thread, which absorbs the wait (T-18).
    pub(crate) fn dword_value(&self, id: u32) -> Option<u32> {
        // SAFETY: the clipboard is open by this thread (owned by `self`).
        let handle = unsafe { GetClipboardData(id) }.ok()?;
        let mem = HGLOBAL(handle.0);
        // SAFETY: `mem` is the clipboard's data handle of a registered (HGLOBAL) format; an
        // invalid handle only makes the call return 0.
        let size = unsafe { GlobalSize(mem) };
        if size != 4 {
            return None;
        }
        let lock = GlobalLockGuard::lock(mem)?;
        let mut bytes = [0u8; 4];
        // SAFETY: `lock.ptr` points to the locked block of exactly 4 readable bytes; `bytes`
        // is a distinct 4-byte buffer.
        unsafe { std::ptr::copy_nonoverlapping(lock.ptr.cast::<u8>(), bytes.as_mut_ptr(), 4) };
        drop(lock);
        Some(u32::from_le_bytes(bytes))
    }

    /// The data handle of a format as a global memory handle, or `None` when the format has
    /// no data. If the owner uses delayed rendering, this blocks until it renders (capture
    /// actor thread only, see `dword_value`).
    fn data_handle(&self, id: u32) -> Option<HGLOBAL> {
        // SAFETY: the clipboard is open by this thread (owned by `self`). The handle stays
        // owned by the clipboard and is never freed here.
        let handle = unsafe { GetClipboardData(id) }.ok()?;
        (!handle.0.is_null()).then_some(HGLOBAL(handle.0))
    }

    /// `GlobalSize` of a format's data block; `None` when there is no data or the size is 0.
    /// Reads no bytes. Only for HGLOBAL formats (text, registered formats, DIBs, `CF_HDROP`).
    pub(crate) fn data_size(&self, id: u32) -> Option<u64> {
        let mem = self.data_handle(id)?;
        // SAFETY: `mem` is the clipboard's data handle of an HGLOBAL format; an invalid
        // handle only makes the call return 0.
        let size = unsafe { GlobalSize(mem) };
        u64::try_from(size).ok().filter(|&s| s > 0)
    }

    /// The first `min(GlobalSize, max)` bytes of a format's data, in a buffer wiped on drop.
    /// The size is checked before locking, the buffer is never larger than `max`, and the
    /// lock is released on every path.
    pub(crate) fn copy_data(&self, id: u32, max: usize) -> Option<Zeroizing<Vec<u8>>> {
        let mem = self.data_handle(id)?;
        // SAFETY: as in `data_size`.
        let size = unsafe { GlobalSize(mem) };
        let n = size.min(max);
        if n == 0 {
            return None;
        }
        let lock = GlobalLockGuard::lock(mem)?;
        let mut out = Zeroizing::new(vec![0u8; n]);
        // SAFETY: `lock.ptr` points to the locked block of `size >= n` readable bytes; `out`
        // is a distinct buffer of exactly `n` bytes.
        unsafe { std::ptr::copy_nonoverlapping(lock.ptr.cast::<u8>(), out.as_mut_ptr(), n) };
        drop(lock);
        Some(out)
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: this guard opened the clipboard on this thread and closes it exactly once.
        let _ = unsafe { CloseClipboard() };
    }
}

/// A locked global memory block; unlocked on drop.
struct GlobalLockGuard {
    mem: HGLOBAL,
    ptr: *const c_void,
}

impl GlobalLockGuard {
    fn lock(mem: HGLOBAL) -> Option<Self> {
        // SAFETY: `mem` is a global memory handle owned by the clipboard and valid while it
        // is open; a bad handle makes the call return null.
        let ptr = unsafe { GlobalLock(mem) };
        if ptr.is_null() {
            return None;
        }
        Some(Self {
            mem,
            ptr: ptr.cast_const(),
        })
    }
}

impl Drop for GlobalLockGuard {
    fn drop(&mut self) {
        // SAFETY: balances the successful `GlobalLock` in `lock`. Returns an "error" when the
        // lock count reaches 0, which is the expected outcome.
        let _ = unsafe { GlobalUnlock(self.mem) };
    }
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
/// there is no owner window or the process cannot be opened or queried. Elevated owners
/// usually still resolve with `PROCESS_QUERY_LIMITED_INFORMATION`; protected processes are
/// the denial case.
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

/// A `GMEM_MOVEABLE` global memory block holding one clipboard format's data. Wiped and
/// freed on drop unless `ClipboardGuard::set_block` handed it to the system.
pub(crate) struct GlobalBlock {
    mem: Option<HGLOBAL>,
    len: usize,
}

impl GlobalBlock {
    /// A new moveable block with a copy of `bytes` (at least 1 byte is allocated).
    pub(crate) fn with_bytes(bytes: &[u8]) -> Result<Self, PlatformError> {
        // SAFETY: plain allocation call; the result is checked.
        let mem =
            unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) }.map_err(|e| backend(&e))?;
        // Owned from here: freed on every path below.
        let block = Self {
            mem: Some(mem),
            len: bytes.len(),
        };
        let lock = GlobalLockGuard::lock(mem)
            .ok_or_else(|| backend(&windows::core::Error::from_thread()))?;
        // SAFETY: `lock.ptr` points to a locked block of at least `bytes.len()` writable bytes
        // that does not overlap `bytes`.
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                lock.ptr.cast_mut().cast::<u8>(),
                bytes.len(),
            );
        };
        drop(lock);
        Ok(block)
    }
}

// SAFETY: a `GlobalAlloc` handle is a process-wide heap handle, not bound to the thread that
// allocated it; `GlobalBlock` owns it exclusively (no aliasing), so moving it to another
// thread is sound. Needed because `WinAccess` (which is `Send`) may keep blocks between the
// `Busy` retries of one copy-back.
unsafe impl Send for GlobalBlock {}

impl Drop for GlobalBlock {
    fn drop(&mut self) {
        let Some(mem) = self.mem.take() else {
            return;
        };
        // The data is content: overwrite it before the memory goes back to the heap.
        if let Some(lock) = GlobalLockGuard::lock(mem) {
            // SAFETY: `lock.ptr` points to the locked block of at least `len` writable bytes.
            unsafe { std::ptr::write_bytes(lock.ptr.cast_mut().cast::<u8>(), 0, self.len) };
            drop(lock);
        }
        // SAFETY: the block was allocated by `with_bytes`, is unlocked, and the system does not
        // own it (`set_block` clears `mem` on success); it is freed exactly once, here.
        let _ = unsafe { GlobalFree(Some(mem)) };
    }
}

/// The clipboard owner window of one `write` (S-2, D-038): a message-only window without a
/// clipboard listener, created on the writing thread for that write only. Not `Send`.
/// Fields drop in declaration order: the window is destroyed, then its class unregistered.
/// The caller drops it after `CloseClipboard`; the written data stays on the clipboard
/// because nothing is delayed-rendered, and the clipboard owner becomes NULL, so no
/// `WM_DESTROYCLIPBOARD` is ever sent to a thread that does not pump messages.
pub(crate) struct OwnerWindow {
    window: WindowGuard,
    _class: ClassGuard,
}

impl OwnerWindow {
    /// Registers a class and creates a message-only window on this thread. On error every
    /// step already done is undone.
    pub(crate) fn create() -> Result<Self, PlatformError> {
        let class = ClassGuard::register()?;
        let window = WindowGuard::create(&class)?;
        Ok(Self {
            window,
            _class: class,
        })
    }

    /// `GetClipboardOwner()` is this window: nobody has emptied the clipboard since our write.
    pub(crate) fn owns_clipboard(&self) -> bool {
        // SAFETY: no arguments; returns a window handle or null, and does not open the
        // clipboard.
        unsafe { GetClipboardOwner() }.is_ok_and(|h| h == self.window.0)
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

    /// `IsWindow`: the handle still names an existing window (teardown tests). A destroyed
    /// window's handle value may in theory be reused later by another window.
    #[cfg(test)]
    pub(crate) fn is_window(self) -> bool {
        let hwnd = HWND(self.0 as *mut c_void);
        // SAFETY: `IsWindow` accepts any handle value, valid or not, and only reads it.
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)) }.as_bool()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Test-only clipboard writer for the ignored `real_clipboard_write_*` tests.

    /// Number of handles open in this process (leak tests).
    pub(crate) fn process_handle_count() -> u32 {
        use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
        let mut n = 0u32;
        // SAFETY: the pseudo-handle of the current process is always valid; `n` is a valid
        // out-pointer.
        let _ = unsafe { GetProcessHandleCount(GetCurrentProcess(), &raw mut n) };
        n
    }

    use super::{
        clipboard_sequence, register_clipboard_format, ClipboardGuard, GlobalBlock, MessageWindow,
    };
    use crate::formats::{CF_UNICODETEXT, CLOUD, EXCLUDE_MONITOR, HISTORY};
    use darkclip_platform::PlatformError;
    use windows::Win32::Foundation::HWND;

    /// Replaces the clipboard with `formats` (in order); returns the sequence number after.
    fn write_formats(formats: Vec<(u32, Vec<u8>)>) -> Result<u32, PlatformError> {
        let owner = MessageWindow::create(None)?;
        let blocks = formats
            .into_iter()
            .map(|(id, bytes)| GlobalBlock::with_bytes(&bytes).map(|b| (id, b)))
            .collect::<Result<Vec<_>, _>>()?;
        let open = ClipboardGuard::open(Some(HWND(owner.raw().0 as *mut _)))?;
        open.empty()?;
        for (id, block) in blocks {
            open.set_block(id, block)?;
        }
        drop(open);
        Ok(clipboard_sequence())
    }

    fn utf16z(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    /// Writes `text` as `CF_UNICODETEXT`; returns the sequence number after the write.
    pub(crate) fn write_text(text: &str) -> Result<u32, PlatformError> {
        write_formats(vec![(CF_UNICODETEXT, utf16z(text))])
    }

    /// Writes a packed DIB as `CF_DIB` only (the system synthesizes `CF_DIBV5` on request).
    pub(crate) fn write_dib(dib: &[u8]) -> Result<u32, PlatformError> {
        write_formats(vec![(crate::formats::CF_DIB, dib.to_vec())])
    }

    /// Writes `html` (a full CF_HTML payload, UTF-8) as `HTML Format`, NUL-terminated.
    pub(crate) fn write_html(html: &str) -> Result<u32, PlatformError> {
        let id = register_clipboard_format(crate::formats::HTML_FORMAT)
            .ok_or(PlatformError::Backend { code: -1 })?;
        let mut bytes = html.as_bytes().to_vec();
        bytes.push(0);
        write_formats(vec![(id, bytes)])
    }

    /// Writes `text` plus Darkshot's exclusion set as `darkshot-windows/src/clipboard.rs` does:
    /// the main format first, then `ExcludeClipboardContentFromMonitorProcessing`,
    /// `CanIncludeInClipboardHistory`, `CanUploadToCloudClipboard`, each a DWORD 0.
    pub(crate) fn write_darkshot_text_set(text: &str) -> Result<u32, PlatformError> {
        let mut formats = vec![(CF_UNICODETEXT, utf16z(text))];
        for name in [EXCLUDE_MONITOR, HISTORY, CLOUD] {
            let id = register_clipboard_format(name).ok_or(PlatformError::Backend { code: -1 })?;
            formats.push((id, 0u32.to_le_bytes().to_vec()));
        }
        write_formats(formats)
    }
}
