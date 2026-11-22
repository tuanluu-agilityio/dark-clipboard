//! Single instance and the local IPC base (ADR-0006).
//!
//! * The wire protocol is fixed and versioned: exactly 2 bytes, `[version, command]`. Anything
//!   else (other length, other version, unknown command) is dropped without a reply.
//! * `start_or_forward` holds the decision logic behind the small [`Transport`] trait, so it is
//!   tested without a second process.
//! * The Windows transport (and the OS UI-language lookup, one more FFI call) (per-user mutex plus a named pipe whose DACL grants the current
//!   user's SID only) is the only `unsafe` code of the desktop app. It lives in the marked
//!   `win` module below; every `unsafe` block carries a `// SAFETY:` line. No network.
//!
//! Nothing here sees clipboard data: the only thing that crosses the pipe is a command byte.
// The Windows transport is cfg(windows); other platforms get it in Phase 4.
#![cfg_attr(not(windows), allow(dead_code))]

/// Protocol version byte (first byte of every message).
pub const PROTOCOL_VERSION: u8 = 1;
/// A valid message is exactly this long.
pub const MESSAGE_LEN: usize = 2;

/// What a second instance can ask the running one to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Toggle,
    ShowSettings,
    Quit,
}

impl Command {
    fn byte(self) -> u8 {
        match self {
            Command::Toggle => 1,
            Command::ShowSettings => 2,
            Command::Quit => 3,
        }
    }

    /// Static log name.
    pub fn name(self) -> &'static str {
        match self {
            Command::Toggle => "toggle",
            Command::ShowSettings => "show_settings",
            Command::Quit => "quit",
        }
    }
}

pub fn encode(c: Command) -> [u8; MESSAGE_LEN] {
    [PROTOCOL_VERSION, c.byte()]
}

/// `None` for a wrong length (too short or oversize), a wrong version or an unknown command.
pub fn decode(msg: &[u8]) -> Option<Command> {
    match msg {
        [PROTOCOL_VERSION, 1] => Some(Command::Toggle),
        [PROTOCOL_VERSION, 2] => Some(Command::ShowSettings),
        [PROTOCOL_VERSION, 3] => Some(Command::Quit),
        _ => None,
    }
}

/// The OS part of single instance, behind a trait so the logic is testable.
pub trait Transport {
    /// Holds the instance lock; dropping it releases the lock.
    type Guard;
    /// `Ok(Some(guard))`: we are the first instance. `Ok(None)`: another instance holds it.
    fn try_lock(&self) -> Result<Option<Self::Guard>, ()>;
    /// Delivers one command to the instance that holds the lock.
    fn send(&self, cmd: Command) -> Result<(), ()>;
}

/// Result of [`start_or_forward`].
#[derive(Debug, PartialEq, Eq)]
pub enum Start<G> {
    /// No other instance: the caller runs the app and keeps the guard alive.
    Primary(G),
    /// Another instance exists and received the command.
    Forwarded,
    /// `start_if_free` was false and nothing is running.
    NotRunning,
    /// The lock or the delivery failed.
    Failed,
}

/// Takes the lock when free; otherwise forwards `cmd` to the running instance. With
/// `start_if_free == false` (`--toggle`, `--quit`) a free lock means nothing is running, and
/// the lock is released again.
pub fn start_or_forward<T: Transport>(t: &T, cmd: Command, start_if_free: bool) -> Start<T::Guard> {
    match t.try_lock() {
        Ok(Some(guard)) => {
            if start_if_free {
                Start::Primary(guard)
            } else {
                drop(guard);
                Start::NotRunning
            }
        }
        Ok(None) => match t.send(cmd) {
            Ok(()) => Start::Forwarded,
            Err(()) => Start::Failed,
        },
        Err(()) => Start::Failed,
    }
}

/// FNV-1a, 64 bit, as 16 hex digits. A name component, not a security primitive.
pub fn name_hash(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.bytes().chain(std::iter::once(0)) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

#[cfg(windows)]
pub use win::{ui_language_id, WinTransport};

/// UNSAFE MODULE: Win32 mutex and named pipe. Safe wrappers only leave this module.
#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
    use super::{decode, encode, name_hash, Command, Transport, MESSAGE_LEN};
    use crate::log::Log;
    use std::ffi::c_void;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use windows::core::{HRESULT, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, LocalFree, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND,
        ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL, INVALID_HANDLE_VALUE,
    };
    use windows::Win32::Globalization::GetUserDefaultUILanguage;
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows::Win32::Security::{
        GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
        TOKEN_USER,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_FIRST_PIPE_INSTANCE,
        FILE_SHARE_NONE, OPEN_EXISTING, PIPE_ACCESS_INBOUND, SECURITY_IDENTIFICATION,
        SECURITY_SQOS_PRESENT,
    };
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, WaitNamedPipeW,
        PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_WAIT,
    };
    use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, OpenProcessToken};

    /// `GENERIC_WRITE`.
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const MAX_INSTANCES: u32 = 255;
    /// Handler threads alive at once; extra connections are closed unread.
    const MAX_HANDLERS: usize = 4;
    /// Bigger than a valid message so an oversize one is seen as oversize.
    const READ_BUF: usize = 16;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Closes the handle on every path.
    struct Owned(HANDLE);

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: the handle is valid and owned by this value; it is closed exactly once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// The current user's SID as a string (`S-1-5-...`).
    fn current_user_sid() -> Option<String> {
        // SAFETY: every pointer passed points to live local storage of the right type and
        // size; the token handle is closed by `Owned`; the string allocated by Windows is
        // copied and then released with `LocalFree`.
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
            let token = Owned(token);
            let mut len = 0u32;
            // The first call only reports the size needed (and fails by design).
            let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut len);
            if len == 0 {
                return None;
            }
            // u64 words so the buffer is aligned for TOKEN_USER.
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            GetTokenInformation(
                token.0,
                TokenUser,
                Some(buf.as_mut_ptr().cast::<c_void>()),
                len,
                &mut len,
            )
            .ok()?;
            let user = &*(buf.as_ptr().cast::<TOKEN_USER>());
            let mut text = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut text).ok()?;
            let sid = text.to_string().ok();
            let _ = LocalFree(Some(HLOCAL(text.0.cast::<c_void>())));
            sid
        }
    }

    /// The user's Windows display language id (LANGID).
    pub fn ui_language_id() -> u16 {
        // SAFETY: takes no arguments and reads no memory of ours.
        unsafe { GetUserDefaultUILanguage() }
    }

    pub struct WinTransport {
        mutex_name: Vec<u16>,
        pipe_name: Vec<u16>,
        sddl: Vec<u16>,
    }

    /// Holds the instance mutex; released on drop.
    pub struct LockGuard(#[allow(dead_code)] Owned);

    impl WinTransport {
        /// `scope` separates instances that must not see each other (a `--data-dir` run, used
        /// by tests); `None` is the normal per-user instance. The mutex name holds a hash of
        /// the SID, never a user name.
        pub fn new(scope: Option<&Path>) -> Option<WinTransport> {
            let sid = current_user_sid()?;
            let scope_text = scope.map(|p| p.to_string_lossy().to_lowercase());
            let scope_hash = scope_text.as_deref().map(|s| name_hash(&["scope", s]));
            let mutex = format!(
                "Local\\DarkClipboard-{}{}",
                name_hash(&["sid", &sid]),
                scope_hash
                    .as_deref()
                    .map_or(String::new(), |h| format!("-{h}"))
            );
            let pipe = format!(
                "\\\\.\\pipe\\dark-clipboard-{sid}{}",
                scope_hash
                    .as_deref()
                    .map_or(String::new(), |h| format!("-{h}"))
            );
            // Protected DACL: one allow entry, GENERIC_ALL for this user's SID only.
            let sddl = format!("D:P(A;;GA;;;{sid})");
            Some(WinTransport {
                mutex_name: wide(&mutex),
                pipe_name: wide(&pipe),
                sddl: wide(&sddl),
            })
        }

        /// Creates one pipe instance restricted to the current user. `first` makes creation
        /// fail when the name already exists (somebody else created the pipe first).
        fn create_instance(&self, first: bool) -> Option<HANDLE> {
            let mut open = PIPE_ACCESS_INBOUND;
            if first {
                open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
            }
            // SAFETY: `sddl` and `pipe_name` are NUL-terminated and outlive the calls; the
            // descriptor allocated by Windows is released with `LocalFree` after the pipe has
            // copied it; `sa` lives across the call.
            unsafe {
                let mut sd = PSECURITY_DESCRIPTOR::default();
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    PCWSTR(self.sddl.as_ptr()),
                    SDDL_REVISION_1,
                    &mut sd,
                    None,
                )
                .ok()?;
                let sa = SECURITY_ATTRIBUTES {
                    nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: sd.0,
                    bInheritHandle: false.into(),
                };
                let h = CreateNamedPipeW(
                    PCWSTR(self.pipe_name.as_ptr()),
                    open,
                    PIPE_TYPE_MESSAGE
                        | PIPE_READMODE_MESSAGE
                        | PIPE_WAIT
                        | PIPE_REJECT_REMOTE_CLIENTS,
                    MAX_INSTANCES,
                    0,
                    READ_BUF as u32,
                    0,
                    Some(&sa),
                );
                let _ = LocalFree(Some(HLOCAL(sd.0)));
                if h == INVALID_HANDLE_VALUE {
                    None
                } else {
                    Some(h)
                }
            }
        }

        /// Creates the first pipe instance now (so a second instance can connect right after
        /// the lock is taken) and serves it on a background thread. `on_command` runs on that
        /// thread for every valid message; it must only hand the command over (no blocking).
        pub fn listen(
            &self,
            log: Arc<Log>,
            on_command: impl Fn(Command) + Send + Sync + 'static,
        ) -> Result<(), ()> {
            let first = self.create_instance(true).ok_or(())?;
            let server = Server {
                t: WinTransport {
                    mutex_name: Vec::new(),
                    pipe_name: self.pipe_name.clone(),
                    sddl: self.sddl.clone(),
                },
                on_command: Arc::new(on_command),
                log,
            };
            let handle = first.0 as isize;
            std::thread::Builder::new()
                .name("darkclip-ipc".into())
                .spawn(move || server.run(handle))
                .map(|_| ())
                .map_err(|_| ())
        }
    }

    struct Server {
        t: WinTransport,
        on_command: Arc<dyn Fn(Command) + Send + Sync>,
        log: Arc<Log>,
    }

    fn handle_from(raw: isize) -> HANDLE {
        HANDLE(raw as *mut c_void)
    }

    /// Reads one message from a connected instance, validates it, closes the instance.
    fn serve_connection(raw: isize, on_command: &(dyn Fn(Command) + Send + Sync), log: &Log) {
        let conn = Owned(handle_from(raw));
        let mut buf = [0u8; READ_BUF];
        let mut got = 0u32;
        // SAFETY: `buf` and `got` are live locals; the handle is a connected pipe instance.
        let read = unsafe { ReadFile(conn.0, Some(&mut buf), Some(&mut got), None) };
        // An oversize message makes ReadFile fail with ERROR_MORE_DATA: that is a drop too.
        let msg = read.ok().and_then(|()| buf.get(..got as usize));
        match msg.and_then(decode) {
            Some(cmd) => on_command(cmd),
            None => log.event("ipc_dropped", &[]),
        }
        // SAFETY: the handle is the connected instance; it is closed by `conn` right after.
        unsafe {
            let _ = DisconnectNamedPipe(conn.0);
        }
    }

    impl Server {
        fn run(self, first: isize) {
            static HANDLERS: AtomicUsize = AtomicUsize::new(0);
            let mut cur = first;
            loop {
                // SAFETY: `cur` is a pipe instance created by `create_instance`.
                let connected = unsafe { ConnectNamedPipe(handle_from(cur), None) };
                let ok = match connected {
                    Ok(()) => true,
                    // The client connected between create and connect: fine.
                    Err(e) => e.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0),
                };
                if !ok {
                    drop(Owned(handle_from(cur)));
                    match self.t.create_instance(false) {
                        Some(h) => {
                            cur = h.0 as isize;
                            continue;
                        }
                        None => break,
                    }
                }
                let next = self.t.create_instance(false);
                if HANDLERS.fetch_add(1, Ordering::SeqCst) >= MAX_HANDLERS {
                    HANDLERS.fetch_sub(1, Ordering::SeqCst);
                    drop(Owned(handle_from(cur)));
                } else {
                    let (cb, log, conn) =
                        (Arc::clone(&self.on_command), Arc::clone(&self.log), cur);
                    let spawned = std::thread::Builder::new()
                        .name("darkclip-ipc-conn".into())
                        .spawn(move || {
                            serve_connection(conn, cb.as_ref(), &log);
                            HANDLERS.fetch_sub(1, Ordering::SeqCst);
                        });
                    if spawned.is_err() {
                        HANDLERS.fetch_sub(1, Ordering::SeqCst);
                        drop(Owned(handle_from(cur)));
                    }
                }
                match next {
                    Some(h) => cur = h.0 as isize,
                    None => break,
                }
            }
            self.log.event("ipc_stopped", &[]);
        }
    }

    impl Transport for WinTransport {
        type Guard = LockGuard;

        fn try_lock(&self) -> Result<Option<LockGuard>, ()> {
            // SAFETY: `mutex_name` is NUL-terminated; the handle is owned and closed by `Owned`.
            unsafe {
                let h =
                    CreateMutexW(None, false, PCWSTR(self.mutex_name.as_ptr())).map_err(|_| ())?;
                let guard = Owned(h);
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    return Ok(None);
                }
                Ok(Some(LockGuard(guard)))
            }
        }

        fn send(&self, cmd: Command) -> Result<(), ()> {
            let msg = encode(cmd);
            debug_assert_eq!(msg.len(), MESSAGE_LEN);
            // About 3 s in total: covers a first instance that is still starting.
            for _ in 0..30 {
                // SAFETY: `pipe_name` is NUL-terminated; the client handle is closed by `Owned`.
                // SECURITY_IDENTIFICATION stops a fake server from impersonating this user.
                let opened = unsafe {
                    CreateFileW(
                        PCWSTR(self.pipe_name.as_ptr()),
                        GENERIC_WRITE,
                        FILE_SHARE_NONE,
                        None,
                        OPEN_EXISTING,
                        FILE_FLAGS_AND_ATTRIBUTES(
                            SECURITY_SQOS_PRESENT.0 | SECURITY_IDENTIFICATION.0,
                        ),
                        None,
                    )
                };
                match opened {
                    Ok(h) => {
                        let conn = Owned(h);
                        let mut written = 0u32;
                        // SAFETY: `msg` and `written` are live locals; `conn` is an open handle.
                        let r = unsafe { WriteFile(conn.0, Some(&msg), Some(&mut written), None) };
                        return if r.is_ok() && written as usize == msg.len() {
                            Ok(())
                        } else {
                            Err(())
                        };
                    }
                    Err(e) if e.code() == HRESULT::from_win32(ERROR_PIPE_BUSY.0) => {
                        // SAFETY: `pipe_name` is NUL-terminated.
                        unsafe {
                            let _ = WaitNamedPipeW(PCWSTR(self.pipe_name.as_ptr()), 100);
                        }
                    }
                    Err(e) if e.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0) => {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    Err(_) => return Err(()),
                }
            }
            Err(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    fn encode_is_version_then_command() {
        assert_eq!(encode(Command::Toggle), [1, 1]);
        assert_eq!(encode(Command::ShowSettings), [1, 2]);
        assert_eq!(encode(Command::Quit), [1, 3]);
    }

    #[test]
    fn decode_round_trips_every_command() {
        for c in [Command::Toggle, Command::ShowSettings, Command::Quit] {
            assert_eq!(decode(&encode(c)), Some(c));
        }
    }

    #[test]
    fn decode_drops_unknown_version_command_and_bad_sizes() {
        assert_eq!(decode(&[2, 1]), None, "unknown version");
        assert_eq!(decode(&[0, 1]), None, "unknown version");
        assert_eq!(decode(&[1, 0]), None, "unknown command");
        assert_eq!(decode(&[1, 4]), None, "unknown command");
        assert_eq!(decode(&[]), None, "empty");
        assert_eq!(decode(&[1]), None, "too short");
        assert_eq!(decode(&[1, 1, 0]), None, "oversize");
        assert_eq!(decode(&[1; 4096]), None, "oversize");
    }

    #[derive(Default)]
    struct Fake {
        held: Cell<bool>,
        lock_fails: bool,
        send_fails: bool,
        sent: RefCell<Vec<Command>>,
    }

    struct FakeGuard;

    impl Transport for Fake {
        type Guard = FakeGuard;
        fn try_lock(&self) -> Result<Option<FakeGuard>, ()> {
            if self.lock_fails {
                Err(())
            } else if self.held.get() {
                Ok(None)
            } else {
                Ok(Some(FakeGuard))
            }
        }
        fn send(&self, cmd: Command) -> Result<(), ()> {
            if self.send_fails {
                return Err(());
            }
            self.sent.borrow_mut().push(cmd);
            Ok(())
        }
    }

    #[test]
    fn free_lock_makes_us_primary_and_sends_nothing() {
        let t = Fake::default();
        let r = start_or_forward(&t, Command::Toggle, true);
        assert!(matches!(r, Start::Primary(_)));
        assert!(t.sent.borrow().is_empty());
    }

    #[test]
    fn held_lock_forwards_the_command() {
        let t = Fake {
            held: Cell::new(true),
            ..Fake::default()
        };
        assert!(matches!(
            start_or_forward(&t, Command::ShowSettings, true),
            Start::Forwarded
        ));
        assert_eq!(*t.sent.borrow(), vec![Command::ShowSettings]);
    }

    #[test]
    fn client_only_mode_reports_not_running() {
        let t = Fake::default();
        assert!(matches!(
            start_or_forward(&t, Command::Toggle, false),
            Start::NotRunning
        ));
        assert!(t.sent.borrow().is_empty());
    }

    #[test]
    fn failures_are_reported() {
        let t = Fake {
            lock_fails: true,
            ..Fake::default()
        };
        assert!(matches!(
            start_or_forward(&t, Command::Toggle, true),
            Start::Failed
        ));
        let t = Fake {
            held: Cell::new(true),
            send_fails: true,
            ..Fake::default()
        };
        assert!(matches!(
            start_or_forward(&t, Command::Quit, true),
            Start::Failed
        ));
    }

    #[test]
    fn name_hash_is_stable_and_separates_parts() {
        assert_eq!(name_hash(&["a"]), name_hash(&["a"]));
        assert_ne!(name_hash(&["a"]), name_hash(&["b"]));
        assert_ne!(name_hash(&["ab", "c"]), name_hash(&["a", "bc"]));
        assert_eq!(name_hash(&["x"]).len(), 16);
    }
}
