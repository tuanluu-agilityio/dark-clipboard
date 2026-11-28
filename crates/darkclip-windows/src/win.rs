//! Thin safe wrappers over the Win32 calls this crate needs. All `unsafe` of the crate lives
//! here; every block has a `// SAFETY:` line. Every OS handle is owned by an RAII guard and
//! released on every path, errors included (GC-13).
//!
//! `write` (Task 104) goes through `OwnerWindow`, `GlobalBlock` and
//! `ClipboardGuard::{open_for_write, empty, set_block}`; it never writes marker formats
//! (D-012).
//!
//! `cred_write` / `cred_read` / `cred_delete` (Task 602) wrap Credential Manager for
//! `WinSecretStore`; the block `CredReadW` returns is wiped and `CredFree`d on every path.
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
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
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
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    REG_VALUE_TYPE,
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
    process_image_path(pid)
}

/// Full image path (UTF-16, no NUL) of process `pid`, or `None` when it cannot be opened or
/// queried (protected processes, a pid that is gone). The handle is closed on every path.
/// Callers keep the file name only (`source.rs`); the path never leaves the crate.
pub(crate) fn process_image_path(pid: u32) -> Option<Vec<u16>> {
    if pid == 0 {
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

    /// The handle value (paste tests use it as a `PasteTarget` window).
    #[cfg(test)]
    pub(crate) fn value(self) -> u64 {
        self.0 as u64
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

// ---------------------------------------------------------------------------------------------
// Focus return and auto-paste (Task 205). Only `paste.rs` calls these; the decisions live there.
// No window title is read (SR-11): the class name is compared with fixed shell class names and
// dropped. The only input ever synthesized is Ctrl+A, Ctrl+C or Ctrl+V (Task 604), by
// `send_ctrl`, after `paste::run` passed every check, and the unassigned menu mask key `0xE8`
// (Task 612, `send_menu_mask`), sent only on the translate host's decision.
// ---------------------------------------------------------------------------------------------

/// Window classes of the taskbars and the desktop (shell windows that must never get a paste).
const SHELL_CLASSES: [&str; 4] = [
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Progman",
    "WorkerW",
];

fn hwnd_from(window: u64) -> Option<HWND> {
    let raw = usize::try_from(window).ok()?;
    (raw != 0).then_some(HWND(raw as *mut c_void))
}

fn hwnd_value(hwnd: HWND) -> Option<u64> {
    let raw = hwnd.0 as usize;
    if raw == 0 {
        return None;
    }
    u64::try_from(raw).ok()
}

fn is_shell_class(hwnd: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let mut buf = [0u16; 64];
    // SAFETY: `buf` is a writable buffer whose length the binding passes; a stale handle only
    // makes the call return 0.
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    let Some(name) = usize::try_from(n).ok().and_then(|n| buf.get(..n)) else {
        return false;
    };
    SHELL_CLASSES
        .iter()
        .any(|c| c.encode_utf16().eq(name.iter().copied()))
}

/// What `paste.rs` needs to know about a window; `None` when it no longer exists.
pub(crate) fn window_facts(window: u64) -> Option<crate::paste::WindowFacts> {
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetDesktopWindow, GetShellWindow, GetWindowLongW, IsIconic, IsWindow,
        IsWindowVisible, GA_ROOT, GWL_EXSTYLE, GWL_STYLE, WS_CHILD, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW,
    };
    let hwnd = hwnd_from(window)?;
    // SAFETY: `IsWindow` accepts any handle value and only reads it.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: a stale handle only makes the call fail; `pid` is a valid out-pointer.
    let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
    if thread == 0 || pid == 0 {
        return None;
    }
    // SAFETY: plain queries on a handle value; a stale handle only makes them return
    // 0 / FALSE / null. Nothing is owned, nothing to release.
    let (visible, minimized, root, style, ex_style, shell, desktop) = unsafe {
        (
            IsWindowVisible(hwnd).as_bool(),
            IsIconic(hwnd).as_bool(),
            GetAncestor(hwnd, GA_ROOT),
            GetWindowLongW(hwnd, GWL_STYLE),
            GetWindowLongW(hwnd, GWL_EXSTYLE),
            GetShellWindow(),
            GetDesktopWindow(),
        )
    };
    // The style words are bit sets; the casts keep the bits.
    let style = style as u32;
    let ex_style = ex_style as u32;
    let mut cloaked = 0u32;
    // SAFETY: `cloaked` is a writable u32 and its size is passed; on failure it stays 0
    // (not cloaked), and the other checks still apply.
    let _ = unsafe {
        DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&raw mut cloaked).cast::<c_void>(), 4)
    };
    Some(crate::paste::WindowFacts {
        pid,
        visible,
        top_level: root == hwnd && style & WS_CHILD.0 == 0,
        minimized,
        cloaked: cloaked != 0,
        tool_window: ex_style & WS_EX_TOOLWINDOW.0 != 0,
        no_activate: ex_style & WS_EX_NOACTIVATE.0 != 0,
        shell: hwnd == shell || hwnd == desktop || is_shell_class(hwnd),
    })
}

/// A token handle; closed on drop.
struct TokenHandle(HANDLE);

impl Drop for TokenHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by `OpenProcessToken` and is closed exactly once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Mandatory integrity level RID (e.g. 0x2000 medium, 0x3000 high) of a process token.
fn token_integrity(process: HANDLE) -> Option<u32> {
    use windows::Win32::Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel,
        TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::OpenProcessToken;
    let mut raw = HANDLE::default();
    // SAFETY: `process` is an open process handle (or the current-process pseudo-handle);
    // `raw` is a valid out-pointer. The token handle is owned by the guard right after.
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut raw) }.ok()?;
    let token = TokenHandle(raw);
    // u64 words: the buffer is aligned for TOKEN_MANDATORY_LABEL (pointer-aligned).
    let mut buf = [0u64; 16];
    let size = u32::try_from(std::mem::size_of_val(&buf)).ok()?;
    let mut needed = 0u32;
    // SAFETY: `buf` is writable for `size` bytes and outlives the call; `needed` is a valid
    // out-pointer. A label larger than the buffer makes the call fail (handled as unknown).
    unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            Some(buf.as_mut_ptr().cast::<c_void>()),
            size,
            &raw mut needed,
        )
    }
    .ok()?;
    // SAFETY: the call succeeded, so `buf` starts with a TOKEN_MANDATORY_LABEL whose SID points
    // into `buf`, which is still alive; the SID functions only read it, and the sub-authority
    // count is checked before the last one is read.
    let rid = unsafe {
        let label = &*buf.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
        let sid = label.Label.Sid;
        if sid.0.is_null() {
            return None;
        }
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return None;
        }
        *GetSidSubAuthority(sid, u32::from(count - 1))
    };
    drop(token);
    Some(rid)
}

/// Integrity level of another process; `None` when it cannot be opened or read (the caller
/// then treats it as higher than ours).
pub(crate) fn process_integrity(pid: u32) -> Option<u32> {
    // SAFETY: plain call with the least right that allows opening the token for query.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let process = ProcessHandle(process);
    token_integrity(process.0)
}

/// Integrity level of this process.
pub(crate) fn own_integrity() -> Option<u32> {
    use windows::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: no arguments; the pseudo-handle needs no closing.
    let me = unsafe { GetCurrentProcess() };
    token_integrity(me)
}

/// True while Shift, Alt (VK_MENU covers both Alt keys, so AltGr too), a Win key or Enter is
/// down (they would change or follow the paste).
pub(crate) fn paste_blocking_keys_held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN, VK_SHIFT,
    };
    [VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN, VK_RETURN]
        .iter()
        // SAFETY: plain value argument; only reads the key state (the high bit means down).
        .any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } < 0)
}

/// True while Alt (VK_MENU covers both Alt keys, so AltGr too) or Enter is down.
pub(crate) fn focus_blocking_keys_held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU, VK_RETURN};
    [VK_MENU, VK_RETURN]
        .iter()
        // SAFETY: plain value argument; only reads the key state (the high bit means down).
        .any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } < 0)
}

/// `SetForegroundWindow`; true when Windows says the window came to the front.
pub(crate) fn set_foreground(window: u64) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
    let Some(hwnd) = hwnd_from(window) else {
        return false;
    };
    // SAFETY: a stale handle only makes the call fail.
    unsafe { SetForegroundWindow(hwnd) }.as_bool()
}

/// The foreground window's handle value.
pub(crate) fn foreground() -> Option<u64> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    // SAFETY: no arguments; the handle is not owned by the caller.
    let hwnd = unsafe { GetForegroundWindow() };
    hwnd_value(hwnd)
}

fn key_event(
    vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY,
    up: bool,
) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC,
    };
    // SAFETY: plain value arguments; returns the scan code of the key, or 0 if there is none.
    let scan = unsafe { MapVirtualKeyW(u32::from(vk.0), MAPVK_VK_TO_VSC) };
    // Scan codes from MAPVK_VK_TO_VSC fit in 16 bits; 0 (unknown) is what was sent before.
    let scan = u16::try_from(scan).unwrap_or(0);
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Sends Ctrl down, `key` down, `key` up, Ctrl up as one `SendInput` batch (not interleaved
/// with user input). If Windows inserted only part of it, key-ups are sent so no key stays
/// down. `key` is one of a closed set (A, C, V). Never called by tests that agents run (GC-19).
pub(crate) fn send_ctrl(key: crate::paste::CtrlKey) -> Result<(), PlatformError> {
    use crate::paste::CtrlKey;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_A, VK_C, VK_CONTROL, VK_V};
    let vk = match key {
        CtrlKey::A => VK_A,
        CtrlKey::C => VK_C,
        CtrlKey::V => VK_V,
    };
    let inputs = [
        key_event(VK_CONTROL, false),
        key_event(vk, false),
        key_event(vk, true),
        key_event(VK_CONTROL, true),
    ];
    let release = [key_event(vk, true), key_event(VK_CONTROL, true)];
    send_batch(&inputs, &release)
}

/// The menu mask (Task 612): the INPUT events of `input::menu_mask_keys`, `0xE8` down then
/// up. Building them sends nothing.
pub(crate) fn menu_mask_inputs() -> [windows::Win32::UI::Input::KeyboardAndMouse::INPUT; 2] {
    use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;
    crate::input::menu_mask_keys().map(|(vk, up)| key_event(VIRTUAL_KEY(vk), up))
}

/// Sends the menu mask key (`0xE8` down, then up) as one batch; on a partial send a key-up of
/// `0xE8` follows so it never stays down. Never called by tests that agents run (GC-19).
pub(crate) fn send_menu_mask() -> Result<(), PlatformError> {
    use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;
    let release = [key_event(VIRTUAL_KEY(crate::input::MENU_MASK_VK), true)];
    send_batch(&menu_mask_inputs(), &release)
}

/// One `SendInput` batch. If Windows inserted fewer events than given, `release` (key-ups,
/// harmless for keys already up) is sent and the error code returned.
fn send_batch(
    inputs: &[windows::Win32::UI::Input::KeyboardAndMouse::INPUT],
    release: &[windows::Win32::UI::Input::KeyboardAndMouse::INPUT],
) -> Result<(), PlatformError> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};
    let size = i32::try_from(std::mem::size_of::<INPUT>())
        .map_err(|_| PlatformError::Backend { code: -1 })?;
    // SAFETY: `inputs` is a slice of initialized keyboard INPUTs and `size` is the size of one
    // INPUT, as the API requires.
    let sent = unsafe { SendInput(inputs, size) };
    if usize::try_from(sent).ok() == Some(inputs.len()) {
        return Ok(());
    }
    // SAFETY: no arguments; reads this thread's last error before any other call.
    let code = unsafe { GetLastError() }.0;
    // SAFETY: as above, initialized key-up events (harmless if the keys are already up).
    let _ = unsafe { SendInput(release, size) };
    Err(PlatformError::Backend {
        code: i64::from(code),
    })
}

/// An open key under `HKEY_CURRENT_USER`; closed on drop (GC-13).
struct RegKey(HKEY);

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: `self.0` was opened by `RegOpenKeyExW`/`RegCreateKeyExW` and is closed once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn reg_error(e: windows::Win32::Foundation::WIN32_ERROR) -> PlatformError {
    PlatformError::Backend {
        code: i64::from(e.0),
    }
}

/// Creates (or opens) `HKCU\<subkey>` for writing.
fn reg_create(subkey: &str) -> Result<RegKey, PlatformError> {
    let sub = HSTRING::from(subkey);
    let mut key = HKEY::default();
    // SAFETY: `sub` is NUL-terminated and outlives the call; `key` is a valid out pointer.
    let r = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &sub,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    if r == ERROR_SUCCESS {
        Ok(RegKey(key))
    } else {
        Err(reg_error(r))
    }
}

/// Opens `HKCU\<subkey>` with `access`; `Ok(None)` when the key does not exist.
fn reg_open(
    subkey: &str,
    access: windows::Win32::System::Registry::REG_SAM_FLAGS,
) -> Result<Option<RegKey>, PlatformError> {
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    let sub = HSTRING::from(subkey);
    let mut key = HKEY::default();
    // SAFETY: `sub` is NUL-terminated and outlives the call; `key` is a valid out pointer.
    let r = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &sub, None, access, &mut key) };
    if r == ERROR_SUCCESS {
        Ok(Some(RegKey(key)))
    } else if r == ERROR_FILE_NOT_FOUND {
        Ok(None)
    } else {
        Err(reg_error(r))
    }
}

/// Writes a `REG_SZ` value under `HKCU\<subkey>`, creating the key when needed.
pub(crate) fn reg_set_string(subkey: &str, name: &str, value: &str) -> Result<(), PlatformError> {
    let key = reg_create(subkey)?;
    let name = HSTRING::from(name);
    // UTF-16 with the terminating NUL, as bytes.
    let bytes: Vec<u8> = value
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    // SAFETY: `key` is open for writing; `name` is NUL-terminated; `bytes` outlives the call.
    let r = unsafe { RegSetValueExW(key.0, &name, None, REG_SZ, Some(&bytes)) };
    if r == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(reg_error(r))
    }
}

/// Reads a value's type and bytes; `Ok(None)` when the key or the value is missing.
fn reg_get_raw(
    subkey: &str,
    name: &str,
) -> Result<Option<(REG_VALUE_TYPE, Vec<u8>)>, PlatformError> {
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    let Some(key) = reg_open(subkey, KEY_QUERY_VALUE)? else {
        return Ok(None);
    };
    let name = HSTRING::from(name);
    let mut kind = REG_VALUE_TYPE(0);
    let mut size = 0u32;
    // SAFETY: a size query: no data buffer; `kind` and `size` are valid out pointers.
    let r = unsafe { RegQueryValueExW(key.0, &name, None, Some(&mut kind), None, Some(&mut size)) };
    if r == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if r != ERROR_SUCCESS {
        return Err(reg_error(r));
    }
    let mut buf = vec![0u8; usize::try_from(size).unwrap_or(0)];
    // SAFETY: `buf` is `size` writable bytes; `size` is updated to the bytes written.
    let r = unsafe {
        RegQueryValueExW(
            key.0,
            &name,
            None,
            Some(&mut kind),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if r != ERROR_SUCCESS {
        return Err(reg_error(r));
    }
    buf.truncate(usize::try_from(size).unwrap_or(0));
    Ok(Some((kind, buf)))
}

/// Reads a `REG_SZ` value; `Ok(None)` when the key or the value is missing or not a string.
pub(crate) fn reg_get_string(subkey: &str, name: &str) -> Result<Option<String>, PlatformError> {
    let Some((kind, buf)) = reg_get_raw(subkey, name)? else {
        return Ok(None);
    };
    if kind != REG_SZ {
        return Ok(None);
    }
    let wide: Vec<u16> = buf
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&c| c != 0)
        .collect();
    Ok(Some(String::from_utf16_lossy(&wide)))
}

/// The first byte of a value's data (any type); `Ok(None)` when missing or empty.
pub(crate) fn reg_get_first_byte(subkey: &str, name: &str) -> Result<Option<u8>, PlatformError> {
    Ok(reg_get_raw(subkey, name)?.and_then(|(_, b)| b.first().copied()))
}

/// Deletes a value; a missing key or value counts as done.
pub(crate) fn reg_delete_value(subkey: &str, name: &str) -> Result<(), PlatformError> {
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    let Some(key) = reg_open(subkey, KEY_SET_VALUE)? else {
        return Ok(());
    };
    let name = HSTRING::from(name);
    // SAFETY: `key` is open for writing; `name` is NUL-terminated and outlives the call.
    let r = unsafe { RegDeleteValueW(key.0, &name) };
    if r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(reg_error(r))
    }
}

/// The Win32 code of a Credential Manager failure (the HRESULT when it is not a Win32 code).
fn cred_error(e: &windows::core::Error) -> PlatformError {
    use windows::Win32::Foundation::WIN32_ERROR;
    PlatformError::Backend {
        code: WIN32_ERROR::from_error(e).map_or_else(|| i64::from(e.code().0), |w| i64::from(w.0)),
    }
}

fn cred_not_found(e: &windows::core::Error) -> bool {
    use windows::Win32::Foundation::{ERROR_NOT_FOUND, WIN32_ERROR};
    WIN32_ERROR::from_error(e) == Some(ERROR_NOT_FOUND)
}

/// A UTF-16 string with its terminating NUL, in a buffer we own (`CREDENTIALW` takes `PWSTR`).
fn wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Owns the single block `CredReadW` returns; wipes the secret blob, then `CredFree`s it once.
struct CredBlock(*mut CREDENTIALW);

impl Drop for CredBlock {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        // SAFETY: `self.0` is the block `CredReadW` returned; it stays valid until `CredFree`
        // below, and nothing else refers to it now.
        let (blob, size) = unsafe { ((*self.0).CredentialBlob, (*self.0).CredentialBlobSize) };
        if !blob.is_null() {
            let len = usize::try_from(size).unwrap_or(0);
            // SAFETY: `blob` points to `size` bytes inside the block we own (the blob is
            // read/write per the `CREDENTIAL` docs); no other reference to them exists.
            unsafe { std::ptr::write_bytes(blob, 0, len) };
        }
        // SAFETY: the block came from `CredReadW` and is freed exactly once, here.
        unsafe { CredFree(self.0.cast_const().cast::<c_void>()) };
    }
}

/// `CredWriteW` of a generic, local-machine credential (Task 602). Creates or replaces it.
pub(crate) fn cred_write(target: &str, user: &str, blob: &[u8]) -> Result<(), PlatformError> {
    let mut target_w = wide_z(target);
    let mut user_w = wide_z(user);
    let size = u32::try_from(blob.len()).map_err(|_| PlatformError::Backend { code: -1 })?;
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target_w.as_mut_ptr()),
        CredentialBlobSize: size,
        // `CredWriteW` takes a const `CREDENTIALW` and only reads the blob.
        CredentialBlob: blob.as_ptr().cast_mut(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user_w.as_mut_ptr()),
        ..Default::default()
    };
    // SAFETY: `cred` and every buffer it points to (`target_w`, `user_w`, `blob`) outlive the
    // call; both strings are NUL-terminated; flags 0 (no `CRED_PRESERVE_CREDENTIAL_BLOB`).
    unsafe { CredWriteW(&cred, 0) }.map_err(|e| cred_error(&e))
}

/// `CredReadW` of a generic credential: the blob bytes, `Ok(None)` when there is none. The
/// OS block is wiped and freed on every path (`CredBlock`).
pub(crate) fn cred_read(target: &str) -> Result<Option<Zeroizing<Vec<u8>>>, PlatformError> {
    let target_w = HSTRING::from(target);
    let mut out: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: `target_w` is NUL-terminated and outlives the call; `out` is a valid out pointer;
    // flags are reserved (0).
    let r = unsafe { CredReadW(&target_w, CRED_TYPE_GENERIC, None, &mut out) };
    let block = CredBlock(out);
    if let Err(e) = r {
        return if cred_not_found(&e) {
            Ok(None)
        } else {
            Err(cred_error(&e))
        };
    }
    if block.0.is_null() {
        return Err(PlatformError::Backend { code: -1 });
    }
    // SAFETY: `CredReadW` succeeded, so `block.0` points to a valid `CREDENTIALW` that lives
    // until `block` drops.
    let (blob, size) = unsafe { ((*block.0).CredentialBlob, (*block.0).CredentialBlobSize) };
    let len = usize::try_from(size).unwrap_or(0);
    if blob.is_null() || len == 0 {
        return Ok(Some(Zeroizing::new(Vec::new())));
    }
    // SAFETY: `blob` points to `size` readable bytes inside the block, alive until `block`
    // drops (after this copy).
    let bytes = unsafe { std::slice::from_raw_parts(blob, len) };
    Ok(Some(Zeroizing::new(bytes.to_vec())))
}

/// `CredDeleteW` of a generic credential; a missing one counts as done.
pub(crate) fn cred_delete(target: &str) -> Result<(), PlatformError> {
    let target_w = HSTRING::from(target);
    // SAFETY: `target_w` is NUL-terminated and outlives the call; flags are reserved (0).
    match unsafe { CredDeleteW(&target_w, CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(e) if cred_not_found(&e) => Ok(()),
        Err(e) => Err(cred_error(&e)),
    }
}

#[cfg(test)]
mod mask_tests {
    // Builds the INPUT array only; `SendInput` is never called (GC-19).
    #[test]
    fn the_mask_inputs_are_two_keyboard_events_of_0xe8() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        };
        let inputs = super::menu_mask_inputs();
        let mut seen = Vec::new();
        for i in &inputs {
            assert_eq!(i.r#type, INPUT_KEYBOARD);
            // SAFETY: every INPUT built by `key_event` is a keyboard input, so `ki` is the
            // initialized member of the union.
            let ki = unsafe { i.Anonymous.ki };
            assert_eq!((ki.time, ki.dwExtraInfo), (0, 0));
            seen.push((ki.wVk.0, ki.dwFlags));
        }
        assert_eq!(
            seen,
            vec![(0xE8, KEYBD_EVENT_FLAGS(0)), (0xE8, KEYEVENTF_KEYUP)]
        );
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
