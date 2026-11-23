//! The popup's list view-model (Task 204). Pure: it turns filters into search requests, applies
//! responses (dropping stale ones by generation), pages through the recent list with the search
//! cursor and keeps the selection. The Slint host mirrors `visible()` into a `VecModel`.
//!
//! Only previews are held here, never full content (the detail pane's loads are in `detail`). Nothing is
//! logged; `Debug` never prints a preview, a query or a source name (GC-06).

use super::keys::Move;
use darkclip_app::{ClipSummaryView, Page};
use darkclip_core::{ClipId, ClipType, CollectionId, Timestamp};
use darkclip_storage::{Cursor, SearchQuery};

/// Rows per search request.
pub const PAGE_SIZE: u16 = 50;
/// A refresh reloads as many rows as were loaded, up to this many (scrolling loads the rest).
pub const REFRESH_MAX: usize = 1000;
/// Rows one request may ask for (`App` caps the limit at 200).
const MAX_REQUEST: usize = 200;
/// Load the next page when the last shown row is this close to the end.
pub const LOAD_MORE_MARGIN: usize = 10;
const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeFilter {
    #[default]
    Any,
    /// Since local midnight.
    Today,
    /// The last 7 × 24 hours.
    Week,
    /// The last 30 × 24 hours (the history window, Task 211).
    Month,
}

/// What the user asked for. `source` is applied to the loaded rows (the search API has no
/// source filter).
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Filters {
    pub text: String,
    pub kinds: Vec<ClipType>,
    pub pinned_only: bool,
    pub time: TimeFilter,
    pub source: Option<String>,
    /// Only members of this collection (Favorites is the system collection; Task 403).
    pub collection: Option<CollectionId>,
    /// Only clips with this tag (a normalised tag name).
    pub tag: Option<String>,
}

/// Never prints the query text or the source name (GC-06).
impl std::fmt::Debug for Filters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Filters")
            .field("text_len", &self.text.len())
            .field("kinds", &self.kinds)
            .field("pinned_only", &self.pinned_only)
            .field("time", &self.time)
            .field("source", &self.source.is_some())
            .field("collection", &self.collection)
            .field("tag", &self.tag.is_some())
            .finish()
    }
}

impl Filters {
    /// No text and no filter: an empty result means the history is empty.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
            && self.kinds.is_empty()
            && !self.pinned_only
            && self.time == TimeFilter::Any
            && self.source.is_none()
            && self.collection.is_none()
            && self.tag.is_none()
    }
}

/// One search to run on the worker thread.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    pub gen: u64,
    /// A next page of the same search (appended), not a new result list.
    pub append: bool,
    pub text: Option<String>,
    pub kinds: Vec<ClipType>,
    pub pinned_only: bool,
    pub since: Option<Timestamp>,
    pub cursor: Option<String>,
    /// Rows asked for (a refresh asks for as many as were loaded, in requests of ≤ 200).
    pub limit: u16,
    pub collection: Option<CollectionId>,
    pub tag: Option<String>,
}

/// Never prints the query text (GC-06).
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("gen", &self.gen)
            .field("append", &self.append)
            .field("has_text", &self.text.is_some())
            .field("kinds", &self.kinds)
            .field("pinned_only", &self.pinned_only)
            .field("since", &self.since)
            .field("has_cursor", &self.cursor.is_some())
            .field("collection", &self.collection)
            .field("has_tag", &self.tag.is_some())
            .finish()
    }
}

impl Request {
    /// The storage query. `include_masked` is decided by `App` for the caller (UI: true).
    pub fn to_query(&self) -> SearchQuery {
        SearchQuery {
            text: self.text.clone(),
            kinds: self.kinds.clone(),
            pinned_only: self.pinned_only,
            since: self.since,
            include_masked: false,
            limit: self.limit,
            cursor: self.cursor.as_deref().and_then(Cursor::decode),
            collection: self.collection,
            tag: self.tag.clone(),
        }
    }
}

/// One row: preview only.
#[derive(Clone, PartialEq, Eq)]
pub struct Item {
    pub id: ClipId,
    pub kind: ClipType,
    pub preview: String,
    pub pinned: bool,
    pub masked: bool,
    pub source: Option<String>,
    /// When the clip was last copied (for the relative time on the row).
    pub at: Timestamp,
    /// Member of Favorites (Task 403).
    pub favorite: bool,
    /// Tag names, sorted. Names are user metadata, never printed in `Debug`.
    pub tags: Vec<String>,
}

/// Never prints the preview or the source name (GC-06).
impl std::fmt::Debug for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Item")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("pinned", &self.pinned)
            .field("masked", &self.masked)
            .field("favorite", &self.favorite)
            .finish_non_exhaustive()
    }
}

impl From<ClipSummaryView> for Item {
    fn from(v: ClipSummaryView) -> Self {
        Item {
            id: v.id,
            kind: v.kind,
            preview: v.preview,
            pinned: v.pinned,
            masked: v.masked,
            source: v.source_app,
            at: v.last_copied_at,
            favorite: v.favorite,
            tags: v.tags,
        }
    }
}

/// What the popup shows instead of (or as) the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Rows,
    /// A fresh search has been running for longer than the loading threshold.
    Loading,
    /// Nothing stored yet: onboarding hint.
    EmptyHistory,
    NoResults,
    /// A search failed; the code is an `ApiError::code()` name (content-free).
    Error(&'static str),
}

/// What `apply` did with a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// Stale or unexpected: nothing changed.
    Ignored,
    /// The visible rows were replaced.
    Replaced,
    /// Rows were added at the end; `from` is the first new visible index.
    Appended { from: usize },
    /// A refresh needs another page before it can replace the rows: run this request.
    More(Request),
}

/// `rows` as a request limit: at least 1, at most what one request may ask for.
fn request_limit(rows: usize) -> u16 {
    u16::try_from(rows.clamp(1, MAX_REQUEST)).unwrap_or(PAGE_SIZE)
}

/// Start of the time window for a filter. `utc_offset_min` is the local offset (east positive).
pub fn since_for(time: TimeFilter, now: Timestamp, utc_offset_min: i32) -> Option<Timestamp> {
    match time {
        TimeFilter::Any => None,
        TimeFilter::Week => Some(Timestamp(now.0.saturating_sub(7 * DAY_MS))),
        TimeFilter::Month => Some(Timestamp(now.0.saturating_sub(30 * DAY_MS))),
        TimeFilter::Today => {
            let offset = i64::from(utc_offset_min) * 60_000;
            let local = now.0.saturating_add(offset);
            let midnight_local = local - local.rem_euclid(DAY_MS);
            Some(Timestamp(midnight_local.saturating_sub(offset)))
        }
    }
}

#[derive(Default)]
pub struct ListModel {
    gen: u64,
    filters: Filters,
    items: Vec<Item>,
    next: Option<String>,
    /// A fresh (non-append) search is running.
    waiting: bool,
    more_in_flight: bool,
    error: Option<&'static str>,
    selected: usize,
    /// Rows removed from view (a delete waiting for its undo window).
    hidden: Vec<ClipId>,
    /// The clip to select again when the running refresh answers.
    keep: Option<ClipId>,
    /// The running search is a refresh (keep the selection), not a new search.
    refreshing: bool,
    /// The time window of the current search (kept for its next pages).
    since: Option<Timestamp>,
    /// Rows a running refresh reloads before it replaces the ones on screen.
    target: usize,
    /// The refreshed rows collected so far, and their cursor (the screen keeps the old rows
    /// until the refresh is complete).
    staged: Vec<Item>,
    staged_next: Option<String>,
}

impl ListModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn filters(&self) -> &Filters {
        &self.filters
    }

    fn trimmed_text(&self) -> Option<String> {
        let text = self.filters.text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    fn request(&mut self, now: Timestamp, utc_offset_min: i32) -> Request {
        self.gen = self.gen.wrapping_add(1);
        self.waiting = true;
        self.more_in_flight = false;
        self.since = since_for(self.filters.time, now, utc_offset_min);
        Request {
            gen: self.gen,
            append: false,
            text: self.trimmed_text(),
            kinds: self.filters.kinds.clone(),
            pinned_only: self.filters.pinned_only,
            since: self.since,
            cursor: None,
            limit: PAGE_SIZE,
            collection: self.filters.collection,
            tag: self.filters.tag.clone(),
        }
    }

    /// A new search; the selection goes back to the first row when it arrives.
    pub fn search(&mut self, f: Filters, now: Timestamp, utc_offset_min: i32) -> Request {
        self.filters = f;
        self.refreshing = false;
        self.keep = None;
        self.staged.clear();
        self.request(now, utc_offset_min)
    }

    /// The same search again (the data changed); the selected clip stays selected.
    pub fn refresh(&mut self, now: Timestamp, utc_offset_min: i32) -> Request {
        // A refresh while a new search is still running stays a new search.
        let new_search_running = self.waiting && !self.refreshing;
        if !new_search_running {
            self.refreshing = true;
            self.keep = self.selected_item().map(|i| i.id);
            self.target = self.items.len().clamp(usize::from(PAGE_SIZE), REFRESH_MAX);
        }
        self.staged.clear();
        self.staged_next = None;
        let mut r = self.request(now, utc_offset_min);
        if self.refreshing {
            r.limit = request_limit(self.target);
        }
        r
    }

    /// The next page, unless there is none or one is already loading.
    pub fn load_more(&mut self) -> Option<Request> {
        if self.waiting || self.more_in_flight {
            return None;
        }
        let cursor = self.next.clone()?;
        self.more_in_flight = true;
        // Same filters and time window as the first page.
        Some(Request {
            gen: self.gen,
            append: true,
            text: self.trimmed_text(),
            kinds: self.filters.kinds.clone(),
            pinned_only: self.filters.pinned_only,
            since: self.since,
            cursor: Some(cursor),
            limit: PAGE_SIZE,
            collection: self.filters.collection,
            tag: self.filters.tag.clone(),
        })
    }

    fn push_new(&mut self, items: Vec<ClipSummaryView>) {
        for v in items {
            if !self.items.iter().any(|i| i.id == v.id) {
                self.items.push(Item::from(v));
            }
        }
    }

    /// Applies a response for `gen`. Responses of older generations are dropped.
    pub fn apply(
        &mut self,
        gen: u64,
        append: bool,
        r: Result<Page<ClipSummaryView>, &'static str>,
    ) -> Applied {
        if gen != self.gen {
            return Applied::Ignored;
        }
        if self.refreshing && self.waiting {
            return self.apply_refresh(append, r);
        }
        if append {
            if !self.more_in_flight || self.waiting {
                return Applied::Ignored;
            }
            self.more_in_flight = false;
            let Ok(page) = r else {
                // Keep what is shown; stop paging.
                self.next = None;
                return Applied::Ignored;
            };
            let from = self.visible().len();
            self.push_new(page.items);
            self.next = page.next_cursor;
            return Applied::Appended { from };
        }
        if !self.waiting {
            return Applied::Ignored;
        }
        self.waiting = false;
        self.items.clear();
        match r {
            Ok(page) => {
                self.error = None;
                self.push_new(page.items);
                self.next = page.next_cursor;
            }
            Err(code) => {
                self.error = Some(code);
                self.next = None;
            }
        }
        let keep = self.keep.take();
        if self.refreshing {
            if let Some(i) = keep.and_then(|id| self.visible().iter().position(|i| i.id == id)) {
                self.selected = i;
            }
        } else {
            self.selected = 0;
        }
        self.refreshing = false;
        self.clamp();
        Applied::Replaced
    }

    /// A page of a running refresh: collect it, ask for the next one, or swap the rows in.
    fn apply_refresh(
        &mut self,
        append: bool,
        r: Result<Page<ClipSummaryView>, &'static str>,
    ) -> Applied {
        if append != self.more_in_flight {
            return Applied::Ignored;
        }
        self.more_in_flight = false;
        let Ok(page) = r else {
            // Keep the rows (and their cursor) on screen; the next change refreshes again.
            self.waiting = false;
            self.refreshing = false;
            self.keep = None;
            self.staged.clear();
            self.staged_next = None;
            return Applied::Ignored;
        };
        for v in page.items {
            if !self.staged.iter().any(|i| i.id == v.id) {
                self.staged.push(Item::from(v));
            }
        }
        self.staged_next = page.next_cursor;
        if self.staged.len() < self.target {
            if let Some(cursor) = self.staged_next.clone() {
                self.more_in_flight = true;
                return Applied::More(Request {
                    gen: self.gen,
                    append: true,
                    text: self.trimmed_text(),
                    kinds: self.filters.kinds.clone(),
                    pinned_only: self.filters.pinned_only,
                    since: self.since,
                    cursor: Some(cursor),
                    limit: request_limit(self.target - self.staged.len()),
                    collection: self.filters.collection,
                    tag: self.filters.tag.clone(),
                });
            }
        }
        self.items = std::mem::take(&mut self.staged);
        self.next = self.staged_next.take();
        self.error = None;
        self.waiting = false;
        self.refreshing = false;
        if let Some(i) = self
            .keep
            .take()
            .and_then(|id| self.visible().iter().position(|i| i.id == id))
        {
            self.selected = i;
        }
        self.clamp();
        Applied::Replaced
    }

    fn clamp(&mut self) {
        let n = self.visible().len();
        self.selected = self.selected.min(n.saturating_sub(1));
    }

    /// True while the fresh search `gen` has not answered yet.
    pub fn is_waiting(&self, gen: u64) -> bool {
        self.waiting && gen == self.gen
    }

    fn shows(&self, i: &Item) -> bool {
        let source_ok = match &self.filters.source {
            Some(s) => i.source.as_ref() == Some(s),
            None => true,
        };
        source_ok && !self.hidden.contains(&i.id)
    }

    /// The rows on screen: loaded, not hidden, matching the source filter.
    pub fn visible(&self) -> Vec<&Item> {
        self.items.iter().filter(|i| self.shows(i)).collect()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_item(&self) -> Option<&Item> {
        self.nth(self.selected)
    }

    pub fn nth(&self, n: usize) -> Option<&Item> {
        self.items.iter().filter(|i| self.shows(i)).nth(n)
    }

    pub fn select(&mut self, i: usize) {
        self.selected = i;
        self.clamp();
    }

    pub fn move_by(&mut self, m: Move, page_rows: usize) {
        let page = page_rows.max(1);
        let s = self.selected;
        self.selected = match m {
            Move::Up => s.saturating_sub(1),
            Move::Down => s.saturating_add(1),
            Move::PageUp => s.saturating_sub(page),
            Move::PageDown => s.saturating_add(page),
            Move::Home => 0,
            Move::End => usize::MAX,
        };
        self.clamp();
    }

    /// True when the row `last_shown` is near the end and another page exists.
    pub fn near_end(&self, last_shown: usize) -> bool {
        self.next.is_some()
            && !self.waiting
            && !self.more_in_flight
            && last_shown.saturating_add(LOAD_MORE_MARGIN) >= self.visible().len()
    }

    /// Hides a row (pending delete); returns its visible index.
    pub fn hide(&mut self, id: ClipId) -> Option<usize> {
        let at = self.visible().iter().position(|i| i.id == id)?;
        self.hidden.push(id);
        self.clamp();
        Some(at)
    }

    /// Shows a hidden row again (undo); returns its visible index.
    pub fn unhide(&mut self, id: ClipId) -> Option<usize> {
        let pos = self.hidden.iter().position(|h| *h == id)?;
        self.hidden.remove(pos);
        self.visible().iter().position(|i| i.id == id)
    }

    /// Forgets every row and filter (the history window closed). The generation moves on, so
    /// an answer still in flight is ignored when it arrives.
    pub fn clear(&mut self) {
        let gen = self.gen.wrapping_add(1);
        *self = Self {
            gen,
            ..Self::default()
        };
    }

    /// Distinct source names of the loaded rows, first seen first, at most `max`; the active
    /// source filter is always included.
    pub fn sources(&self, max: usize) -> Vec<String> {
        let max = max.max(1);
        let mut out: Vec<String> = Vec::new();
        if let Some(s) = &self.filters.source {
            out.push(s.clone());
        }
        for s in self.items.iter().filter_map(|i| i.source.as_ref()) {
            if out.len() >= max {
                break;
            }
            if !out.contains(s) {
                out.push(s.clone());
            }
        }
        out
    }

    /// `slow`: the fresh search has run longer than the loading threshold.
    pub fn view(&self, slow: bool) -> View {
        // A refresh keeps showing the current rows however long it takes.
        if self.waiting && slow && !self.refreshing {
            return View::Loading;
        }
        if let Some(code) = self.error.filter(|_| !self.waiting) {
            return View::Error(code);
        }
        if self.waiting || !self.visible().is_empty() {
            return View::Rows;
        }
        if self.filters.is_empty() && self.items.is_empty() {
            View::EmptyHistory
        } else {
            View::NoResults
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = Timestamp(1_795_320_000_000);

    fn view_item(n: u32, pinned: bool, source: Option<&str>) -> ClipSummaryView {
        ClipSummaryView {
            id: ClipId::new_v7(Timestamp(NOW.0 + i64::from(n))),
            kind: ClipType::Text,
            preview: format!("clip {n}"),
            pinned,
            last_copied_at: NOW,
            masked: false,
            source_app: source.map(str::to_string),
            favorite: false,
            tags: Vec::new(),
        }
    }

    fn page(items: Vec<ClipSummaryView>, next: Option<&str>) -> Page<ClipSummaryView> {
        Page {
            items,
            limit: PAGE_SIZE,
            next_cursor: next.map(str::to_string),
        }
    }

    fn items(range: std::ops::Range<u32>) -> Vec<ClipSummaryView> {
        range.map(|n| view_item(n, false, None)).collect()
    }

    fn previews(m: &ListModel) -> Vec<String> {
        m.visible().iter().map(|i| i.preview.clone()).collect()
    }

    fn text(s: &str) -> Filters {
        Filters {
            text: s.into(),
            ..Filters::default()
        }
    }

    /// A real cursor string, as `App` would return it.
    fn cursor() -> String {
        Cursor {
            last_copied_at: NOW,
            id: ClipId::new_v7(NOW),
        }
        .encode()
    }

    #[test]
    fn collection_and_tag_filters_reach_the_query_on_every_page() {
        let col = darkclip_core::CollectionId::new_v7(NOW);
        let f = Filters {
            collection: Some(col),
            tag: Some("todo".into()),
            ..Filters::default()
        };
        assert!(!f.is_empty(), "a collection or tag filter is a filter");
        let mut m = ListModel::new();
        let r = m.search(f, NOW, 0);
        let q = r.to_query();
        assert_eq!(q.collection, Some(col));
        assert_eq!(q.tag.as_deref(), Some("todo"));
        let gen = r.gen;
        let cur = cursor();
        m.apply(gen, false, Ok(page(items(0..3), Some(&cur))));
        let next = m.load_more().unwrap().to_query();
        assert_eq!(next.collection, Some(col));
        assert_eq!(next.tag.as_deref(), Some("todo"));
        // The Debug output never prints the tag name.
        let dbg = format!("{:?}", m.filters());
        assert!(!dbg.contains("todo"));
    }

    #[test]
    fn rows_carry_the_favorite_flag_and_the_tags() {
        let mut v = view_item(1, false, None);
        v.favorite = true;
        v.tags = vec!["todo".to_string(), "urgent".to_string()];
        let item = Item::from(v);
        assert!(item.favorite);
        assert_eq!(item.tags, ["todo", "urgent"]);
        assert!(!format!("{item:?}").contains("urgent"));
    }

    #[test]
    fn empty_query_lists_recent_clips() {
        let mut m = ListModel::new();
        let r = m.search(text("   "), NOW, 0);
        assert!(!r.append);
        let q = r.to_query();
        assert_eq!(q.text, None);
        assert_eq!(q.limit, PAGE_SIZE);
        assert_eq!(q.cursor, None);
        assert!(!q.pinned_only);
        assert!(q.kinds.is_empty());
        assert_eq!(q.since, None);
    }

    #[test]
    fn query_and_filters_become_a_request() {
        let mut m = ListModel::new();
        let f = Filters {
            text: "  kubectl get ".into(),
            kinds: vec![ClipType::Code, ClipType::Url],
            pinned_only: true,
            time: TimeFilter::Week,
            source: Some("editor.exe".into()),
            ..Filters::default()
        };
        let q = m.search(f, NOW, 0).to_query();
        assert_eq!(q.text.as_deref(), Some("kubectl get"));
        assert_eq!(q.kinds, vec![ClipType::Code, ClipType::Url]);
        assert!(q.pinned_only);
        assert_eq!(q.since, Some(Timestamp(NOW.0 - 7 * DAY_MS)));
        assert_eq!(q.limit, PAGE_SIZE);
    }

    #[test]
    fn today_starts_at_local_midnight() {
        // 2026-11-23T10:30:00Z
        let now = Timestamp(1_795_429_800_000);
        let utc_midnight = Timestamp(1_795_392_000_000);
        assert_eq!(since_for(TimeFilter::Today, now, 0), Some(utc_midnight));
        // UTC+7 (Vietnam): 17:30 local, so local midnight is 17:00Z the day before.
        assert_eq!(
            since_for(TimeFilter::Today, now, 7 * 60),
            Some(Timestamp(utc_midnight.0 - 7 * 3_600_000))
        );
        // UTC-5: 05:30 local, local midnight is 05:00Z the same day.
        assert_eq!(
            since_for(TimeFilter::Today, now, -5 * 60),
            Some(Timestamp(utc_midnight.0 + 5 * 3_600_000))
        );
        assert_eq!(since_for(TimeFilter::Any, now, 0), None);
        assert_eq!(
            since_for(TimeFilter::Week, now, 420),
            Some(Timestamp(now.0 - 7 * DAY_MS))
        );
    }

    #[test]
    fn a_month_is_thirty_days() {
        let now = Timestamp(1_795_320_000_000);
        assert_eq!(
            since_for(TimeFilter::Month, now, 420),
            Some(Timestamp(now.0 - 30 * DAY_MS))
        );
    }

    #[test]
    fn clear_drops_the_rows_and_ignores_answers_still_in_flight() {
        let mut m = ListModel::new();
        let r = m.search(Filters::default(), NOW, 0);
        assert!(matches!(
            m.apply(r.gen, false, Ok(page(items(0..3), Some("c")))),
            Applied::Replaced
        ));
        m.select(2);
        let hidden = m.visible()[0].id;
        m.hide(hidden);
        // An answer for a search that is still running when the window closes.
        let running = m.search(text("secret"), NOW, 0);
        m.clear();
        assert!(m.visible().is_empty());
        assert_eq!(m.selected(), 0);
        assert!(m.sources(4).is_empty());
        assert!(m.filters().is_empty());
        assert!(matches!(
            m.apply(running.gen, false, Ok(page(items(0..3), None))),
            Applied::Ignored
        ));
        assert!(m.visible().is_empty());
    }

    #[test]
    fn every_search_gets_a_newer_generation() {
        let mut m = ListModel::new();
        let a = m.search(text("a"), NOW, 0);
        let b = m.search(text("ab"), NOW, 0);
        let c = m.refresh(NOW, 0);
        assert!(a.gen < b.gen && b.gen < c.gen);
        assert!(m.is_waiting(c.gen));
        assert!(!m.is_waiting(b.gen));
    }

    #[test]
    fn stale_responses_are_discarded() {
        let mut m = ListModel::new();
        let old = m.search(text("ku"), NOW, 0);
        let new = m.search(text("kub"), NOW, 0);
        // The newer answer arrives first, then the older one.
        assert_eq!(
            m.apply(new.gen, false, Ok(page(items(0..2), None))),
            Applied::Replaced
        );
        assert_eq!(
            m.apply(old.gen, false, Ok(page(items(10..15), None))),
            Applied::Ignored
        );
        assert_eq!(previews(&m), ["clip 0", "clip 1"]);
        assert!(!m.is_waiting(new.gen));
        // A stale error is dropped too.
        assert_eq!(
            m.apply(old.gen, false, Err("STORAGE_ERROR")),
            Applied::Ignored
        );
        assert_eq!(m.view(false), View::Rows);
    }

    #[test]
    fn pages_load_near_the_end_and_append() {
        let mut m = ListModel::new();
        let r = m.search(Filters::default(), NOW, 0);
        let cur = cursor();
        m.apply(r.gen, false, Ok(page(items(0..50), Some(&cur))));
        assert!(!m.near_end(10));
        assert!(m.near_end(40));
        let more = m.load_more().expect("a next page");
        assert!(more.append);
        assert_eq!(more.gen, r.gen);
        assert_eq!(more.since, r.since);
        assert_eq!(more.cursor.as_deref(), Some(cur.as_str()));
        assert_eq!(
            more.to_query().cursor.map(|c| c.encode()),
            Some(cur.clone())
        );
        // Only one page at a time.
        assert!(m.load_more().is_none());
        assert!(!m.near_end(49));
        assert_eq!(
            m.apply(more.gen, true, Ok(page(items(50..80), None))),
            Applied::Appended { from: 50 }
        );
        assert_eq!(m.visible().len(), 80);
        // The last page has no cursor: nothing more to load.
        assert!(!m.near_end(79));
        assert!(m.load_more().is_none());
    }

    #[test]
    fn a_page_of_an_old_search_is_dropped() {
        let mut m = ListModel::new();
        let r = m.search(Filters::default(), NOW, 0);
        let cur = cursor();
        m.apply(r.gen, false, Ok(page(items(0..50), Some(&cur))));
        let more = m.load_more().unwrap();
        let fresh = m.search(text("x"), NOW, 0);
        assert_eq!(
            m.apply(more.gen, true, Ok(page(items(50..60), None))),
            Applied::Ignored
        );
        m.apply(fresh.gen, false, Ok(page(items(0..1), None)));
        assert_eq!(m.visible().len(), 1);
        // An append nobody asked for is ignored.
        assert_eq!(
            m.apply(fresh.gen, true, Ok(page(items(5..6), None))),
            Applied::Ignored
        );
    }

    #[test]
    fn a_failed_page_stops_paging_without_losing_rows() {
        let mut m = ListModel::new();
        let r = m.search(Filters::default(), NOW, 0);
        let cur = cursor();
        m.apply(r.gen, false, Ok(page(items(0..50), Some(&cur))));
        let more = m.load_more().unwrap();
        assert_eq!(
            m.apply(more.gen, true, Err("STORAGE_ERROR")),
            Applied::Ignored
        );
        assert_eq!(m.visible().len(), 50);
        assert!(m.load_more().is_none());
        assert_eq!(m.view(false), View::Rows);
    }

    #[test]
    fn duplicate_rows_in_a_page_are_skipped() {
        let mut m = ListModel::new();
        let r = m.search(Filters::default(), NOW, 0);
        let cur = cursor();
        let first = items(0..3);
        let dup = first[2].clone();
        m.apply(r.gen, false, Ok(page(first, Some(&cur))));
        let more = m.load_more().unwrap();
        let mut next = vec![dup];
        next.extend(items(3..5));
        assert_eq!(
            m.apply(more.gen, true, Ok(page(next, None))),
            Applied::Appended { from: 3 }
        );
        assert_eq!(
            previews(&m),
            ["clip 0", "clip 1", "clip 2", "clip 3", "clip 4"]
        );
    }

    #[test]
    fn selection_moves_and_clamps() {
        let mut m = ListModel::new();
        let r = m.search(Filters::default(), NOW, 0);
        m.apply(r.gen, false, Ok(page(items(0..30), None)));
        assert_eq!(m.selected(), 0);
        m.move_by(Move::Up, 8);
        assert_eq!(m.selected(), 0);
        m.move_by(Move::Down, 8);
        assert_eq!(m.selected(), 1);
        m.move_by(Move::PageDown, 8);
        assert_eq!(m.selected(), 9);
        m.move_by(Move::End, 8);
        assert_eq!(m.selected(), 29);
        m.move_by(Move::Down, 8);
        assert_eq!(m.selected(), 29);
        m.move_by(Move::PageDown, 8);
        assert_eq!(m.selected(), 29);
        m.move_by(Move::PageUp, 8);
        assert_eq!(m.selected(), 21);
        m.move_by(Move::Home, 8);
        assert_eq!(m.selected(), 0);
        m.move_by(Move::PageUp, 8);
        assert_eq!(m.selected(), 0);
        m.select(5);
        assert_eq!(m.selected_item().unwrap().preview, "clip 5");
        m.select(500);
        assert_eq!(m.selected(), 29);
        assert_eq!(m.nth(2).unwrap().preview, "clip 2");
        assert!(m.nth(30).is_none());
    }

    #[test]
    fn empty_list_has_no_selection() {
        let mut m = ListModel::new();
        m.move_by(Move::Down, 8);
        m.move_by(Move::End, 8);
        assert_eq!(m.selected(), 0);
        assert!(m.selected_item().is_none());
    }

    #[test]
    fn a_new_search_selects_the_first_row_but_a_refresh_keeps_the_clip() {
        let mut m = ListModel::new();
        let r = m.search(Filters::default(), NOW, 0);
        let rows = items(0..5);
        m.apply(r.gen, false, Ok(page(rows.clone(), None)));
        m.select(3);
        // A refresh (e.g. after pinning) moves the clip to the top: the selection follows it.
        let again = m.refresh(NOW, 0);
        let mut moved = vec![rows[3].clone()];
        moved.extend([
            rows[0].clone(),
            rows[1].clone(),
            rows[2].clone(),
            rows[4].clone(),
        ]);
        m.apply(again.gen, false, Ok(page(moved, None)));
        assert_eq!(m.selected(), 0);
        assert_eq!(m.selected_item().unwrap().preview, "clip 3");
        m.select(2);
        // The selected clip is gone after a refresh: the index stays, clamped.
        let again = m.refresh(NOW, 0);
        m.apply(again.gen, false, Ok(page(items(0..2), None)));
        assert_eq!(m.selected(), 1);
        // A new search always starts at the top.
        m.select(1);
        let fresh = m.search(text("clip"), NOW, 0);
        m.apply(fresh.gen, false, Ok(page(items(0..5), None)));
        assert_eq!(m.selected(), 0);
    }

    #[test]
    fn states_follow_the_responses() {
        let mut m = ListModel::new();
        let r = m.search(Filters::default(), NOW, 0);
        // Waiting: nothing until the loading threshold passes.
        assert_eq!(m.view(false), View::Rows);
        assert_eq!(m.view(true), View::Loading);
        m.apply(r.gen, false, Ok(page(vec![], None)));
        assert_eq!(m.view(true), View::EmptyHistory);
        let r = m.search(text("nothing"), NOW, 0);
        m.apply(r.gen, false, Ok(page(vec![], None)));
        assert_eq!(m.view(false), View::NoResults);
        let r = m.search(
            Filters {
                pinned_only: true,
                ..Filters::default()
            },
            NOW,
            0,
        );
        m.apply(r.gen, false, Ok(page(vec![], None)));
        assert_eq!(m.view(false), View::NoResults);
        let r = m.search(text("x"), NOW, 0);
        m.apply(r.gen, false, Err("STORAGE_ERROR"));
        assert_eq!(m.view(false), View::Error("STORAGE_ERROR"));
        assert!(m.visible().is_empty());
        let r = m.search(text("x"), NOW, 0);
        m.apply(r.gen, false, Ok(page(items(0..1), None)));
        assert_eq!(m.view(false), View::Rows);
    }

    /// A list loaded to `pages` × `per_page` rows through the cursor.
    fn deep_list(pages: u32, per_page: u32) -> ListModel {
        let mut m = ListModel::new();
        let r = m.search(Filters::default(), NOW, 0);
        let cur = cursor();
        m.apply(r.gen, false, Ok(page(items(0..per_page), Some(&cur))));
        for p in 1..pages {
            let more = m.load_more().unwrap();
            m.apply(
                more.gen,
                true,
                Ok(page(items(p * per_page..(p + 1) * per_page), Some(&cur))),
            );
        }
        m
    }

    #[test]
    fn a_refresh_keeps_a_deep_list_and_the_selected_clip() {
        let mut m = deep_list(3, 50);
        m.select(120);
        let r = m.refresh(NOW, 0);
        assert!(!r.append);
        assert_eq!(r.limit, 150);
        assert_eq!(r.to_query().limit, 150);
        // The old rows stay on screen while the refresh runs, even when it is slow.
        assert_eq!(m.visible().len(), 150);
        assert_eq!(m.view(true), View::Rows);
        let cur = cursor();
        assert_eq!(
            m.apply(r.gen, false, Ok(page(items(0..150), Some(&cur)))),
            Applied::Replaced
        );
        assert_eq!(m.visible().len(), 150);
        assert_eq!(m.selected_item().unwrap().preview, "clip 120");
        // Paging goes on from there.
        assert!(m.load_more().is_some());
    }

    #[test]
    fn a_refresh_of_more_than_one_request_comes_in_staged_pages() {
        let mut m = deep_list(5, 50);
        m.select(240);
        let r = m.refresh(NOW, 0);
        assert_eq!(r.limit, 200);
        let cur = cursor();
        let Applied::More(next) = m.apply(r.gen, false, Ok(page(items(0..200), Some(&cur)))) else {
            panic!("a second refresh page is needed");
        };
        assert!(next.append);
        assert_eq!(next.gen, r.gen);
        assert_eq!(next.limit, 50);
        assert_eq!(next.cursor.as_deref(), Some(cur.as_str()));
        // Still the old rows until the refresh is complete.
        assert_eq!(m.visible().len(), 250);
        assert!(m.load_more().is_none());
        assert_eq!(
            m.apply(next.gen, true, Ok(page(items(200..250), None))),
            Applied::Replaced
        );
        assert_eq!(m.visible().len(), 250);
        assert_eq!(m.selected_item().unwrap().preview, "clip 240");
    }

    #[test]
    fn a_refresh_reloads_at_most_a_thousand_rows() {
        let mut m = deep_list(6, 200);
        assert_eq!(m.visible().len(), 1200);
        let r = m.refresh(NOW, 0);
        assert_eq!(r.limit, 200);
        let cur = cursor();
        let mut answer = m.apply(r.gen, false, Ok(page(items(0..200), Some(&cur))));
        let mut requests = 1;
        let mut at = 200;
        while let Applied::More(next) = answer {
            requests += 1;
            answer = m.apply(
                next.gen,
                true,
                Ok(page(items(at..at + next.limit as u32), Some(&cur))),
            );
            at += u32::from(next.limit);
        }
        assert_eq!(answer, Applied::Replaced);
        assert_eq!(requests, 5);
        assert_eq!(m.visible().len(), 1000);
        // The rest loads by scrolling, as before.
        assert!(m.load_more().is_some());
    }

    #[test]
    fn a_failed_refresh_keeps_the_rows_on_screen() {
        let mut m = deep_list(2, 50);
        m.select(70);
        let r = m.refresh(NOW, 0);
        assert_eq!(
            m.apply(r.gen, false, Err("STORAGE_ERROR")),
            Applied::Ignored
        );
        assert_eq!(m.visible().len(), 100);
        assert_eq!(m.selected_item().unwrap().preview, "clip 70");
        assert_eq!(m.view(false), View::Rows);
        assert!(!m.is_waiting(r.gen));
        assert!(m.load_more().is_some());
    }

    #[test]
    fn a_new_search_during_a_refresh_wins() {
        let mut m = deep_list(5, 50);
        let r = m.refresh(NOW, 0);
        let fresh = m.search(text("x"), NOW, 0);
        assert_eq!(fresh.limit, PAGE_SIZE);
        let cur = cursor();
        assert_eq!(
            m.apply(r.gen, false, Ok(page(items(0..200), Some(&cur)))),
            Applied::Ignored
        );
        m.apply(fresh.gen, false, Ok(page(items(0..3), None)));
        assert_eq!(m.visible().len(), 3);
    }

    #[test]
    fn hidden_rows_leave_the_view_until_undone() {
        let mut m = ListModel::new();
        let r = m.search(Filters::default(), NOW, 0);
        let rows = items(0..4);
        let id = rows[2].id;
        m.apply(r.gen, false, Ok(page(rows.clone(), None)));
        m.select(3);
        assert_eq!(m.hide(id), Some(2));
        assert_eq!(previews(&m), ["clip 0", "clip 1", "clip 3"]);
        // The selection stays on a valid row.
        assert_eq!(m.selected(), 2);
        assert_eq!(m.hide(id), None);
        // A late response that still has the clip does not bring it back.
        let again = m.refresh(NOW, 0);
        m.apply(again.gen, false, Ok(page(rows.clone(), None)));
        assert_eq!(m.visible().len(), 3);
        assert_eq!(m.unhide(id), Some(2));
        assert_eq!(m.visible().len(), 4);
        assert_eq!(m.unhide(id), None);
    }

    #[test]
    fn source_filter_applies_to_loaded_rows() {
        let mut m = ListModel::new();
        let r = m.search(Filters::default(), NOW, 0);
        let rows = vec![
            view_item(0, false, Some("editor.exe")),
            view_item(1, false, Some("Browser")),
            view_item(2, false, None),
            view_item(3, false, Some("editor.exe")),
        ];
        m.apply(r.gen, false, Ok(page(rows.clone(), None)));
        assert_eq!(m.sources(5), ["editor.exe", "Browser"]);
        assert_eq!(m.sources(1), ["editor.exe"]);
        let f = Filters {
            source: Some("editor.exe".into()),
            ..Filters::default()
        };
        let r = m.search(f, NOW, 0);
        m.apply(r.gen, false, Ok(page(rows, None)));
        assert_eq!(previews(&m), ["clip 0", "clip 3"]);
        // The other sources stay selectable.
        assert_eq!(m.sources(5), ["editor.exe", "Browser"]);
        // A source no longer in the rows is still listed while it is the active filter.
        let r = m.refresh(NOW, 0);
        m.apply(r.gen, false, Ok(page(items(5..6), None)));
        assert_eq!(m.sources(5), ["editor.exe"]);
        assert_eq!(m.view(false), View::NoResults);
    }

    #[test]
    fn debug_output_hides_text_previews_and_sources() {
        let f = Filters {
            text: "secret query".into(),
            source: Some("secret app".into()),
            ..Filters::default()
        };
        let mut m = ListModel::new();
        let r = m.search(f.clone(), NOW, 0);
        let item = Item::from(view_item(7, false, Some("secret app")));
        for d in [format!("{f:?}"), format!("{r:?}"), format!("{item:?}")] {
            assert!(!d.contains("secret"), "{d}");
            assert!(!d.contains("clip 7"), "{d}");
        }
    }
}
