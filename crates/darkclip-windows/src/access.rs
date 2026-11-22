//! `WinAccess`: clipboard access from the capture actor's thread.
//!
//! `sequence()` (Task 101) and `snapshot_meta` (Task 102) work; `read` and `write` arrive in
//! Tasks 103-104. Adapter rule (S-5): a failed `OpenClipboard` returns `PlatformError::Busy`
//! once; the actor owns retries. No policy here (GC-04), no content in errors (GC-06).
//!
//! `snapshot_meta` goes through the small `ClipboardOs` seam so tests can count OS calls. It
//! never fetches the data of a content format: the only data it reads are the DWORDs of the
//! `CanIncludeInClipboardHistory` / `CanUploadToCloudClipboard` markers (privacy-design §2).

use crate::formats::{self, MarkerId, OfferedFormat};
use crate::source::WinSourceResolver;
use crate::win;
use darkclip_core::markers::windows_markers;
use darkclip_core::policy::ReadPlan;
use darkclip_core::{ClipboardSource, RawRepresentation};
use darkclip_platform::{
    ClipboardAccess, ClipboardMeta, PlatformError, PlatformFormat, SourceResolver,
};

const NOT_YET: PlatformError = PlatformError::Unsupported("not yet");

/// The OS calls `snapshot_meta` needs. The real implementation is `WinOs`; tests use a fake.
pub(crate) trait ClipboardOs {
    /// The clipboard, open by this thread; closed when dropped.
    type Open<'a>: OpenClipboardView
    where
        Self: 'a;

    /// `RegisterClipboardFormatW`; `None` when registration fails (id 0).
    fn register(&mut self, name: &str) -> Option<u32>;

    /// One `OpenClipboard` attempt; `Busy` when another process holds it (S-5).
    fn open(&mut self) -> Result<Self::Open<'_>, PlatformError>;
}

/// Calls made while the clipboard is open.
pub(crate) trait OpenClipboardView {
    /// `GetClipboardSequenceNumber`, read while open so it matches the enumerated formats.
    fn sequence(&self) -> u64;
    /// Every format id, in clipboard order. An enumeration error is an error, never a
    /// shortened list (a missing marker must not look like "no marker").
    fn enum_formats(&self) -> Result<Vec<u32>, PlatformError>;
    /// `GetClipboardFormatNameW` for a registered or private format.
    fn format_name(&self, id: u32) -> Option<String>;
    /// The DWORD of a marker format: data exactly 4 bytes, else `None`. Called only for the
    /// two DWORD marker ids.
    fn dword_value(&self, id: u32) -> Option<u32>;
    /// The clipboard owner's app id (`WinSourceResolver`).
    fn owner_source(&self) -> Option<ClipboardSource>;
}

/// Registered ids of the marker names, cached after the first full registration.
#[derive(Default)]
pub(crate) struct MarkerCache(Option<Vec<MarkerId>>);

impl MarkerCache {
    /// The marker ids, registering the names on first use. Needs no open clipboard, so it
    /// runs before `open` and keeps the clipboard open as briefly as possible. A failed
    /// registration is an error (the change is not captured), never a missed marker.
    fn ids<O: ClipboardOs>(&mut self, os: &mut O) -> Result<Vec<MarkerId>, PlatformError> {
        if let Some(ids) = &self.0 {
            return Ok(ids.clone());
        }
        let mut ids = Vec::with_capacity(formats::MARKER_NAMES.len());
        for &name in formats::MARKER_NAMES {
            let id = os
                .register(name)
                .ok_or(PlatformError::Backend { code: -1 })?;
            ids.push((name, id));
        }
        self.0 = Some(ids.clone());
        Ok(ids)
    }
}

/// `snapshot_meta` over the seam: names, markers (by id), sequence and source; never content.
/// The only data fetched are the DWORDs of the history and cloud markers.
pub(crate) fn snapshot<O: ClipboardOs>(
    os: &mut O,
    cache: &mut MarkerCache,
) -> Result<ClipboardMeta, PlatformError> {
    let registered = cache.ids(os)?;
    let (seq, offered, found, source) = {
        let open = os.open()?;
        let seq = open.sequence();
        let ids = open.enum_formats()?;
        let offered: Vec<OfferedFormat> = ids
            .iter()
            .map(|&id| OfferedFormat {
                id,
                name: format_name(&open, id),
            })
            .collect();
        let found: Vec<(&'static str, Option<u32>)> = formats::match_markers(&ids, &registered)
            .into_iter()
            .map(|(name, id)| {
                let value = if formats::marker_has_dword(name) {
                    open.dword_value(id)
                } else {
                    None
                };
                (name, value)
            })
            .collect();
        let source = open.owner_source();
        (seq, offered, found, source)
        // `open` closes the clipboard here.
    };
    let formats: Vec<PlatformFormat> = formats::map_formats(&offered, &registered);
    Ok(ClipboardMeta {
        seq,
        formats,
        markers: windows_markers(&found),
        source,
    })
}

/// Fixed name for standard formats, registered name otherwise, `#0xNNNN` when there is none.
fn format_name<V: OpenClipboardView>(open: &V, id: u32) -> String {
    if let Some(name) = formats::standard_format_name(id) {
        return name.to_owned();
    }
    open.format_name(id)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| format!("#0x{id:04X}"))
}

/// The real seam over `win.rs`.
#[derive(Default)]
pub(crate) struct WinOs {
    _private: (),
}

impl ClipboardOs for WinOs {
    type Open<'a> = win::ClipboardGuard;

    fn register(&mut self, name: &str) -> Option<u32> {
        win::register_clipboard_format(name)
    }

    fn open(&mut self) -> Result<win::ClipboardGuard, PlatformError> {
        win::ClipboardGuard::open_for_read()
    }
}

impl OpenClipboardView for win::ClipboardGuard {
    fn sequence(&self) -> u64 {
        u64::from(win::clipboard_sequence())
    }

    fn enum_formats(&self) -> Result<Vec<u32>, PlatformError> {
        win::ClipboardGuard::enum_formats(self)
    }

    fn format_name(&self, id: u32) -> Option<String> {
        win::clipboard_format_name(id)
    }

    fn dword_value(&self, id: u32) -> Option<u32> {
        win::ClipboardGuard::dword_value(self, id)
    }

    fn owner_source(&self) -> Option<ClipboardSource> {
        WinSourceResolver.resolve()
    }
}

/// Win32 clipboard access.
#[derive(Default)]
pub struct WinAccess {
    os: WinOs,
    markers: MarkerCache,
}

impl WinAccess {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ClipboardAccess for WinAccess {
    /// `GetClipboardSequenceNumber`; never fails (F-03).
    fn sequence(&self) -> Result<u64, PlatformError> {
        Ok(u64::from(win::clipboard_sequence()))
    }

    /// One `OpenClipboard` attempt (`Busy` otherwise), formats, markers, sequence, source.
    fn snapshot_meta(&mut self) -> Result<ClipboardMeta, PlatformError> {
        snapshot(&mut self.os, &mut self.markers)
    }

    fn read(&mut self, _plan: &ReadPlan) -> Result<Vec<RawRepresentation>, PlatformError> {
        Err(NOT_YET)
    }

    fn write(&mut self, _reps: &[RawRepresentation]) -> Result<u64, PlatformError> {
        Err(NOT_YET)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::{
        CF_LOCALE, CF_OEMTEXT, CF_TEXT, CF_UNICODETEXT, CLOUD, EXCLUDE_MONITOR, HISTORY,
    };
    use darkclip_core::markers::PrivacyMarker;
    use darkclip_core::{Mime, Platform, SourceConfidence};
    use std::cell::{Cell, RefCell};

    /// Fake OS: a clipboard of `(id, registered name)` entries plus DWORD data for some ids.
    /// Registration mimics atoms (case-insensitive, one id per name).
    struct FakeOs {
        /// `(id, name)`; standard formats carry no name.
        clipboard: Vec<(u32, Option<&'static str>)>,
        /// Registered names and their ids.
        atoms: RefCell<Vec<(String, u32)>>,
        /// DWORD data per id (`None` = data not exactly 4 bytes or unreadable).
        dwords: Vec<(u32, Option<u32>)>,
        busy: bool,
        enum_fails: bool,
        seq: u64,
        opens: Cell<u32>,
        closes: Cell<u32>,
        registers: Cell<u32>,
        /// Every name passed to `register`, in order.
        registered_names: RefCell<Vec<String>>,
        /// Every id passed to the data getter.
        data_reads: RefCell<Vec<u32>>,
    }

    impl FakeOs {
        fn new() -> Self {
            Self {
                clipboard: Vec::new(),
                atoms: RefCell::new(Vec::new()),
                dwords: Vec::new(),
                busy: false,
                enum_fails: false,
                seq: 42,
                opens: Cell::new(0),
                closes: Cell::new(0),
                registers: Cell::new(0),
                registered_names: RefCell::new(Vec::new()),
                data_reads: RefCell::new(Vec::new()),
            }
        }

        fn atom(&self, name: &str) -> u32 {
            let mut atoms = self.atoms.borrow_mut();
            if let Some((_, id)) = atoms.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                return *id;
            }
            let id = 0xC100 + u32::try_from(atoms.len()).unwrap();
            atoms.push((name.to_owned(), id));
            id
        }

        fn standard(&mut self, id: u32) -> &mut Self {
            self.clipboard.push((id, None));
            self
        }

        fn registered(&mut self, name: &'static str) -> &mut Self {
            let id = self.atom(name);
            self.clipboard.push((id, Some(name)));
            self
        }

        fn dword(&mut self, name: &'static str, value: Option<u32>) -> &mut Self {
            self.registered(name);
            let id = self.atom(name);
            self.dwords.push((id, value));
            self
        }

        /// Darkshot's set as `darkshot-windows/src/clipboard.rs` writes it: the main format,
        /// then the three exclusion formats, each with DWORD 0.
        fn darkshot_text(&mut self) -> &mut Self {
            self.standard(CF_UNICODETEXT)
                .standard(CF_LOCALE)
                .standard(CF_TEXT)
                .standard(CF_OEMTEXT)
                .dword(EXCLUDE_MONITOR, Some(0))
                .dword(HISTORY, Some(0))
                .dword(CLOUD, Some(0))
        }
    }

    struct FakeOpen<'a>(&'a FakeOs);

    impl Drop for FakeOpen<'_> {
        fn drop(&mut self) {
            self.0.closes.set(self.0.closes.get() + 1);
        }
    }

    impl ClipboardOs for FakeOs {
        type Open<'a> = FakeOpen<'a>;

        fn register(&mut self, name: &str) -> Option<u32> {
            self.registers.set(self.registers.get() + 1);
            self.registered_names.borrow_mut().push(name.to_owned());
            Some(self.atom(name))
        }

        fn open(&mut self) -> Result<FakeOpen<'_>, PlatformError> {
            self.opens.set(self.opens.get() + 1);
            if self.busy {
                return Err(PlatformError::Busy);
            }
            Ok(FakeOpen(self))
        }
    }

    impl OpenClipboardView for FakeOpen<'_> {
        fn sequence(&self) -> u64 {
            self.0.seq
        }

        fn enum_formats(&self) -> Result<Vec<u32>, PlatformError> {
            if self.0.enum_fails {
                return Err(PlatformError::Backend { code: 5 });
            }
            Ok(self.0.clipboard.iter().map(|(id, _)| *id).collect())
        }

        fn format_name(&self, id: u32) -> Option<String> {
            self.0
                .clipboard
                .iter()
                .find(|(i, _)| *i == id)
                .and_then(|(_, n)| n.map(str::to_owned))
        }

        fn dword_value(&self, id: u32) -> Option<u32> {
            self.0.data_reads.borrow_mut().push(id);
            self.0
                .dwords
                .iter()
                .find(|(i, _)| *i == id)
                .and_then(|(_, v)| *v)
        }

        fn owner_source(&self) -> Option<ClipboardSource> {
            Some(ClipboardSource {
                app_id: "darkshot.exe".to_owned(),
                app_name: None,
                platform: Platform::Windows,
                confidence: SourceConfidence::Owner,
            })
        }
    }

    fn snap(os: &mut FakeOs) -> Result<ClipboardMeta, PlatformError> {
        snapshot(os, &mut MarkerCache::default())
    }

    #[test]
    fn snapshot_never_reads_content() {
        let mut os = FakeOs::new();
        os.darkshot_text().registered("PNG");
        let meta = snap(&mut os).unwrap();
        let history = os.atom(HISTORY);
        let cloud = os.atom(CLOUD);
        let mut reads = os.data_reads.borrow().clone();
        reads.sort_unstable();
        let mut want = vec![history, cloud];
        want.sort_unstable();
        assert_eq!(reads, want, "only the two marker DWORDs are fetched");
        assert_eq!(os.opens.get(), 1);
        assert_eq!(os.closes.get(), 1, "the clipboard is closed");
        assert_eq!(meta.seq, 42);
        assert_eq!(
            meta.formats
                .iter()
                .filter_map(|f| f.mime)
                .collect::<Vec<_>>(),
            vec![Mime::TextPlain, Mime::ImagePng]
        );

        // Plain text and PNG with no markers: no data read at all.
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT).registered("PNG");
        snap(&mut os).unwrap();
        assert!(os.data_reads.borrow().is_empty());
    }

    #[test]
    fn busy_returns_busy_once() {
        let mut os = FakeOs::new();
        os.darkshot_text();
        os.busy = true;
        assert_eq!(snap(&mut os).unwrap_err(), PlatformError::Busy);
        assert_eq!(os.opens.get(), 1, "one OpenClipboard attempt, no retry");
        assert_eq!(os.closes.get(), 0);
        assert!(os.data_reads.borrow().is_empty());
    }

    #[test]
    fn darkshot_set_reports_darkshot_marker() {
        let mut os = FakeOs::new();
        os.darkshot_text();
        let meta = snap(&mut os).unwrap();
        assert!(
            meta.markers.contains(&PrivacyMarker::DarkshotExcluded),
            "{:?}",
            meta.markers
        );
        assert_eq!(meta.source.unwrap().app_id, "darkshot.exe");
        // Marker formats are listed with canonical names and no MIME.
        for name in [EXCLUDE_MONITOR, HISTORY, CLOUD] {
            let f = meta.formats.iter().find(|f| f.name == name).unwrap();
            assert_eq!(f.mime, None);
        }
    }

    #[test]
    fn exclude_format_any_data_reports_concealed() {
        // No data on the exclude format at all (and never fetched).
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT).registered(EXCLUDE_MONITOR);
        let meta = snap(&mut os).unwrap();
        assert_eq!(
            meta.markers,
            vec![PrivacyMarker::OsConcealed {
                name: EXCLUDE_MONITOR
            }]
        );
        assert!(os.data_reads.borrow().is_empty());

        // Any data, e.g. a DWORD of 1: still concealed, and not read.
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT).dword(EXCLUDE_MONITOR, Some(1));
        let meta = snap(&mut os).unwrap();
        assert!(meta
            .markers
            .iter()
            .any(darkclip_core::markers::is_mandatory));
        assert!(os.data_reads.borrow().is_empty());
    }

    #[test]
    fn history_dword_zero_concealed_and_one_not() {
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT).dword(HISTORY, Some(0));
        let meta = snap(&mut os).unwrap();
        assert_eq!(
            meta.markers,
            vec![PrivacyMarker::OsConcealed { name: HISTORY }]
        );

        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT).dword(HISTORY, Some(1));
        let meta = snap(&mut os).unwrap();
        assert!(meta.markers.is_empty(), "{:?}", meta.markers);

        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT).dword(CLOUD, Some(0));
        assert_eq!(snap(&mut os).unwrap().markers, vec![PrivacyMarker::NoCloud]);
    }

    #[test]
    fn unreadable_dword_reported_none() {
        // The adapter reports `None`; core fails closed (history concealed, cloud NoCloud).
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT)
            .dword(HISTORY, None)
            .dword(CLOUD, None);
        let meta = snap(&mut os).unwrap();
        assert_eq!(
            meta.markers,
            windows_markers(&[(HISTORY, None), (CLOUD, None)])
        );
        assert!(meta
            .markers
            .contains(&PrivacyMarker::OsConcealed { name: HISTORY }));
        assert!(meta.markers.contains(&PrivacyMarker::NoCloud));
    }

    #[test]
    fn lookalike_marker_name_under_other_id_is_not_a_marker() {
        // A name differing only by a trailing space is a different atom (another id).
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT)
            .registered("CanIncludeInClipboardHistory ");
        let meta = snap(&mut os).unwrap();
        assert!(meta.markers.is_empty(), "{:?}", meta.markers);
        assert!(os.data_reads.borrow().is_empty());
    }

    #[test]
    fn ole_placeholder_phase_has_no_mime() {
        let mut os = FakeOs::new();
        os.registered("DataObject").registered("Ole Private Data");
        let meta = snap(&mut os).unwrap();
        assert_eq!(meta.formats.len(), 2);
        assert!(meta.formats.iter().all(|f| f.mime.is_none()));
        assert!(meta.markers.is_empty());
    }

    #[test]
    fn enumeration_error_is_an_error_and_closes() {
        let mut os = FakeOs::new();
        os.darkshot_text();
        os.enum_fails = true;
        assert!(matches!(
            snap(&mut os).unwrap_err(),
            PlatformError::Backend { .. }
        ));
        assert_eq!(os.closes.get(), os.opens.get());
        assert!(os.data_reads.borrow().is_empty());
    }

    #[test]
    fn marker_ids_registered_once_and_before_open() {
        let mut os = FakeOs::new();
        os.darkshot_text();
        let mut cache = MarkerCache::default();
        snapshot(&mut os, &mut cache).unwrap();
        let first = os.registers.get();
        assert_eq!(first, u32::try_from(formats::MARKER_NAMES.len()).unwrap());
        snapshot(&mut os, &mut cache).unwrap();
        assert_eq!(os.registers.get(), first, "ids are cached");
        // Registration still happens when the clipboard is busy (it needs no open clipboard).
        let mut os = FakeOs::new();
        os.busy = true;
        let _ = snap(&mut os);
        assert_eq!(os.registers.get(), first);
    }

    #[test]
    fn registers_exactly_core_windows_marker_names() {
        let mut os = FakeOs::new();
        os.standard(CF_UNICODETEXT);
        snap(&mut os).unwrap();
        let names = os.registered_names.borrow().clone();
        assert_eq!(names, darkclip_core::markers::WINDOWS_MARKER_NAMES);
    }

    #[test]
    fn unnamed_private_format_gets_a_placeholder_name() {
        let mut os = FakeOs::new();
        os.standard(0x0200).standard(CF_UNICODETEXT);
        let meta = snap(&mut os).unwrap();
        let f = meta.formats.first().unwrap();
        assert_eq!(f.mime, None);
        assert!(!f.name.is_empty());
    }

    #[test]
    fn sequence_is_ok() {
        let a = WinAccess::new();
        let first = a.sequence().unwrap();
        // Never fails, and is stable or increasing (another app may copy meanwhile).
        let second = a.sequence().unwrap();
        assert!(second >= first);
    }

    #[test]
    fn read_and_write_are_unsupported() {
        let mut a = WinAccess::new();
        assert_eq!(a.write(&[]).unwrap_err(), NOT_YET);
        let plan = ReadPlan {
            formats: Vec::new(),
            truncate_text_at: 0,
            image_max_bytes: 0,
        };
        assert_eq!(a.read(&plan).unwrap_err(), NOT_YET);
    }

    #[test]
    fn snapshot_meta_on_current_clipboard_does_not_fail() {
        // Read-only: names, markers and the two marker DWORDs only.
        let mut a = WinAccess::new();
        match a.snapshot_meta() {
            Ok(meta) => {
                assert!(meta.seq > 0 || meta.formats.is_empty());
                let mimes: Vec<Mime> = meta.formats.iter().filter_map(|f| f.mime).collect();
                for m in &mimes {
                    assert_eq!(mimes.iter().filter(|x| *x == m).count(), 1, "S-1");
                }
            }
            Err(e) => assert_eq!(e, PlatformError::Busy),
        }
    }

    #[test]
    #[ignore = "writes the real clipboard"]
    fn real_clipboard_write_darkshot_set_detected() {
        let seq = (0..10)
            .find_map(|_| {
                let r = win::test_support::write_darkshot_text_set("darkclip test 102");
                if r.is_err() {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                r.ok()
            })
            .expect("clipboard stayed busy");
        let mut a = WinAccess::new();
        let meta = (0..10)
            .find_map(|_| match a.snapshot_meta() {
                Ok(m) => Some(m),
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    None
                }
            })
            .expect("clipboard stayed busy");
        assert_eq!(meta.seq, u64::from(seq));
        assert!(
            meta.markers.contains(&PrivacyMarker::DarkshotExcluded),
            "{:?}",
            meta.markers
        );
    }

    #[test]
    #[ignore = "writes the real clipboard"]
    fn real_clipboard_write_powershell_ole_phases() {
        // Spike S5: a .NET writer produces a placeholder-only phase, then real formats. Every
        // snapshot is either placeholder-only (no MIME at all) or offers plain text once.
        let (tx, rx) = std::sync::mpsc::channel::<Result<ClipboardMeta, PlatformError>>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<win::RawWindow>();
        let handle = std::thread::spawn(move || {
            let mut a = WinAccess::new();
            let on_update: win::UpdateHandler = Box::new(move || {
                let _ = tx.send(a.snapshot_meta());
                true
            });
            let w = win::MessageWindow::create(Some(on_update)).unwrap();
            ready_tx.send(w.raw()).unwrap();
            w.run();
        });
        let raw = ready_rx.recv().unwrap();
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "Set-Clipboard -Value x"])
            .status()
            .unwrap();
        assert!(status.success());
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(raw.post_stop());
        handle.join().unwrap();
        let snaps: Vec<ClipboardMeta> = rx.try_iter().filter_map(Result::ok).collect();
        assert!(!snaps.is_empty());
        let mut saw_text = false;
        for s in &snaps {
            let mimes: Vec<Mime> = s.formats.iter().filter_map(|f| f.mime).collect();
            if mimes.contains(&Mime::TextPlain) {
                saw_text = true;
            }
            for m in &mimes {
                assert_eq!(mimes.iter().filter(|x| *x == m).count(), 1, "S-1");
            }
        }
        assert!(saw_text);
    }
}
