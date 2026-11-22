//! The popup's detail pane (Task 206), as a view-model with no window and no I/O.
//!
//! Rules that live here so they are testable:
//! - A masked clip never asks for content: every request for it is `None`, and the model holds
//!   nothing but the badge flag for it (the row's masked preview is all the user ever sees).
//! - Loads are on demand and run on the worker thread; this model only says *what* to load.
//!   Decoded images are cached in an LRU bounded by bytes (`CACHE_BUDGET`, 32 MiB). The host
//!   clears it when the popup hides, so decoded clipboard images do not stay in memory.
//! - Displayed text is capped at 32 KiB (`TEXT_DISPLAY_CAP`; the app cuts at the same size).
//! - File paths are shown (the user copied them) and never logged; `Debug` prints none.

use super::list::Item;
use darkclip_core::{ClipId, ClipType};

/// Text shown in the detail pane is cut at this many bytes (keep equal to the app's
/// `UI_TEXT_MAX_BYTES`) (on a character boundary).
pub const TEXT_DISPLAY_CAP: usize = 32 * 1024;
/// Decoded images cached at most (bytes of RGBA pixels).
pub const CACHE_BUDGET: usize = 32 * 1024 * 1024;
/// The "larger preview" is downscaled to at most this many pixels on its longest side.
pub const FULL_IMAGE_SIDE: u32 = 1024;
/// File names listed in the pane (the app already caps what it sends).
pub const MAX_FILES_SHOWN: usize = 100;

/// What can be loaded for a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want {
    /// Image size and thumbnail, or the file list: small, loaded when the pane shows the clip.
    Detail,
    /// The stored text, up to the display cap.
    FullText,
    /// The image itself, downscaled (decoded on the worker).
    FullImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetailRequest {
    pub id: ClipId,
    pub want: Want,
}

/// The request to load `want` for `item`, or `None` when it must not or cannot be loaded.
/// Masked clips never load anything.
pub fn request_for(item: &Item, want: Want) -> Option<DetailRequest> {
    if item.masked {
        return None;
    }
    let ok = match want {
        Want::Detail => matches!(item.kind, ClipType::Image | ClipType::Files),
        Want::FullText => has_text(item.kind),
        Want::FullImage => item.kind == ClipType::Image,
    };
    ok.then_some(DetailRequest { id: item.id, want })
}

fn has_text(k: ClipType) -> bool {
    matches!(
        k,
        ClipType::Text | ClipType::RichText | ClipType::Url | ClipType::Code | ClipType::Color
    )
}

/// `text` cut to `TEXT_DISPLAY_CAP` bytes on a character boundary; true when it was cut.
pub fn cap_display(mut text: String) -> (String, bool) {
    if text.len() <= TEXT_DISPLAY_CAP {
        return (text, false);
    }
    let mut end = TEXT_DISPLAY_CAP;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}

/// A least-recently-used cache bounded by the bytes its entries say they weigh.
pub struct ByteLru<K, V> {
    budget: usize,
    bytes: usize,
    /// Oldest first.
    entries: Vec<(K, V, usize)>,
}

impl<K: PartialEq, V: Clone> ByteLru<K, V> {
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            bytes: 0,
            entries: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The value, now the most recently used.
    pub fn get(&mut self, key: &K) -> Option<V> {
        let at = self.entries.iter().position(|(k, _, _)| k == key)?;
        let e = self.entries.remove(at);
        let v = e.1.clone();
        self.entries.push(e);
        Some(v)
    }

    pub fn contains(&self, key: &K) -> bool {
        self.entries.iter().any(|(k, _, _)| k == key)
    }

    /// Stores `value` weighing `bytes`; the oldest entries go until the total fits. A value
    /// heavier than the whole budget is not stored.
    pub fn insert(&mut self, key: K, value: V, bytes: usize) {
        self.remove(&key);
        if bytes > self.budget {
            return;
        }
        self.entries.push((key, value, bytes));
        self.bytes += bytes;
        while self.bytes > self.budget && !self.entries.is_empty() {
            let (_, _, b) = self.entries.remove(0);
            self.bytes -= b;
        }
    }

    pub fn remove(&mut self, key: &K) {
        if let Some(at) = self.entries.iter().position(|(k, _, _)| k == key) {
            let (_, _, b) = self.entries.remove(at);
            self.bytes -= b;
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

/// One file of a file clip, ready to show.
#[derive(Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
}

/// Never prints names or paths (GC-06).
impl std::fmt::Debug for FileEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileEntry").finish_non_exhaustive()
    }
}

fn hex(b: u8) -> Option<u8> {
    char::from(b)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if let (Some(h), Some(l)) = (
                b.get(i + 1).copied().and_then(hex),
                b.get(i + 2).copied().and_then(hex),
            ) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Invisible direction controls: a name like `invoice<RLO>fdp.exe` would read as `invoiceexe.pdf`.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// A line of a `text/uri-list` as a display path. `file:///C:/a%20b.txt` becomes
/// `C:\a b.txt`, `file://server/share/x` becomes `\\server\share\x`, a POSIX path stays
/// POSIX. Control characters are replaced so a name cannot move the text around.
pub fn display_path(uri: &str) -> String {
    let lower = uri.get(..7).map(str::to_ascii_lowercase);
    let decoded = if lower.as_deref() == Some("file://") {
        let rest = percent_decode(uri.get(7..).unwrap_or(""));
        let drive = {
            let b = rest.as_bytes();
            b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':'
        };
        if drive {
            rest.get(1..).unwrap_or("").replace('/', "\\")
        } else if rest.starts_with('/') {
            rest
        } else {
            // file://server/share/x
            format!("\\\\{}", rest.replace('/', "\\"))
        }
    } else {
        percent_decode(uri)
    };
    decoded
        .chars()
        .map(|c| {
            if c.is_control() {
                '?'
            } else if is_bidi_control(c) {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

/// The entries of a file clip (at most `MAX_FILES_SHOWN`).
pub fn file_entries(uris: &[String]) -> Vec<FileEntry> {
    uris.iter()
        .take(MAX_FILES_SHOWN)
        .map(|u| {
            let path = display_path(u);
            let name = path
                .trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .next()
                .filter(|n| !n.is_empty())
                .unwrap_or(&path)
                .to_string();
            FileEntry { name, path }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Size {
    Thumb,
    Full,
}

/// A decoded picture and its pixel size; `bytes` is what it weighs in the cache.
#[derive(Clone)]
pub struct Pic<P> {
    pub pixels: P,
    pub width: u32,
    pub height: u32,
}

/// What the worker found for `Want::Detail`.
pub struct DetailData<P> {
    /// The clip turned out to be masked: nothing else is set.
    pub masked: bool,
    /// Original image size.
    pub dims: Option<(u32, u32)>,
    pub thumb: Option<Pic<P>>,
    pub files: Vec<FileEntry>,
    pub file_total: usize,
}

/// What the pane shows right now.
pub struct DetailView<P> {
    pub masked: bool,
    pub text: Option<String>,
    /// The shown text is cut (stored preview only, or cut at the display cap).
    pub text_cut: bool,
    pub image: Option<P>,
    /// The larger preview is shown (read by the tests; the pane shows one picture either way).
    #[cfg_attr(not(test), allow(dead_code))]
    pub image_is_full: bool,
    pub dims: Option<(u32, u32)>,
    pub files: Vec<FileEntry>,
    /// Files in the clip that are not listed.
    pub files_more: usize,
    pub file_total: usize,
    pub loading: bool,
    pub failed: bool,
    pub can_load_text: bool,
    pub can_load_image: bool,
}

/// State of the pane for the selected clip. `P` is the host's pixel type.
pub struct DetailModel<P: Clone> {
    open: bool,
    clip: Option<ClipId>,
    kind: Option<ClipType>,
    masked: bool,
    text: Option<(String, bool)>,
    files: Vec<FileEntry>,
    file_total: usize,
    dims: Option<(u32, u32)>,
    detail_loaded: bool,
    detail_pending: bool,
    text_pending: bool,
    image_pending: bool,
    failed: bool,
    cache: ByteLru<(ClipId, Size), Pic<P>>,
}

impl<P: Clone> Default for DetailModel<P> {
    fn default() -> Self {
        Self::with_budget(CACHE_BUDGET)
    }
}

impl<P: Clone> DetailModel<P> {
    pub fn with_budget(budget: usize) -> Self {
        Self {
            open: false,
            clip: None,
            kind: None,
            masked: false,
            text: None,
            files: Vec::new(),
            file_total: 0,
            dims: None,
            detail_loaded: false,
            detail_pending: false,
            text_pending: false,
            image_pending: false,
            failed: false,
            cache: ByteLru::new(budget),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    #[cfg(test)]
    pub fn cache_bytes(&self) -> usize {
        self.cache.bytes()
    }

    fn reset_clip(&mut self, item: Option<&Item>) {
        self.clip = item.map(|i| i.id);
        self.kind = item.map(|i| i.kind);
        self.masked = item.is_some_and(|i| i.masked);
        self.text = None;
        self.files.clear();
        self.file_total = 0;
        self.dims = None;
        self.detail_loaded = false;
        self.detail_pending = false;
        self.text_pending = false;
        self.image_pending = false;
        self.failed = false;
    }

    /// The current clip turned out to be masked: nothing loaded for it may stay.
    fn become_masked(&mut self) {
        self.masked = true;
        self.text = None;
        self.files.clear();
        self.file_total = 0;
        self.dims = None;
        self.detail_loaded = false;
        self.text_pending = false;
        self.image_pending = false;
        if let Some(id) = self.clip {
            self.cache.remove(&(id, Size::Thumb));
            self.cache.remove(&(id, Size::Full));
        }
    }

    /// The selected item (or the pane's open state) changed: what to load now. Idempotent:
    /// a clip already loading or loaded asks for nothing again.
    pub fn sync(&mut self, item: Option<&Item>) -> Vec<DetailRequest> {
        if self.clip != item.map(|i| i.id) {
            self.reset_clip(item);
        } else if let Some(i) = item {
            if i.masked && !self.masked {
                self.become_masked();
            }
            self.masked = i.masked;
        }
        let Some(item) = item.filter(|_| self.open) else {
            return Vec::new();
        };
        if self.detail_loaded || self.detail_pending {
            return Vec::new();
        }
        match request_for(item, Want::Detail) {
            Some(r) => {
                self.detail_pending = true;
                vec![r]
            }
            None => Vec::new(),
        }
    }

    pub fn set_open(&mut self, open: bool, item: Option<&Item>) -> Vec<DetailRequest> {
        self.open = open;
        self.sync(item)
    }

    /// "Load full text".
    pub fn load_full_text(&mut self, item: &Item) -> Option<DetailRequest> {
        self.sync(Some(item));
        if !self.open || self.text.is_some() || self.text_pending {
            return None;
        }
        let r = request_for(item, Want::FullText)?;
        self.text_pending = true;
        Some(r)
    }

    /// "Load larger preview".
    pub fn load_full_image(&mut self, item: &Item) -> Option<DetailRequest> {
        self.sync(Some(item));
        if !self.open || self.image_pending || self.cache.contains(&(item.id, Size::Full)) {
            return None;
        }
        let r = request_for(item, Want::FullImage)?;
        self.image_pending = true;
        Some(r)
    }

    /// The worker answered `Want::Detail`. Returns true when the pane changed.
    pub fn on_detail(&mut self, id: ClipId, r: Result<DetailData<P>, ()>, bytes: usize) -> bool {
        let current = self.clip == Some(id);
        if current {
            self.detail_pending = false;
        }
        let Ok(d) = r else {
            if current {
                self.failed = true;
            }
            return current;
        };
        if let Some(t) = d.thumb {
            self.cache.insert((id, Size::Thumb), t, bytes);
        }
        if !current {
            return false;
        }
        if d.masked {
            // The row said unmasked but storage says masked: show it as masked, drop the rest.
            self.become_masked();
            return true;
        }
        self.detail_loaded = true;
        self.dims = d.dims;
        self.files = d.files;
        self.file_total = d.file_total;
        true
    }

    /// The worker answered `Want::FullText`; the text is capped for display here as well.
    pub fn on_text(&mut self, id: ClipId, r: Result<(String, bool), ()>) -> bool {
        if self.clip != Some(id) || self.masked {
            return false;
        }
        self.text_pending = false;
        match r {
            Ok((text, cut)) => {
                let (text, cut2) = cap_display(text);
                self.text = Some((text, cut || cut2));
            }
            Err(()) => self.failed = true,
        }
        true
    }

    /// The worker answered `Want::FullImage`.
    pub fn on_full_image(&mut self, id: ClipId, r: Result<Pic<P>, ()>, bytes: usize) -> bool {
        let current = self.clip == Some(id);
        if current {
            self.image_pending = false;
        }
        match r {
            Ok(p) => {
                // A clip that turned out masked keeps no picture.
                if !(current && self.masked) {
                    self.cache.insert((id, Size::Full), p, bytes);
                }
                current
            }
            Err(()) => {
                if current {
                    self.failed = true;
                }
                current
            }
        }
    }

    /// A clip was deleted: forget anything held for it.
    pub fn forget(&mut self, id: ClipId) {
        self.cache.remove(&(id, Size::Thumb));
        self.cache.remove(&(id, Size::Full));
        if self.clip == Some(id) {
            self.reset_clip(None);
        }
    }

    /// The popup hid: drop every decoded image and the loaded text.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.reset_clip(None);
        self.open = false;
    }

    pub fn view(&mut self) -> DetailView<P> {
        let id = self.clip;
        let kind = self.kind;
        let masked = self.masked;
        let (mut image, mut image_is_full) = (None, false);
        let mut shown_side = 0;
        if let (Some(id), false) = (id, masked) {
            if let Some(p) = self.cache.get(&(id, Size::Full)) {
                shown_side = p.width.max(p.height);
                image = Some(p.pixels);
                image_is_full = true;
            } else if let Some(p) = self.cache.get(&(id, Size::Thumb)) {
                shown_side = p.width.max(p.height);
                image = Some(p.pixels);
            }
        }
        let is_image = kind == Some(ClipType::Image) && !masked;
        let larger_exists = self.dims.is_some_and(|(w, h)| w.max(h) > shown_side);
        DetailView {
            masked,
            text: self.text.as_ref().filter(|_| !masked).map(|t| t.0.clone()),
            text_cut: !masked && self.text.as_ref().is_some_and(|t| t.1),
            image,
            image_is_full,
            dims: self.dims.filter(|_| !masked),
            files: if masked {
                Vec::new()
            } else {
                self.files.clone()
            },
            files_more: if masked {
                0
            } else {
                self.file_total.saturating_sub(self.files.len())
            },
            file_total: if masked { 0 } else { self.file_total },
            loading: self.detail_pending || self.text_pending || self.image_pending,
            failed: self.failed,
            can_load_text: !masked
                && kind.is_some_and(has_text)
                && self.text.is_none()
                && !self.text_pending,
            can_load_image: is_image
                && self.detail_loaded
                && !image_is_full
                && !self.image_pending
                && larger_exists,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_core::Timestamp;

    const T: Timestamp = Timestamp(1_795_320_000_000);

    fn item(kind: ClipType, masked: bool) -> Item {
        Item {
            id: ClipId::new_v7(Timestamp(T.0 + rand_offset())),
            kind,
            preview: "preview".into(),
            pinned: false,
            masked,
            source: None,
            at: T,
        }
    }

    /// Distinct ids within a test run (UUIDv7 differs by time or random bits anyway).
    fn rand_offset() -> i64 {
        use std::sync::atomic::{AtomicI64, Ordering};
        static N: AtomicI64 = AtomicI64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    }

    const ALL_KINDS: [ClipType; 7] = [
        ClipType::Text,
        ClipType::RichText,
        ClipType::Url,
        ClipType::Code,
        ClipType::Color,
        ClipType::Image,
        ClipType::Files,
    ];
    const ALL_WANTS: [Want; 3] = [Want::Detail, Want::FullText, Want::FullImage];

    fn pic(side: u32) -> Pic<u8> {
        Pic {
            pixels: 1,
            width: side,
            height: side,
        }
    }

    fn thumb_data(w: u32, h: u32) -> DetailData<u8> {
        DetailData {
            masked: false,
            dims: Some((w, h)),
            thumb: Some(pic(256)),
            files: Vec::new(),
            file_total: 0,
        }
    }

    #[test]
    fn masked_clips_never_request_content() {
        for kind in ALL_KINDS {
            let it = item(kind, true);
            for want in ALL_WANTS {
                assert_eq!(request_for(&it, want), None, "{kind:?} {want:?}");
            }
            // Through the model too: opening the pane, selecting, loading text or image.
            let mut m: DetailModel<u8> = DetailModel::default();
            assert!(m.set_open(true, Some(&it)).is_empty());
            assert!(m.sync(Some(&it)).is_empty());
            assert_eq!(m.load_full_text(&it), None);
            assert_eq!(m.load_full_image(&it), None);
            let v = m.view();
            assert!(v.masked);
            assert!(v.text.is_none() && v.image.is_none() && v.files.is_empty());
            assert!(!v.can_load_text && !v.can_load_image);
        }
    }

    #[test]
    fn a_masked_answer_for_an_unmasked_row_drops_everything() {
        let it = item(ClipType::Image, false);
        let mut m: DetailModel<u8> = DetailModel::default();
        assert_eq!(m.set_open(true, Some(&it)).len(), 1);
        let masked = DetailData {
            masked: true,
            ..thumb_data(10, 10)
        };
        assert!(m.on_detail(it.id, Ok(masked), 100));
        let v = m.view();
        assert!(v.masked && v.image.is_none());
        // A late full image does not get cached or shown either.
        assert!(m.on_full_image(it.id, Ok(pic(500)), 1000));
        assert_eq!(m.cache_bytes(), 0);
    }

    #[test]
    fn a_row_turning_masked_drops_loaded_text_files_and_pictures() {
        // Loaded full text, then the same clip arrives masked (sync with a masked row).
        let mut it = item(ClipType::Text, false);
        let mut m: DetailModel<u8> = DetailModel::default();
        m.set_open(true, Some(&it));
        m.load_full_text(&it);
        assert!(m.on_text(it.id, Ok(("secret body".into(), false))));
        assert!(m.view().text.is_some());
        it.masked = true;
        assert!(m.sync(Some(&it)).is_empty());
        let v = m.view();
        assert!(v.masked && v.text.is_none() && !v.can_load_text);

        // Loaded file list and thumbnail, then masked.
        let mut f = item(ClipType::Files, false);
        let mut m: DetailModel<u8> = DetailModel::default();
        m.set_open(true, Some(&f));
        let d = DetailData {
            masked: false,
            dims: Some((5, 5)),
            thumb: Some(pic(10)),
            files: file_entries(&["file:///a/b.txt".to_string()]),
            file_total: 1,
        };
        assert!(m.on_detail(f.id, Ok(d), 400));
        assert_eq!(m.view().files.len(), 1);
        f.masked = true;
        m.sync(Some(&f));
        let v = m.view();
        assert!(v.files.is_empty() && v.dims.is_none() && v.image.is_none());
        assert_eq!(v.files_more, 0);
        assert_eq!(m.cache_bytes(), 0);

        // The worker answering "masked" for an unmasked row clears text and files too.
        let g = item(ClipType::Files, false);
        let mut m: DetailModel<u8> = DetailModel::default();
        m.set_open(true, Some(&g));
        let d = DetailData {
            masked: false,
            dims: None,
            thumb: None,
            files: file_entries(&["file:///x".to_string()]),
            file_total: 1,
        };
        m.on_detail(g.id, Ok(d), 0);
        let masked = DetailData {
            masked: true,
            dims: None,
            thumb: None,
            files: Vec::new(),
            file_total: 0,
        };
        assert!(m.on_detail(g.id, Ok(masked), 0));
        let v = m.view();
        assert!(v.masked && v.files.is_empty());
    }

    #[test]
    fn requests_follow_kind_and_are_not_repeated() {
        let text = item(ClipType::Text, false);
        let img = item(ClipType::Image, false);
        let files = item(ClipType::Files, false);
        assert_eq!(request_for(&text, Want::Detail), None);
        assert_eq!(request_for(&text, Want::FullImage), None);
        assert!(request_for(&text, Want::FullText).is_some());
        assert!(request_for(&img, Want::Detail).is_some());
        assert!(request_for(&img, Want::FullImage).is_some());
        assert_eq!(request_for(&img, Want::FullText), None);
        assert!(request_for(&files, Want::Detail).is_some());
        assert_eq!(request_for(&files, Want::FullText), None);

        let mut m: DetailModel<u8> = DetailModel::default();
        // Closed: nothing loads.
        assert!(m.sync(Some(&img)).is_empty());
        let first = m.set_open(true, Some(&img));
        assert_eq!(
            first,
            vec![DetailRequest {
                id: img.id,
                want: Want::Detail
            }]
        );
        // Same clip again (a refresh): no second request while one is running.
        assert!(m.sync(Some(&img)).is_empty());
        // Text clips need nothing on open; the full text only on demand, and once.
        assert!(m.sync(Some(&text)).is_empty());
        assert!(m.load_full_text(&text).is_some());
        assert_eq!(m.load_full_text(&text), None);
    }

    #[test]
    fn full_text_is_loaded_on_demand_and_capped() {
        let it = item(ClipType::Text, false);
        let mut m: DetailModel<u8> = DetailModel::default();
        // Not open: no load.
        assert_eq!(m.load_full_text(&it), None);
        m.set_open(true, Some(&it));
        assert!(m.view().can_load_text);
        assert_eq!(
            m.load_full_text(&it),
            Some(DetailRequest {
                id: it.id,
                want: Want::FullText
            })
        );
        assert!(m.view().loading && !m.view().can_load_text);
        // The app already cut at the cap; a longer answer is cut again here.
        let long = "é".repeat(TEXT_DISPLAY_CAP);
        assert!(m.on_text(it.id, Ok((long, false))));
        let v = m.view();
        let shown = v.text.unwrap();
        assert!(shown.len() <= TEXT_DISPLAY_CAP);
        assert_eq!(shown.len() % 2, 0);
        assert!(v.text_cut);
        // A short answer is shown whole.
        let it2 = item(ClipType::Code, false);
        m.sync(Some(&it2));
        m.load_full_text(&it2);
        assert!(m.on_text(it2.id, Ok(("fn main() {}".into(), false))));
        let v = m.view();
        assert_eq!(v.text.as_deref(), Some("fn main() {}"));
        assert!(!v.text_cut);
    }

    #[test]
    fn display_cap_is_on_a_char_boundary() {
        let (s, cut) = cap_display("a".repeat(TEXT_DISPLAY_CAP));
        assert_eq!((s.len(), cut), (TEXT_DISPLAY_CAP, false));
        let (s, cut) = cap_display("a".repeat(TEXT_DISPLAY_CAP + 1));
        assert_eq!((s.len(), cut), (TEXT_DISPLAY_CAP, true));
        // Three-byte characters: the cap is not a multiple of 3.
        let (s, cut) = cap_display("€".repeat(40_000));
        assert!(cut && s.len() <= TEXT_DISPLAY_CAP && s.len() % 3 == 0);
        assert!(!s.contains('\u{FFFD}'));
    }

    #[test]
    fn stale_answers_are_dropped() {
        let a = item(ClipType::Text, false);
        let b = item(ClipType::Text, false);
        let mut m: DetailModel<u8> = DetailModel::default();
        m.set_open(true, Some(&a));
        m.load_full_text(&a);
        // The selection moved to b before a's text arrived.
        m.sync(Some(&b));
        assert!(!m.on_text(a.id, Ok(("late".into(), false))));
        assert!(m.view().text.is_none());
        // And b can load its own.
        assert!(m.load_full_text(&b).is_some());
    }

    #[test]
    fn lru_is_bounded_by_bytes_and_evicts_least_recently_used() {
        let mut c: ByteLru<u32, u8> = ByteLru::new(100);
        c.insert(1, 1, 40);
        c.insert(2, 2, 40);
        // Touch 1, so 2 is the oldest.
        assert_eq!(c.get(&1), Some(1));
        c.insert(3, 3, 40);
        assert!(c.bytes() <= 100);
        assert_eq!(c.get(&2), None);
        assert_eq!(c.get(&1), Some(1));
        assert_eq!(c.get(&3), Some(3));
        // Re-inserting a key replaces its weight.
        c.insert(1, 9, 10);
        assert_eq!(c.bytes(), 50);
        // Something heavier than the whole budget is not stored (and never evicts the rest).
        c.insert(4, 4, 101);
        assert_eq!(c.get(&4), None);
        assert_eq!(c.len(), 2);
        c.clear();
        assert_eq!((c.len(), c.bytes()), (0, 0));
    }

    #[test]
    fn model_cache_never_exceeds_32_mib() {
        let mut m: DetailModel<u8> = DetailModel::default();
        assert_eq!(CACHE_BUDGET, 32 * 1024 * 1024);
        let mb = 1024 * 1024;
        let mut last = None;
        for i in 0..40 {
            let it = item(ClipType::Image, false);
            m.set_open(true, Some(&it));
            m.load_full_image(&it);
            // Pretend the thumbnail arrived so the full image can be asked for.
            m.on_detail(it.id, Ok(thumb_data(4000, 3000)), 1000);
            m.load_full_image(&it);
            m.on_full_image(
                it.id,
                Ok(Pic {
                    pixels: u8::try_from(i).unwrap(),
                    width: 1024,
                    height: 768,
                }),
                4 * mb,
            );
            assert!(m.cache_bytes() <= CACHE_BUDGET);
            last = Some(it);
        }
        // 40 x 4 MiB went in; at most 8 stay.
        assert!(m.cache_bytes() <= CACHE_BUDGET);
        assert!(m.cache_bytes() >= 7 * 4 * mb);
        // The newest is still shown from the cache.
        // The selected (newest) clip is the one shown, not some older cached picture.
        assert!(last.is_some());
        assert_eq!(m.view().image, Some(39));
        m.clear();
        assert_eq!(m.cache_bytes(), 0);
        assert!(!m.is_open());
    }

    #[test]
    fn image_flow_thumbnail_first_then_larger_on_demand() {
        let it = item(ClipType::Image, false);
        let mut m: DetailModel<u8> = DetailModel::default();
        let req = m.set_open(true, Some(&it));
        assert_eq!(req[0].want, Want::Detail);
        assert!(m.view().loading && m.view().image.is_none());
        assert!(m.on_detail(it.id, Ok(thumb_data(1600, 900)), 256 * 256 * 4));
        let v = m.view();
        assert_eq!(v.dims, Some((1600, 900)));
        assert!(v.image.is_some() && !v.image_is_full && !v.loading);
        assert!(v.can_load_image);
        let full = m.load_full_image(&it).unwrap();
        assert_eq!(full.want, Want::FullImage);
        assert_eq!(m.load_full_image(&it), None);
        assert!(m.on_full_image(it.id, Ok(pic(1024)), 1024 * 576 * 4));
        let v = m.view();
        assert!(v.image_is_full && !v.can_load_image);
        // A small image whose thumbnail already is the whole picture offers nothing larger.
        let small = item(ClipType::Image, false);
        m.sync(Some(&small));
        m.on_detail(
            small.id,
            Ok(DetailData {
                thumb: Some(pic(100)),
                dims: Some((100, 80)),
                ..thumb_data(0, 0)
            }),
            100 * 100 * 4,
        );
        assert!(!m.view().can_load_image);
    }

    #[test]
    fn failures_show_as_failed_and_forget_drops_the_clip() {
        let it = item(ClipType::Files, false);
        let mut m: DetailModel<u8> = DetailModel::default();
        m.set_open(true, Some(&it));
        assert!(m.on_detail(it.id, Err(()), 0));
        assert!(m.view().failed);
        m.forget(it.id);
        assert!(!m.view().failed);
        assert!(m.is_open());
    }

    #[test]
    fn file_lists_show_names_and_paths_and_count_the_rest() {
        let it = item(ClipType::Files, false);
        let mut m: DetailModel<u8> = DetailModel::default();
        m.set_open(true, Some(&it));
        let uris = vec![
            "file:///C:/Users/me/My%20Docs/a%20b.txt".to_string(),
            "file:///home/me/x.png".to_string(),
        ];
        let d = DetailData {
            masked: false,
            dims: None,
            thumb: None,
            files: file_entries(&uris),
            file_total: 130,
        };
        assert!(m.on_detail(it.id, Ok(d), 0));
        let v = m.view();
        assert_eq!(v.files.len(), 2);
        assert_eq!(v.files[0].name, "a b.txt");
        assert_eq!(v.files[0].path, "C:\\Users\\me\\My Docs\\a b.txt");
        assert_eq!(v.files[1].name, "x.png");
        assert_eq!(v.files[1].path, "/home/me/x.png");
        assert_eq!(v.files_more, 128);
    }

    #[test]
    fn display_paths_handle_unc_odd_input_and_control_characters() {
        assert_eq!(
            display_path("file://server/share/a%20b.txt"),
            "\\\\server\\share\\a b.txt"
        );
        assert_eq!(display_path("FILE:///C:/x"), "C:\\x");
        assert_eq!(display_path("file:///tmp/a%0Ab"), "/tmp/a?b");
        // A bad escape stays as it is; a non-file URI is shown as it is.
        assert_eq!(display_path("file:///tmp/100%zz"), "/tmp/100%zz");
        assert_eq!(
            display_path("https://example.com/a"),
            "https://example.com/a"
        );
        // A folder with a trailing slash still has a name.
        let e = file_entries(&["file:///home/me/dir/".to_string()]);
        assert_eq!(e[0].name, "dir");
        // Never more than the cap.
        let many: Vec<String> = (0..300).map(|i| format!("file:///f{i}")).collect();
        assert_eq!(file_entries(&many).len(), MAX_FILES_SHOWN);
    }

    #[test]
    fn bidi_controls_in_names_are_replaced() {
        // "invoice<RLO>fdp.exe" would read as "invoiceexe.pdf".
        let raw = "file:///tmp/invoice\u{202E}fdp.exe";
        assert_eq!(display_path(raw), "/tmp/invoice\u{FFFD}fdp.exe");
        let escaped = "file:///tmp/invoice%E2%80%AEfdp.exe";
        assert_eq!(display_path(escaped), "/tmp/invoice\u{FFFD}fdp.exe");
        for c in [
            '\u{200E}', '\u{200F}', '\u{202A}', '\u{202D}', '\u{2066}', '\u{2069}',
        ] {
            assert_eq!(display_path(&format!("file:///a{c}b")), "/a\u{FFFD}b");
        }
        let e = file_entries(&[raw.to_string()]);
        assert!(!e[0].name.contains('\u{202E}') && !e[0].path.contains('\u{202E}'));
    }

    #[test]
    fn display_cap_is_32_kib() {
        assert_eq!(TEXT_DISPLAY_CAP, 32 * 1024);
    }

    #[test]
    fn debug_never_prints_paths() {
        let e = FileEntry {
            name: "secret.txt".into(),
            path: "/home/me/secret.txt".into(),
        };
        assert!(!format!("{e:?}").contains("secret"));
    }
}
