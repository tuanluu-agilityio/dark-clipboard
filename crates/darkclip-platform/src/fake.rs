//! In-memory platform for tests (also used by `darkclip-app` via feature `fake`).
//!
//! The adapter contract is enforced here exactly as real adapters must: `read` touches only
//! formats in the plan and applies the plan's byte caps; `snapshot_meta` never counts as a
//! read. Test data only; nothing here logs content (GC-06).

use crate::caps::{Capabilities, ShortcutSupport, SourceSupport, Support};
use crate::error::PlatformError;
use crate::traits::{
    ChangeSignal, ClipboardAccess, ClipboardMeta, ClipboardMonitor, PlatformBundle, PlatformFormat,
    SourceResolver,
};
use darkclip_core::markers::PrivacyMarker;
use darkclip_core::policy::ReadPlan;
use darkclip_core::{ClipboardSource, Mime, RawRepresentation, Timestamp};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// What another app puts on the fake clipboard.
pub struct FakeCopy {
    pub formats: Vec<(PlatformFormat, Vec<u8>)>,
    pub markers: Vec<PrivacyMarker>,
    pub source: Option<ClipboardSource>,
}

#[derive(Default)]
struct State {
    seq: u64,
    formats: Vec<(PlatformFormat, Vec<u8>)>,
    markers: Vec<PrivacyMarker>,
    source: Option<ClipboardSource>,
    busy: u32,
    read_calls: u32,
    snapshot_meta_calls: u32,
    tx: Option<Sender<ChangeSignal>>,
}

impl State {
    fn take_busy(&mut self) -> Result<(), PlatformError> {
        if self.busy > 0 {
            self.busy -= 1;
            return Err(PlatformError::Busy);
        }
        Ok(())
    }
}

type Shared = Arc<Mutex<State>>;

fn lock(s: &Shared) -> MutexGuard<'_, State> {
    s.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now() -> Timestamp {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    Timestamp(ms)
}

fn emit(st: &State) {
    if let Some(tx) = &st.tx {
        // A closed receiver is a normal shutdown.
        let _ = tx.send(ChangeSignal {
            seq: st.seq,
            at: now(),
        });
    }
}

fn len_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// Cuts `bytes` to at most `cap` bytes without splitting a UTF-8 sequence.
fn truncate_utf8(bytes: &[u8], cap: u64) -> Vec<u8> {
    let mut n = usize::try_from(cap).unwrap_or(usize::MAX).min(bytes.len());
    while n > 0 && n < bytes.len() && bytes.get(n).is_some_and(|b| b & 0xC0 == 0x80) {
        n -= 1;
    }
    bytes.get(..n).unwrap_or_default().to_vec()
}

pub struct FakePlatform {
    state: Shared,
}

impl FakePlatform {
    pub fn new() -> (Self, FakeHandle) {
        let state: Shared = Arc::new(Mutex::new(State::default()));
        (
            Self {
                state: Arc::clone(&state),
            },
            FakeHandle { state },
        )
    }

    /// All trait objects share one state. No paste injector (`auto_paste` is unsupported).
    pub fn bundle(&self) -> PlatformBundle {
        PlatformBundle {
            monitor: Box::new(FakeMonitor {
                state: Arc::clone(&self.state),
            }),
            access: Box::new(FakeAccess {
                state: Arc::clone(&self.state),
            }),
            source: Box::new(FakeSource {
                state: Arc::clone(&self.state),
            }),
            paste: None,
            caps: Capabilities {
                monitor: Support::Native,
                read_without_focus: Support::Native,
                write: Support::Native,
                concealed_markers: Support::Native,
                source_app: SourceSupport::Owner,
                global_shortcut: ShortcutSupport::Native,
                auto_paste: Support::Unsupported {
                    reason: "fake platform",
                },
                tray: Support::Native,
                autostart: Support::Native,
                backend: "fake",
            },
        }
    }
}

/// Test driver for the fake clipboard.
#[derive(Clone)]
pub struct FakeHandle {
    state: Shared,
}

impl FakeHandle {
    /// Simulates another app copying. Bumps seq, sets formats/markers/source, emits one signal.
    pub fn copy(&self, c: FakeCopy) -> u64 {
        let mut st = lock(&self.state);
        st.seq += 1;
        st.formats = c.formats;
        st.markers = c.markers;
        st.source = c.source;
        emit(&st);
        st.seq
    }

    /// Emits n extra signals without changing seq (Windows burst/OLE behaviour).
    pub fn signal_again(&self, n: usize) {
        let st = lock(&self.state);
        for _ in 0..n {
            emit(&st);
        }
    }

    /// Changes formats without a new seq (OLE delayed rendering phase 2).
    pub fn render(&self, formats: Vec<(PlatformFormat, Vec<u8>)>) {
        lock(&self.state).formats = formats;
    }

    /// The next `times` calls of `snapshot_meta`, `read` or `write` return `Busy`.
    /// `sequence` never fails, so a monitor polling it does not consume the budget.
    pub fn set_busy(&self, times: u32) {
        lock(&self.state).busy = times;
    }

    /// OLE flush: owner gone.
    pub fn clear_source(&self) {
        lock(&self.state).source = None;
    }

    /// What is on the fake clipboard (for copy-back asserts). Formats without a MIME are skipped.
    pub fn current(&self) -> Vec<RawRepresentation> {
        lock(&self.state)
            .formats
            .iter()
            .filter_map(|(f, bytes)| {
                Some(RawRepresentation {
                    mime: f.mime?,
                    platform_format: f.name.clone(),
                    bytes: bytes.clone(),
                    width: None,
                    height: None,
                    truncated: false,
                })
            })
            .collect()
    }

    /// How many times `snapshot_meta` was invoked (counted even when it returns `Busy`).
    pub fn snapshot_meta_calls(&self) -> u32 {
        lock(&self.state).snapshot_meta_calls
    }

    /// How many times `read` was invoked (privacy tests: excluded content is never read).
    pub fn read_calls(&self) -> u32 {
        lock(&self.state).read_calls
    }
}

struct FakeMonitor {
    state: Shared,
}

impl ClipboardMonitor for FakeMonitor {
    fn start(&mut self, tx: Sender<ChangeSignal>) -> Result<(), PlatformError> {
        lock(&self.state).tx = Some(tx);
        Ok(())
    }

    fn stop(&mut self) {
        lock(&self.state).tx = None;
    }
}

struct FakeSource {
    state: Shared,
}

impl SourceResolver for FakeSource {
    fn resolve(&self) -> Option<ClipboardSource> {
        lock(&self.state).source.clone()
    }
}

struct FakeAccess {
    state: Shared,
}

impl ClipboardAccess for FakeAccess {
    fn sequence(&self) -> Result<u64, PlatformError> {
        Ok(lock(&self.state).seq)
    }

    fn snapshot_meta(&mut self) -> Result<ClipboardMeta, PlatformError> {
        let mut st = lock(&self.state);
        st.snapshot_meta_calls += 1;
        st.take_busy()?;
        Ok(ClipboardMeta {
            seq: st.seq,
            formats: st.formats.iter().map(|(f, _)| f.clone()).collect(),
            markers: st.markers.clone(),
            source: st.source.clone(),
        })
    }

    fn read(&mut self, plan: &ReadPlan) -> Result<Vec<RawRepresentation>, PlatformError> {
        let mut st = lock(&self.state);
        st.read_calls += 1;
        st.take_busy()?;
        let mut out = Vec::new();
        for want in &plan.formats {
            let Some((_, bytes)) = st
                .formats
                .iter()
                .find(|(f, _)| f.name == want.platform_format)
            else {
                continue;
            };
            let (bytes, truncated) = if want.mime == Mime::ImagePng {
                if len_u64(bytes.len()) > plan.image_max_bytes {
                    return Err(PlatformError::TooLarge {
                        format: want.platform_format.clone(),
                        bytes: len_u64(bytes.len()),
                    });
                }
                (bytes.clone(), false)
            } else {
                let cut = truncate_utf8(bytes, plan.truncate_text_at);
                let was_cut = cut.len() < bytes.len();
                (cut, was_cut)
            };
            out.push(RawRepresentation {
                mime: want.mime,
                platform_format: want.platform_format.clone(),
                bytes,
                width: None,
                height: None,
                truncated,
            });
        }
        Ok(out)
    }

    fn write(&mut self, reps: &[RawRepresentation]) -> Result<u64, PlatformError> {
        let mut st = lock(&self.state);
        st.take_busy()?;
        st.seq += 1;
        st.formats = reps
            .iter()
            .map(|r| {
                (
                    PlatformFormat {
                        name: r.platform_format.clone(),
                        mime: Some(r.mime),
                        size_hint: Some(len_u64(r.bytes.len())),
                    },
                    r.bytes.clone(),
                )
            })
            .collect();
        st.markers = Vec::new();
        st.source = None;
        emit(&st);
        Ok(st.seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caps::{Capabilities, ShortcutSupport, SourceSupport, Support};
    use crate::error::PlatformError;
    use darkclip_core::policy::{AvailableFormat, ReadPlan};
    use darkclip_core::Mime;
    use std::sync::mpsc;

    fn pf(name: &str, mime: Mime, size: Option<u64>) -> PlatformFormat {
        PlatformFormat {
            name: name.to_string(),
            mime: Some(mime),
            size_hint: size,
        }
    }

    fn text_copy(s: &str) -> FakeCopy {
        FakeCopy {
            formats: vec![(
                pf("CF_UNICODETEXT", Mime::TextPlain, None),
                s.as_bytes().to_vec(),
            )],
            markers: Vec::new(),
            source: None,
        }
    }

    fn plan(formats: &[(&str, Mime)], text_cap: u64, image_cap: u64) -> ReadPlan {
        ReadPlan {
            formats: formats
                .iter()
                .map(|(n, m)| AvailableFormat {
                    mime: *m,
                    platform_format: (*n).to_string(),
                    size_hint: None,
                })
                .collect(),
            truncate_text_at: text_cap,
            image_max_bytes: image_cap,
        }
    }

    fn started() -> (PlatformBundle, FakeHandle, mpsc::Receiver<ChangeSignal>) {
        let (fake, handle) = FakePlatform::new();
        let mut b = fake.bundle();
        let (tx, rx) = mpsc::channel();
        b.monitor.start(tx).unwrap();
        (b, handle, rx)
    }

    #[test]
    fn fake_copy_emits_signal_with_new_seq() {
        let (b, h, rx) = started();
        let before = b.access.sequence().unwrap();
        let seq = h.copy(text_copy("hello"));
        assert!(seq > before);
        assert_eq!(b.access.sequence().unwrap(), seq);
        let sig = rx.try_recv().unwrap();
        assert_eq!(sig.seq, seq);
        assert!(rx.try_recv().is_err(), "exactly one signal");
    }

    #[test]
    fn signal_again_keeps_seq() {
        let (b, h, rx) = started();
        let seq = h.copy(text_copy("x"));
        h.signal_again(3);
        let sigs: Vec<_> = rx.try_iter().collect();
        assert_eq!(sigs.len(), 4);
        assert!(sigs.iter().all(|s| s.seq == seq));
        assert_eq!(b.access.sequence().unwrap(), seq);
    }

    #[test]
    fn snapshot_meta_does_not_count_as_read() {
        let (mut b, h, _rx) = started();
        let seq = h.copy(text_copy("secret-ish"));
        let meta = b.access.snapshot_meta().unwrap();
        assert_eq!(meta.seq, seq);
        assert_eq!(meta.formats.len(), 1);
        assert_eq!(h.read_calls(), 0);
        let p = plan(&[("CF_UNICODETEXT", Mime::TextPlain)], 100, 100);
        b.access.read(&p).unwrap();
        assert_eq!(h.read_calls(), 1);
    }

    #[test]
    fn write_returns_new_seq_and_updates_current() {
        let (mut b, h, rx) = started();
        let seq = h.copy(text_copy("old"));
        let _ = rx.try_recv();
        let rep = RawRepresentation {
            mime: Mime::TextPlain,
            platform_format: "CF_UNICODETEXT".to_string(),
            bytes: b"new".to_vec(),
            width: None,
            height: None,
            truncated: false,
        };
        let new_seq = b.access.write(&[rep]).unwrap();
        assert!(new_seq > seq);
        assert_eq!(b.access.sequence().unwrap(), new_seq);
        let cur = h.current();
        assert_eq!(cur.len(), 1);
        assert_eq!(cur[0].bytes, b"new");
        assert_eq!(rx.try_recv().unwrap().seq, new_seq);
    }

    #[test]
    fn busy_then_ok() {
        let (mut b, h, _rx) = started();
        h.copy(text_copy("x"));
        h.set_busy(2);
        assert_eq!(b.access.snapshot_meta().unwrap_err(), PlatformError::Busy);
        assert_eq!(b.access.snapshot_meta().unwrap_err(), PlatformError::Busy);
        assert!(b.access.snapshot_meta().is_ok());
    }

    #[test]
    fn render_changes_formats_without_new_seq() {
        let (mut b, h, rx) = started();
        let seq = h.copy(FakeCopy {
            formats: vec![(pf("DataObject", Mime::TextPlain, None), Vec::new())],
            markers: Vec::new(),
            source: None,
        });
        let _ = rx.try_recv();
        h.render(vec![(
            pf("CF_UNICODETEXT", Mime::TextPlain, None),
            b"late".to_vec(),
        )]);
        let meta = b.access.snapshot_meta().unwrap();
        assert_eq!(meta.seq, seq);
        assert_eq!(meta.formats[0].name, "CF_UNICODETEXT");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn clear_source_removes_source() {
        let (b, h, _rx) = started();
        let mut c = text_copy("x");
        c.source = Some(darkclip_core::ClipboardSource {
            app_id: "a.exe".to_string(),
            app_name: None,
            platform: darkclip_core::Platform::Windows,
            confidence: darkclip_core::SourceConfidence::Owner,
        });
        h.copy(c);
        assert!(b.source.resolve().is_some());
        h.clear_source();
        assert!(b.source.resolve().is_none());
    }

    #[test]
    fn read_skips_formats_absent_from_plan() {
        let (mut b, h, _rx) = started();
        h.copy(FakeCopy {
            formats: vec![
                (pf("CF_UNICODETEXT", Mime::TextPlain, None), b"a".to_vec()),
                (
                    pf("HTML Format", Mime::TextHtml, None),
                    b"<b>a</b>".to_vec(),
                ),
            ],
            markers: Vec::new(),
            source: None,
        });
        let p = plan(&[("CF_UNICODETEXT", Mime::TextPlain)], 100, 100);
        let reps = b.access.read(&p).unwrap();
        assert_eq!(reps.len(), 1);
        assert_eq!(reps[0].platform_format, "CF_UNICODETEXT");
    }

    #[test]
    fn fake_read_truncates_html_and_uri_list() {
        let (mut b, h, _rx) = started();
        h.copy(FakeCopy {
            formats: vec![
                (
                    pf("HTML Format", Mime::TextHtml, None),
                    "ab\u{e9}cd".as_bytes().to_vec(),
                ),
                (
                    pf("text/uri-list", Mime::UriList, None),
                    b"file:///a\nfile:///b".to_vec(),
                ),
            ],
            markers: Vec::new(),
            source: None,
        });
        // Cap 3 falls inside the two-byte e-acute (bytes 2..4): back off to 2.
        let p = plan(
            &[
                ("HTML Format", Mime::TextHtml),
                ("text/uri-list", Mime::UriList),
            ],
            3,
            1,
        );
        let reps = b.access.read(&p).unwrap();
        assert_eq!(reps[0].bytes, b"ab");
        assert_eq!(reps[1].bytes, b"fil");
        assert!(reps[0].truncated && reps[1].truncated);
    }

    #[test]
    fn fake_read_under_cap_is_not_truncated() {
        let (mut b, h, _rx) = started();
        h.copy(text_copy("abc"));
        let p = plan(&[("CF_UNICODETEXT", Mime::TextPlain)], 3, 1);
        let reps = b.access.read(&p).unwrap();
        assert_eq!(reps[0].bytes, b"abc");
        assert!(!reps[0].truncated);
        assert!(h.current().iter().all(|r| !r.truncated));
    }

    #[test]
    fn snapshot_meta_calls_counts_meta_not_read() {
        let (mut b, h, _rx) = started();
        h.copy(text_copy("x"));
        assert_eq!(h.snapshot_meta_calls(), 0);
        b.access.snapshot_meta().unwrap();
        assert_eq!(h.snapshot_meta_calls(), 1);
        assert_eq!(h.read_calls(), 0);
        let p = plan(&[("CF_UNICODETEXT", Mime::TextPlain)], 10, 10);
        b.access.read(&p).unwrap();
        assert_eq!(h.snapshot_meta_calls(), 1);
        assert_eq!(h.read_calls(), 1);
    }

    #[test]
    fn fake_read_rejects_oversize_image_even_without_size_hint() {
        let (mut b, h, _rx) = started();
        h.copy(FakeCopy {
            formats: vec![(pf("PNG", Mime::ImagePng, None), vec![0u8; 11])],
            markers: Vec::new(),
            source: None,
        });
        let p = plan(&[("PNG", Mime::ImagePng)], 1000, 10);
        assert_eq!(
            b.access.read(&p).unwrap_err(),
            PlatformError::TooLarge {
                format: "PNG".to_string(),
                bytes: 11
            }
        );
        // At the cap is fine.
        let p = plan(&[("PNG", Mime::ImagePng)], 1000, 11);
        assert_eq!(b.access.read(&p).unwrap()[0].bytes.len(), 11);
    }

    #[test]
    fn caps_debug_and_serde() {
        let caps = Capabilities {
            monitor: Support::Polling { interval_ms: 250 },
            read_without_focus: Support::Native,
            write: Support::Native,
            concealed_markers: Support::Unsupported {
                reason: "no markers",
            },
            source_app: SourceSupport::Frontmost,
            global_shortcut: ShortcutSupport::Portal,
            auto_paste: Support::Native,
            tray: Support::Native,
            autostart: Support::Native,
            backend: "fake",
        };
        assert!(format!("{caps:?}").contains("Polling"));
        let v = serde_json::to_value(caps).unwrap();
        assert_eq!(v["backend"], "fake");
        assert_eq!(v["monitor"]["kind"], "polling");
        assert_eq!(v["monitor"]["interval_ms"], 250);
        assert_eq!(v["source_app"], "frontmost");
        assert_eq!(v["concealed_markers"]["reason"], "no markers");
        assert_eq!(v["global_shortcut"], "portal");
    }
}
