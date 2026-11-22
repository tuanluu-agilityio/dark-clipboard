//! Capture actor tests, split by topic. This file holds the shared harness (fake platform,
//! test clock, temporary storage) and helpers; each topic file starts with `use super::*;`.

mod commands;
mod copy_back;
mod debounce;
mod f14;
mod gates;
mod images;
mod pipeline;
mod retention;
mod sources;

use super::pending::{within_ms, within_ms_or_earlier};
use super::pipeline::{decide_all_sources, gate_without_formats};
use super::retention::next_backoff;
use super::*;
use crate::events::{UNAVAILABLE_RETENTION, UNAVAILABLE_STORAGE_WRITE};
use darkclip_core::markers::{is_mandatory, PrivacyMarker};
use darkclip_core::policy::SkipReason;
use darkclip_core::policy::{AvailableFormat, Decision, PreReadInput, SizeCaps};
use darkclip_core::sensitive::{masked_preview, scan};
use darkclip_core::Timestamp;
use darkclip_core::{
    canonical_hash, classify, ClipType, ClipboardSource, ExcludedApp, Lang, Mime, Platform,
    RawRepresentation, SensitiveCategory, SensitiveHandling, Sensitivity, SensitivityLevel,
    SourceConfidence,
};
use darkclip_platform::fake::{FakeCopy, FakeHandle, FakePlatform};
use darkclip_platform::PlatformFormat;
use darkclip_storage::{NewClip, OpenMode, SearchQuery};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Instant;

const T0: i64 = 1_795_320_000_000;
const WAIT: Duration = Duration::from_secs(5);

struct TestClock(AtomicI64);

impl TestClock {
    fn set(&self, ms: i64) {
        self.0.store(ms, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.0.load(Ordering::SeqCst))
    }
}

fn test_cfg() -> CaptureConfig {
    CaptureConfig {
        debounce_ms: 5,
        open_retries: 10,
        retry_delay_ms: 1,
        lang: Lang::En,
    }
}

fn caps() -> SizeCaps {
    SizeCaps {
        text_max_bytes: 1024 * 1024,
        image_max_bytes: 20 * 1024 * 1024,
    }
}

/// Random-looking key body, assembled at runtime (GC-09).
fn body(n: usize) -> String {
    let pool = b"aB3dE5gH7jK9mN1pQ2rS4tU6vW8xY0z";
    (0..n)
        .map(|i| char::from(pool[i % pool.len()]))
        .collect::<String>()
}

fn api_key_text() -> String {
    format!("key {}{}", concat!("s", "k-"), body(48))
}

fn pf(name: &str, mime: Option<Mime>, len: usize) -> PlatformFormat {
    PlatformFormat {
        name: name.to_string(),
        mime,
        size_hint: Some(len as u64),
    }
}

fn text_copy(s: &str) -> FakeCopy {
    FakeCopy {
        formats: vec![(
            pf("CF_UNICODETEXT", Some(Mime::TextPlain), s.len()),
            s.as_bytes().to_vec(),
        )],
        markers: Vec::new(),
        source: None,
    }
}

fn src(app_id: &str) -> ClipboardSource {
    ClipboardSource {
        app_id: app_id.to_string(),
        app_name: None,
        platform: Platform::Windows,
        confidence: SourceConfidence::Owner,
    }
}

struct H {
    dir: tempfile::TempDir,
    fake: FakeHandle,
    cap: Option<CaptureHandle>,
    events: Receiver<AppEvent>,
    clock: Arc<TestClock>,
}

fn start(policy: PrivacyPolicy) -> H {
    start_with(policy, RetentionPolicy::default(), test_cfg(), |_| {})
}

fn start_with(
    policy: PrivacyPolicy,
    retention: RetentionPolicy,
    cfg: CaptureConfig,
    seed: impl FnOnce(&mut Storage),
) -> H {
    start_full(policy, retention, cfg, MAX_BURST_WAIT, seed)
}

fn start_full(
    policy: PrivacyPolicy,
    retention: RetentionPolicy,
    cfg: CaptureConfig,
    max_burst_wait: Duration,
    seed: impl FnOnce(&mut Storage),
) -> H {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock(AtomicI64::new(T0)));
    let dyn_clock: Arc<dyn Clock> = clock.clone();
    let mut storage =
        Storage::open(dir.path(), OpenMode::ReadWrite, Arc::clone(&dyn_clock)).unwrap();
    seed(&mut storage);
    let (platform, fake) = FakePlatform::new();
    let (tx, events) = mpsc::channel();
    let cap = CaptureHandle::spawn_inner(
        platform.bundle(),
        storage,
        dyn_clock,
        cfg,
        policy,
        retention,
        tx,
        max_burst_wait,
    )
    .unwrap();
    H {
        dir,
        fake,
        cap: Some(cap),
        events,
        clock,
    }
}

impl H {
    fn cap(&self) -> &CaptureHandle {
        self.cap.as_ref().unwrap()
    }

    fn ro(&self) -> Storage {
        let clock: Arc<dyn Clock> = self.clock.clone();
        Storage::open(self.dir.path(), OpenMode::ReadOnly, clock).unwrap()
    }

    /// Previews of every stored clip (masked ones included).
    fn stored_previews(&self) -> Vec<String> {
        let (rows, _) = self
            .ro()
            .search(&SearchQuery {
                text: None,
                kinds: Vec::new(),
                pinned_only: false,
                since: None,
                include_masked: true,
                limit: 200,
                cursor: None,
            })
            .unwrap();
        rows.into_iter().map(|r| r.preview).collect()
    }

    fn clip_count(&self) -> u64 {
        self.ro().stats().unwrap().clips
    }

    /// Waits for the first event matching `pred`; returns it and the events seen before it.
    fn wait_for(&self, what: &str, pred: impl Fn(&AppEvent) -> bool) -> (AppEvent, Vec<AppEvent>) {
        let deadline = Instant::now() + WAIT;
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(e) if pred(&e) => return (e, seen),
                Ok(e) => seen.push(e),
                Err(_) => panic!("timed out waiting for {what}; saw {seen:?}"),
            }
        }
    }

    fn wait_added(&self) -> (ClipId, Vec<AppEvent>) {
        match self.wait_for("ClipAdded", |e| matches!(e, AppEvent::ClipAdded(_))) {
            (AppEvent::ClipAdded(id), seen) => (id, seen),
            _ => unreachable!(),
        }
    }

    fn wait_skip(&self, reason: SkipReason) -> Vec<AppEvent> {
        self.wait_for("Skipped", |e| *e == AppEvent::Skipped(reason))
            .1
    }

    fn set_mode(&self, mode: CaptureMode) {
        self.cap().send(ActorCommand::SetMode(mode)).unwrap();
        self.wait_for("ModeChanged", |e| *e == AppEvent::ModeChanged(mode));
    }

    fn copy_back(&self, id: ClipId, plain: bool) -> Result<(), ApiError> {
        let (tx, rx) = mpsc::channel();
        self.cap()
            .send(ActorCommand::CopyBack {
                id,
                plain,
                deadline: Instant::now() + WAIT,
                reply: tx,
            })
            .unwrap();
        rx.recv_timeout(WAIT).unwrap()
    }

    /// A new copy and its `ClipAdded`: proves every earlier change was fully processed.
    fn sentinel(&self) -> Vec<AppEvent> {
        // Owned on purpose: an ownerless sentinel right after an ExcludedApp skip can fall in
        // D-042 rule 3's 500 ms window (signal time vs actor time) and be skipped, which made
        // `excluded_app_ole_flush_without_markers_is_skipped` flaky on CI.
        self.fake.copy(FakeCopy {
            source: Some(src("sentinel.exe")),
            ..text_copy("sentinel clip")
        });
        self.wait_added().1
    }
}

impl Drop for H {
    fn drop(&mut self) {
        if let Some(cap) = self.cap.take() {
            let _ = cap.shutdown(Duration::from_secs(2));
        }
    }
}

fn no_store_events(events: &[AppEvent]) -> bool {
    !events
        .iter()
        .any(|e| matches!(e, AppEvent::ClipAdded(_) | AppEvent::ClipBumped(_)))
}

pub(super) fn slow_cfg() -> CaptureConfig {
    // Wide enough that two back-to-back copies land in one debounce window.
    CaptureConfig {
        debounce_ms: 100,
        ..test_cfg()
    }
}

pub(super) fn excluded_copy(s: &str) -> FakeCopy {
    let mut c = text_copy(s);
    // In the default exclusion list; case differs on purpose (Windows match is insensitive).
    c.source = Some(src("KeePassXC.exe"));
    c
}

/// Runs a scenario whose steps must land in one debounce window. Under heavy load the
/// window can close between two steps (detected by the attempt, which returns false);
/// the attempt is then repeated with a fresh actor. Every attempt that lands in one
/// window runs all its assertions.
pub(super) fn in_one_window(mut attempt: impl FnMut() -> bool) {
    for _ in 0..5 {
        if attempt() {
            return;
        }
    }
    panic!("the steps never landed in one debounce window");
}

pub(super) fn masked_policy() -> PrivacyPolicy {
    PrivacyPolicy {
        sensitive_handling: SensitiveHandling::StoreMasked,
        ..PrivacyPolicy::default()
    }
}
