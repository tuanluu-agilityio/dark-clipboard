//! The translate-in-place host (Task 605): the real [`FlowSys`] and the press wiring.
//!
//! A press of the translate shortcut calls [`on_press`] (on the main thread). Single flight: a
//! press while a flow runs is ignored (Review Focus 3). The flow runs on the named thread
//! `darkclip-translate`; while it runs, Esc is a temporary global hotkey that sets the cancel
//! flag, unregistered when the flow ends.
//!
//! Every app call goes to the thread that owns `App` (the UI worker) and waits with a bound;
//! the clipboard is only ever touched by the capture actor through the app API (GC-03). The API
//! key is read from the OS credential store once per press, never cached, and handed to the
//! provider inside `Redacted`; the network is reached only through `ai_net::UreqTransport`
//! (GC-08, D-094). The log gets one content-free line per press: codes, counts and timings,
//! never text, never the app id (GC-06).
//!
//! Windows only: other platforms never register the shortcut and report the feature as
//! unavailable.

// The real system is Windows only; the pure parts compile and are tested everywhere (F-19).
#![cfg_attr(not(windows), allow(dead_code))]

use super::flow::{Outcome, OverlayState};
use crate::log::{Log, Val};
use darkclip_app::App;
use darkclip_core::SensitiveCategory;
use darkclip_platform::PasteInjector;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// The platform's paste injector, shared with the popup (`popup_host::Injector`): the
/// translate copy and paste are serialized with the popup's paste by the same lock.
pub(crate) type Injector = Arc<Mutex<Box<dyn PasteInjector>>>;

/// A job for the thread that owns `App`.
pub(crate) type AppJob = Box<dyn FnOnce(&App) + Send>;
/// Queues an [`AppJob`] on that thread (the UI worker's `Poster::post_quiet`).
pub(crate) type RunOnApp = Arc<dyn Fn(AppJob) + Send + Sync>;
/// Registers (`true`) or unregisters Esc as the cancel hotkey; hops to the main thread itself.
pub(crate) type CancelKey = Arc<dyn Fn(bool) + Send + Sync>;

/// True where translate in place can run (Windows; elsewhere it reports unavailable).
pub(crate) fn available() -> bool {
    cfg!(windows)
}

/// Where the overlay states go. Task 606 replaces [`LogOverlay`] with the window.
pub(crate) trait OverlaySink: Send + Sync {
    fn show(&self, s: &OverlayState);
}

/// Static name of an overlay state, for the log.
pub(crate) fn overlay_name(s: &OverlayState) -> &'static str {
    match s {
        OverlayState::Sending { .. } => "sending",
        OverlayState::Done => "done",
        OverlayState::Cancelled => "cancelled",
        OverlayState::LeftOnClipboard => "left_on_clipboard",
        OverlayState::Blocked(_) => "blocked",
        OverlayState::Failed(_) => "failed",
    }
}

/// The overlay until Task 606: one content-free log line per state.
pub(crate) struct LogOverlay(pub Arc<Log>);

impl OverlaySink for LogOverlay {
    fn show(&self, s: &OverlayState) {
        let mut fields = vec![("state", Val::Name(overlay_name(s)))];
        match s {
            OverlayState::Sending { chars } => {
                fields.push(("chars", Val::Num(u64::try_from(*chars).unwrap_or(u64::MAX))));
            }
            OverlayState::Blocked(b) => fields.push(("reason", Val::Name(b.name()))),
            OverlayState::Failed(e) => fields.push(("reason", Val::Name(e.name()))),
            OverlayState::Done | OverlayState::Cancelled | OverlayState::LeftOnClipboard => {}
        }
        self.0.event("translate_overlay", &fields);
    }
}

/// One flow at a time: a press while one runs is ignored (Review Focus 3).
#[derive(Default)]
pub(crate) struct SingleFlight {
    running: Arc<AtomicBool>,
}

/// Clears the flag when the job ends, however it ends.
struct Landing(Arc<AtomicBool>);

impl Drop for Landing {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl SingleFlight {
    /// Starts `job` on a new thread named `name` unless one is running. `false`: ignored.
    pub(crate) fn try_start(&self, name: &str, job: impl FnOnce() + Send + 'static) -> bool {
        if self.running.swap(true, Ordering::SeqCst) {
            return false;
        }
        let landing = Landing(Arc::clone(&self.running));
        let started = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let _landing = landing;
                job();
            });
        if started.is_err() {
            self.running.store(false, Ordering::SeqCst);
            return false;
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

/// Every sensitive category, whatever the user's sensitive setting (the stricter rule for
/// text that would leave the device).
pub(crate) const ALL_CATEGORIES: [SensitiveCategory; 7] = [
    SensitiveCategory::ApiKey,
    SensitiveCategory::Token,
    SensitiveCategory::PrivateKey,
    SensitiveCategory::Password,
    SensitiveCategory::PaymentCard,
    SensitiveCategory::Email,
    SensitiveCategory::Phone,
];

/// Compile-time check that [`ALL_CATEGORIES`] lists every category: a new variant breaks this
/// match until it is added above.
const fn _every_category_listed(c: SensitiveCategory) {
    match c {
        SensitiveCategory::ApiKey
        | SensitiveCategory::Token
        | SensitiveCategory::PrivateKey
        | SensitiveCategory::Password
        | SensitiveCategory::PaymentCard
        | SensitiveCategory::Email
        | SensitiveCategory::Phone => {}
    }
}

/// True when the sensitive scan finds anything in any category.
pub(crate) fn sensitive_all(text: &str) -> bool {
    !darkclip_core::sensitive::scan(text, &ALL_CATEGORIES)
        .categories
        .is_empty()
}

/// Windows virtual-key code of a shortcut key code name (`KeyT`, `Digit1`, `F5`, `Space`...),
/// as produced by `shortcut::parse`.
pub(crate) fn vk_for_code(code: &str) -> Option<u16> {
    if let Some(c) = code.strip_prefix("Key") {
        let b = c.as_bytes();
        return (b.len() == 1 && b[0].is_ascii_uppercase()).then(|| u16::from(b[0]));
    }
    if let Some(d) = code.strip_prefix("Digit") {
        let b = d.as_bytes();
        return (b.len() == 1 && b[0].is_ascii_digit()).then(|| u16::from(b[0]));
    }
    if let Some(n) = code.strip_prefix('F') {
        if let Ok(n) = n.parse::<u16>() {
            return (1..=24).contains(&n).then(|| 0x70 + n - 1);
        }
    }
    Some(match code {
        "Space" => 0x20,
        "Enter" => 0x0D,
        "Tab" => 0x09,
        "Backspace" => 0x08,
        "Delete" => 0x2E,
        "Insert" => 0x2D,
        "Home" => 0x24,
        "End" => 0x23,
        "PageUp" => 0x21,
        "PageDown" => 0x22,
        "ArrowLeft" => 0x25,
        "ArrowUp" => 0x26,
        "ArrowRight" => 0x27,
        "ArrowDown" => 0x28,
        _ => return None,
    })
}

/// Ctrl, Shift, Alt (`VK_MENU`), left and right Win: they must all be up before arming.
pub(crate) const MODIFIER_VKS: [u16; 5] = [0x11, 0x10, 0x12, 0x5B, 0x5C];

/// What one press measured, for the log line.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Metrics {
    pub src_chars: u64,
    pub out_chars: u64,
    pub api_ms: u64,
    pub total_ms: u64,
}

/// The fields of the one log line per press: codes, counts and timings only.
pub(crate) fn log_fields(o: Outcome, m: &Metrics) -> Vec<(&'static str, Val)> {
    let mut f = vec![("outcome", Val::Name(o.name()))];
    if let Some(r) = o.reason() {
        f.push(("reason", Val::Name(r)));
    }
    f.extend([
        ("src_chars", Val::Num(m.src_chars)),
        ("out_chars", Val::Num(m.out_chars)),
        ("api_ms", Val::Num(m.api_ms)),
        ("total_ms", Val::Num(m.total_ms)),
    ]);
    f
}

/// What the host needs from the UI side.
pub(crate) struct HostDeps {
    pub run_on_app: RunOnApp,
    pub log: Arc<Log>,
    pub overlay: Arc<dyn OverlaySink>,
    pub cancel_key: CancelKey,
}

struct Host {
    deps: HostDeps,
    flight: SingleFlight,
    cancel: Arc<AtomicBool>,
}

static HOST: OnceLock<Host> = OnceLock::new();
static INJECTOR: Mutex<Option<Injector>> = Mutex::new(None);
/// Whether a key is stored, as last seen (`None`: not read yet). Only a bool: the key itself
/// is read per press. Task 606 calls [`key_changed`] when the key is set or removed.
static KEY_PRESENT: Mutex<Option<bool>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Installs the host once (the UI's `run`). A second call is ignored.
pub(crate) fn install(deps: HostDeps) {
    let _ = HOST.set(Host {
        deps,
        flight: SingleFlight::default(),
        cancel: Arc::new(AtomicBool::new(false)),
    });
}

/// The platform's paste injector (only where auto-paste is native).
pub(crate) fn set_injector(injector: Injector) {
    *lock(&INJECTOR) = Some(injector);
}

/// Esc while a flow runs.
pub(crate) fn on_cancel() {
    if let Some(h) = HOST.get() {
        h.cancel.store(true, Ordering::SeqCst);
    }
}

/// Shutdown: a running flow stops at its next wait.
pub(crate) fn shutdown() {
    on_cancel();
}

/// The stored key changed (Task 606): the next status refresh reads its presence again.
#[allow(dead_code)] // called by Task 606's key entry
pub(crate) fn key_changed() {
    *lock(&KEY_PRESENT) = None;
}

/// Whether an API key is stored (cached as a bool; read from the store when unknown). Called
/// by the status refresh only while the feature is consented, so a user who never turned it
/// on never has the credential store read.
pub(crate) fn key_present() -> bool {
    if let Some(p) = *lock(&KEY_PRESENT) {
        return p;
    }
    let p = imp::read_key().is_some_and(|mut k| {
        let some = !k.0.is_empty();
        darkclip_core::sensitive::wipe_string(&mut k.0);
        some
    });
    *lock(&KEY_PRESENT) = Some(p);
    p
}

fn note_key_present(p: bool) {
    *lock(&KEY_PRESENT) = Some(p);
}

/// A press of the translate shortcut (main thread). Ignored while a flow runs.
pub(crate) fn on_press() {
    let Some(h) = HOST.get() else {
        return;
    };
    let started = h
        .flight
        .try_start("darkclip-translate", move || run_press(h));
    if !started {
        h.deps
            .log
            .event("translate", &[("outcome", Val::Name("ignored_busy"))]);
    }
}

/// Unregisters Esc when dropped: the flow's end, whatever path it takes.
struct EscGuard(CancelKey);

impl EscGuard {
    fn arm(key: CancelKey) -> Self {
        key(true);
        Self(key)
    }
}

impl Drop for EscGuard {
    fn drop(&mut self) {
        (self.0)(false);
    }
}

fn run_press(h: &'static Host) {
    let t0 = std::time::Instant::now();
    h.cancel.store(false, Ordering::SeqCst);
    let esc = EscGuard::arm(Arc::clone(&h.deps.cancel_key));
    let injector = lock(&INJECTOR).clone();
    let (outcome, mut m) = imp::run_flow(h, injector);
    drop(esc);
    m.total_ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
    h.deps.log.event("translate", &log_fields(outcome, &m));
}

/// A text that is wiped when dropped unused (a job the worker never ran).
struct Secret(String);

impl Secret {
    fn take(&mut self) -> String {
        std::mem::take(&mut self.0)
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        darkclip_core::sensitive::wipe_string(&mut self.0);
    }
}

#[cfg(windows)]
mod imp {
    use super::{
        any_modifier_or, lock, note_key_present, sensitive_all, AppJob, Host, Injector, Metrics,
        Secret,
    };
    use crate::translate::flow::{
        self, first_block, Block, BlockFacts, Cleared, FlowConfig, FlowSys, Outcome, OverlayState,
    };
    use darkclip_ai::{
        AiError, AnthropicProvider, Provider, TransformKind, TransformRequest, TransformResult,
    };
    use darkclip_app::api::Caller;
    use darkclip_app::capture::{ClaimError, ClaimedText};
    use darkclip_app::settings::AiSettings;
    use darkclip_app::App;
    use darkclip_core::sensitive::wipe_string;
    use darkclip_core::{CaptureMode, ClipId, Redacted};
    use darkclip_platform::{PasteTarget, PlatformError, SecretStore, ANTHROPIC_KEY_TARGET};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::{self, Receiver};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// The longest wait for an app call on the worker (it may be busy with another job).
    const APP_CALL_WAIT: Duration = Duration::from_secs(5);

    pub(super) fn read_key() -> Option<Redacted<String>> {
        darkclip_windows::WinSecretStore
            .get(ANTHROPIC_KEY_TARGET)
            .ok()
            .flatten()
    }

    struct WinFlow<'a> {
        host: &'a Host,
        injector: Option<Injector>,
        ai: AiSettings,
        /// Read once per press in `blocked`; wiped when the press ends.
        key: Option<Redacted<String>>,
        metrics: Metrics,
        api_ms: Arc<AtomicU64>,
    }

    impl Drop for WinFlow<'_> {
        fn drop(&mut self) {
            if let Some(k) = self.key.as_mut() {
                wipe_string(&mut k.0);
            }
        }
    }

    impl WinFlow<'_> {
        /// Runs `f` on the thread that owns `App` and waits for its answer (bounded).
        fn call<T: Send + 'static>(&self, f: impl FnOnce(&App) -> T + Send + 'static) -> Option<T> {
            let (tx, rx) = mpsc::channel();
            let job: AppJob = Box::new(move |app| {
                let _ = tx.send(f(app));
            });
            (self.host.deps.run_on_app)(job);
            rx.recv_timeout(APP_CALL_WAIT).ok()
        }

        fn shortcut_vk(&self) -> Option<u16> {
            let s = crate::shortcut::parse(&self.ai.shortcut).ok()?;
            super::vk_for_code(&s.code)
        }
    }

    impl FlowSys for WinFlow<'_> {
        fn target(&mut self) -> Option<PasteTarget> {
            let (window, pid) = crate::winsys::foreground_window_of_other_process()?;
            Some(PasteTarget {
                app_id: darkclip_windows::process_app_id(pid),
                window: Some(window),
                pid: Some(pid),
            })
        }

        fn blocked(&mut self, t: &PasteTarget) -> Option<Block> {
            let app_id = t.app_id.clone();
            let Some((ai, private, excluded)) = self.call(move |app| {
                let s = app.settings();
                let excluded = app_id.as_deref().is_some_and(|a| app.is_excluded_app(a));
                (s.ai, s.privacy.mode == CaptureMode::Private, excluded)
            }) else {
                // The app does not answer: nothing is sent.
                return Some(Block::Off);
            };
            self.ai = ai;
            let consented = self.ai.consented();
            if consented {
                self.key = read_key().filter(|k| !k.0.is_empty());
                note_key_present(self.key.is_some());
            }
            first_block(&BlockFacts {
                consented,
                has_key: self.key.is_some(),
                private,
                terminal: t.window.is_some_and(crate::winsys::is_console_window),
                app_known: t.app_id.as_deref().is_some_and(|a| !a.is_empty()),
                excluded,
            })
        }

        fn clipboard_seq(&mut self) -> Option<u64> {
            // Unknown: the flow then skips the select-all retry.
            self.call(|app| app.ui_clipboard_seq(&Caller::Ui).ok())
                .flatten()
        }

        fn keys_up(&mut self, wait: u64) -> bool {
            let key = self.shortcut_vk();
            let deadline = Instant::now() + Duration::from_millis(wait);
            loop {
                if !any_modifier_or(key) {
                    return true;
                }
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(flow::POLL_MS));
            }
        }

        fn claim(&mut self, app_id: &str) -> Receiver<Result<ClaimedText, ClaimError>> {
            let app_id = app_id.to_string();
            match self.call(move |app| app.ui_translate_claim(&Caller::Ui, app_id)) {
                Some(Ok(rx)) => rx,
                // Not armed: answer at once, so the flow sends no key.
                _ => {
                    let (tx, rx) = mpsc::channel();
                    let _ = tx.send(Err(ClaimError::Busy));
                    rx
                }
            }
        }

        fn send_copy(&mut self, t: &PasteTarget, select_all: bool) -> Result<(), PlatformError> {
            let injector = self
                .injector
                .as_ref()
                .ok_or(PlatformError::Unsupported("copy"))?;
            let mut guard = lock(injector);
            guard.copy(t, select_all)
        }

        fn is_sensitive(&mut self, text: &str) -> bool {
            sensitive_all(text)
        }

        fn translate(&mut self, text: Cleared) -> Receiver<Result<TransformResult, AiError>> {
            let (tx, rx) = mpsc::channel();
            let Some(key) = self.key.clone() else {
                drop(text);
                let _ = tx.send(Err(AiError::NoKey));
                return rx;
            };
            let model = self.ai.model_or_default().to_string();
            let direction = self.ai.direction;
            let api_ms = Arc::clone(&self.api_ms);
            let spawned = std::thread::Builder::new()
                .name("darkclip-translate-net".into())
                .spawn(move || {
                    let t0 = Instant::now();
                    let provider =
                        AnthropicProvider::new(crate::ai_net::UreqTransport::new(), key, &model);
                    let mut req = TransformRequest {
                        kind: TransformKind::Translate { direction },
                        text: text.into_inner(),
                    };
                    let answer = provider.transform(&req);
                    wipe_string(&mut req.text.0);
                    drop(provider);
                    api_ms.store(
                        u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX),
                        Ordering::SeqCst,
                    );
                    // The flow gave up (Esc or the guard): the late answer is wiped.
                    if let Err(mpsc::SendError(Ok(mut late))) = tx.send(answer) {
                        wipe_string(&mut late.text.0);
                    }
                });
            // A failed spawn drops the closure (and so wipes the text); the closed channel
            // ends the flow's wait as a network failure.
            drop(spawned);
            rx
        }

        fn write_own_if(
            &mut self,
            expect_seq: u64,
            text: Redacted<String>,
        ) -> Result<Option<u64>, ()> {
            self.metrics.out_chars = u64::try_from(text.0.chars().count()).unwrap_or(u64::MAX);
            let mut secret = Secret(text.0);
            match self.call(move |app| {
                app.ui_copy_text_if(&Caller::Ui, expect_seq, Redacted(secret.take()))
            }) {
                Some(Ok(own)) => Ok(own),
                _ => Err(()),
            }
        }

        fn paste(&mut self, t: &PasteTarget) -> Result<(), PlatformError> {
            let injector = self
                .injector
                .as_ref()
                .ok_or(PlatformError::Unsupported("paste"))?;
            let mut guard = lock(injector);
            guard.paste(t)
        }

        fn restore_candidate(&mut self) -> Option<ClipId> {
            self.call(|app| app.ui_restore_candidate(&Caller::Ui).ok().flatten())
                .flatten()
        }

        fn restore_if(&mut self, expect_seq: u64, id: ClipId) -> Result<bool, ()> {
            match self.call(move |app| app.ui_restore_if(&Caller::Ui, expect_seq, id)) {
                Some(Ok(done)) => Ok(done),
                _ => Err(()),
            }
        }

        fn overlay(&mut self, s: OverlayState) {
            if let OverlayState::Sending { chars } = s {
                self.metrics.src_chars = u64::try_from(chars).unwrap_or(u64::MAX);
            }
            self.host.deps.overlay.show(&s);
        }

        fn sleep(&mut self, ms: u64) {
            std::thread::sleep(Duration::from_millis(ms));
        }

        fn cancelled(&mut self) -> bool {
            self.host.cancel.load(Ordering::SeqCst)
        }
    }

    pub(super) fn run_flow(h: &Host, injector: Option<Injector>) -> (Outcome, Metrics) {
        let mut sys = WinFlow {
            host: h,
            injector,
            ai: AiSettings::default(),
            key: None,
            metrics: Metrics::default(),
            api_ms: Arc::new(AtomicU64::new(0)),
        };
        let outcome = flow::run(&mut sys, &FlowConfig::default());
        let mut m = sys.metrics;
        m.api_ms = sys.api_ms.load(Ordering::SeqCst);
        (outcome, m)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Host, Injector, Metrics};
    use crate::translate::flow::{Block, Outcome};
    use darkclip_core::Redacted;

    pub(super) fn read_key() -> Option<Redacted<String>> {
        None
    }

    pub(super) fn run_flow(_h: &Host, _injector: Option<Injector>) -> (Outcome, Metrics) {
        (Outcome::Blocked(Block::Off), Metrics::default())
    }
}

/// True while any modifier, or the shortcut's own key, is down.
#[cfg(windows)]
fn any_modifier_or(key: Option<u16>) -> bool {
    let mut vks = MODIFIER_VKS.to_vec();
    vks.extend(key);
    crate::winsys::any_key_down(&vks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::flow::{Block, FlowError};
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn second_press_while_running_is_ignored() {
        let flight = SingleFlight::default();
        let runs = Arc::new(AtomicUsize::new(0));
        let (go, wait) = mpsc::channel::<()>();
        let r = Arc::clone(&runs);
        assert!(flight.try_start("t-flow", move || {
            r.fetch_add(1, Ordering::SeqCst);
            let _ = wait.recv();
        }));
        assert!(flight.running());
        // Pressed again (or held) while the first flow runs: ignored, nothing started.
        for _ in 0..5 {
            let r = Arc::clone(&runs);
            assert!(!flight.try_start("t-flow", move || {
                r.fetch_add(1, Ordering::SeqCst);
            }));
        }
        go.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while flight.running() {
            assert!(Instant::now() < deadline, "the flow never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        // After it ended, the next press runs.
        let (done_tx, done_rx) = mpsc::channel();
        let r = Arc::clone(&runs);
        assert!(flight.try_start("t-flow", move || {
            r.fetch_add(1, Ordering::SeqCst);
            let _ = done_tx.send(());
        }));
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn sensitive_uses_all_categories() {
        // Runtime-assembled samples (GC-09), one per category a user may switch off.
        let email = format!("{}@{}.com", "someone", "example");
        let card = ["4111", "1111", "1111", "1111"].join(" ");
        let key = format!(
            "{}{}",
            concat!("s", "k-"),
            "aB3dE5gH7jK9mN1pQ2rS4tU6vW8xY0zAB3dE5"
        );
        for s in [email.as_str(), card.as_str(), key.as_str()] {
            let text = format!("please translate: {s}");
            assert!(sensitive_all(&text));
        }
        assert!(!sensitive_all("Xin chào, hôm nay trời đẹp quá."));
        // The list is every category.
        for c in ALL_CATEGORIES {
            _every_category_listed(c);
        }
        assert_eq!(ALL_CATEGORIES.len(), 7);
    }

    #[test]
    fn shortcut_keys_map_to_virtual_keys() {
        assert_eq!(vk_for_code("KeyT"), Some(0x54));
        assert_eq!(vk_for_code("KeyA"), Some(0x41));
        assert_eq!(vk_for_code("Digit0"), Some(0x30));
        assert_eq!(vk_for_code("F1"), Some(0x70));
        assert_eq!(vk_for_code("F24"), Some(0x87));
        assert_eq!(vk_for_code("Space"), Some(0x20));
        assert_eq!(vk_for_code("ArrowDown"), Some(0x28));
        assert_eq!(vk_for_code("F25"), None);
        assert_eq!(vk_for_code("Keyt"), None);
        assert_eq!(vk_for_code("Banana"), None);
        // Every key the parser accepts has a virtual key.
        for s in [
            "Ctrl+Alt+T",
            "Ctrl+9",
            "Ctrl+F12",
            "Ctrl+Enter",
            "Ctrl+Tab",
            "Ctrl+Backspace",
            "Ctrl+Del",
            "Ctrl+Ins",
            "Ctrl+Home",
            "Ctrl+End",
            "Ctrl+PgUp",
            "Ctrl+PgDn",
            "Ctrl+Up",
            "Ctrl+Left",
            "Ctrl+Right",
            "Ctrl+Space",
        ] {
            let code = crate::shortcut::parse(s).unwrap().code;
            assert!(vk_for_code(&code).is_some(), "{s}");
        }
    }

    fn render(fields: &[(&'static str, Val)]) -> String {
        fields
            .iter()
            .map(|(k, v)| match v {
                Val::Num(n) => format!("{k}={n}"),
                Val::Name(s) => format!("{k}={s}"),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn debug_and_log_lines_are_content_free() {
        let source = "Xin chào thế giới";
        let m = Metrics {
            src_chars: 17,
            out_chars: 11,
            api_ms: 900,
            total_ms: 1500,
        };
        let line = render(&log_fields(Outcome::Replaced, &m));
        assert_eq!(
            line,
            "outcome=replaced src_chars=17 out_chars=11 api_ms=900 total_ms=1500"
        );
        let line = render(&log_fields(
            Outcome::Blocked(Block::Terminal),
            &Metrics::default(),
        ));
        assert!(line.starts_with("outcome=blocked reason=terminal "));
        for o in [
            Outcome::Replaced,
            Outcome::LeftOnClipboard,
            Outcome::Cancelled,
            Outcome::Blocked(Block::ExcludedApp),
            Outcome::Failed(FlowError::Sensitive),
            Outcome::Failed(FlowError::Ai(darkclip_ai::AiError::Auth)),
        ] {
            let text = format!("{o:?} {}", render(&log_fields(o, &m)));
            assert!(!text.contains("chào") && !text.contains(source), "{text}");
            assert!(!text.contains(".exe"), "{text}");
        }
        // The overlay log line carries the state, a count or a code, nothing else.
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(Log::open(dir.path()));
        let sink = LogOverlay(Arc::clone(&log));
        for s in [
            OverlayState::Sending { chars: 17 },
            OverlayState::Done,
            OverlayState::Failed(FlowError::TooLong),
            OverlayState::Blocked(Block::NoKey),
        ] {
            sink.show(&s);
            assert!(!format!("{s:?}").contains("chào"));
        }
        drop(sink);
        drop(log);
        let text =
            std::fs::read_to_string(dir.path().join("logs").join("dark-clipboard.log")).unwrap();
        assert!(text.contains("translate_overlay state=sending chars=17"));
        assert!(text.contains("translate_overlay state=blocked reason=no_key"));
        assert!(!text.contains("chào"));
    }

    #[test]
    fn inactive_without_consent_or_key() {
        use crate::translate::flow::{first_block, BlockFacts};
        use darkclip_app::settings::AiSettings;
        let facts = |ai: &AiSettings, has_key: bool| BlockFacts {
            consented: ai.consented(),
            has_key,
            private: false,
            terminal: false,
            app_known: true,
            excluded: false,
        };
        let mut ai = AiSettings::default();
        assert_eq!(first_block(&facts(&ai, true)), Some(Block::Off));
        ai.enabled = true;
        // Enabled without the consent dialog: still off.
        assert_eq!(first_block(&facts(&ai, true)), Some(Block::Off));
        ai.consent_version = 1;
        assert_eq!(first_block(&facts(&ai, false)), Some(Block::NoKey));
        assert_eq!(first_block(&facts(&ai, true)), None);
        // Not available off Windows at all.
        assert_eq!(available(), cfg!(windows));
    }

    #[test]
    fn esc_is_unregistered_when_the_flow_ends_on_any_path() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let c = Arc::clone(&calls);
        let key: CancelKey = Arc::new(move |on| c.lock().unwrap().push(on));
        {
            let _esc = EscGuard::arm(Arc::clone(&key));
            assert_eq!(*calls.lock().unwrap(), vec![true]);
        }
        assert_eq!(*calls.lock().unwrap(), vec![true, false]);
        // Also when the flow's thread unwinds.
        let k = Arc::clone(&key);
        let _ = std::thread::spawn(move || {
            let _esc = EscGuard::arm(k);
            panic!("flow failed");
        })
        .join();
        assert_eq!(*calls.lock().unwrap(), vec![true, false, true, false]);
    }

    #[test]
    fn a_secret_job_never_run_is_wiped_and_a_taken_one_moves_out() {
        let mut s = Secret("abc".into());
        assert_eq!(s.take(), "abc");
        assert!(s.0.is_empty());
        drop(Secret("xyz".into()));
    }
}
