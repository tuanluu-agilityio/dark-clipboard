//! Wires the popup window to the view-model (`crate::popup`) and the worker (Task 204).
//!
//! Everything here runs on the Slint event loop (main) thread; the state lives in a
//! thread-local. `App` calls (search, copy, pin, delete) are queued on the worker thread with
//! `Poster` and answer with `upgrade_in_event_loop`; the UI thread never waits for them. Search
//! answers carry their generation and stale ones are dropped (`ListModel::apply`). Copy goes
//! through `App::copy_clip_to_clipboard` (the capture actor writes the clipboard); nothing here
//! touches the clipboard. Only previews are held, never full content.
//!
//! Logging: only the content-free `perf` event (timings and a source name). Queries, previews
//! and source names are never logged or formatted (GC-06).

use super::{Chip, ClipRow, MainWindow, Poster, Seg, Strings};
use crate::log::{Log, Val};
use crate::popup::highlight::row_segments;
use crate::popup::keys::{self, Action, CopyKind, KeyCtx, Mods};
use crate::popup::list::{Applied, Filters, Item, ListModel, Request, TimeFilter, View};
use darkclip_app::api::{Caller, CopyMode};
use darkclip_app::{ClipApi, ClipSummaryView, Page};
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::{ClipId, ClipType, Clock};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel, Weak};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Typing pause before a search runs.
const DEBOUNCE: Duration = Duration::from_millis(60);
/// A search that runs longer than this shows the loading state.
const LOADING_AFTER: Duration = Duration::from_millis(150);
/// How long a delete can be undone.
const UNDO_FOR: Duration = Duration::from_secs(5);
/// How long an error message stays.
const ERROR_FOR: Duration = Duration::from_secs(4);
/// Characters of a preview shown on one row.
const ROW_CHARS: usize = 160;
/// Source app chips shown at most.
const MAX_SOURCES: usize = 4;

/// Kind chips, in order; the chip id is the index.
const KIND_CHIPS: [ClipType; 7] = [
    ClipType::Text,
    ClipType::RichText,
    ClipType::Url,
    ClipType::Code,
    ClipType::Image,
    ClipType::Files,
    ClipType::Color,
];
const CHIP_PINNED: i32 = 100;
const CHIP_TODAY: i32 = 101;
const CHIP_WEEK: i32 = 102;
/// Source chips are `CHIP_SOURCE + index` into the current source list.
const CHIP_SOURCE: i32 = 200;

/// What opened the popup (for the `perf` event).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Hotkey,
    Ipc,
    Tray,
}

impl Trigger {
    fn name(self) -> &'static str {
        match self {
            Trigger::Hotkey => "hotkey",
            Trigger::Ipc => "ipc",
            Trigger::Tray => "tray",
        }
    }
}

fn kind_msg(k: ClipType) -> Msg {
    match k {
        ClipType::Text => Msg::ClipTypeText,
        ClipType::RichText => Msg::ClipTypeRichText,
        ClipType::Url => Msg::ClipTypeUrl,
        ClipType::Color => Msg::ClipTypeColor,
        ClipType::Code => Msg::ClipTypeCode,
        ClipType::Image => Msg::ClipTypeImage,
        ClipType::Files => Msg::ClipTypeFiles,
    }
}

/// Sets the popup's strings (called with every status refresh).
pub fn set_strings(w: &MainWindow, lang: Lang) {
    let s = w.global::<Strings>();
    s.set_search_placeholder(t(lang, Msg::UiSearchPlaceholder).into());
    s.set_search_label(t(lang, Msg::UiSearchLabel).into());
    s.set_filters_label(t(lang, Msg::UiFiltersLabel).into());
    s.set_list_label(t(lang, Msg::UiClipListLabel).into());
    s.set_empty_history(t(lang, Msg::UiEmptyHistory).into());
    s.set_no_results(t(lang, Msg::UiNoResults).into());
    s.set_loading(t(lang, Msg::UiLoading).into());
    s.set_load_failed(t(lang, Msg::UiLoadFailed).into());
    s.set_copy(t(lang, Msg::UiCopy).into());
    s.set_copy_plain(t(lang, Msg::UiCopyPlain).into());
    s.set_pin(t(lang, Msg::UiPin).into());
    s.set_unpin(t(lang, Msg::UiUnpin).into());
    s.set_delete(t(lang, Msg::UiDelete).into());
    s.set_undo(t(lang, Msg::UiUndo).into());
    s.set_close(t(lang, Msg::UiClose).into());
    s.set_key_hints(t(lang, Msg::UiKeyHints).into());
}

struct Host {
    model: ListModel,
    rows: Rc<VecModel<ClipRow>>,
    chips: Rc<VecModel<Chip>>,
    /// The source names behind the source chips, in chip order.
    sources: Vec<String>,
    poster: Poster,
    window: Weak<MainWindow>,
    lang: Lang,
    debounce: Timer,
    loading: Timer,
    toast: Timer,
    /// The running search has passed the loading threshold.
    slow: bool,
    /// A deleted clip that can still be undone (the delete is not sent yet).
    pending_delete: Option<ClipId>,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    /// When the popup was asked to show, and by what; taken by the first frame after it.
    static SHOW_ASKED: Cell<Option<(Instant, Trigger)>> = const { Cell::new(None) };
    /// The window that had the focus before the popup showed (raw handle; Task 205 returns the
    /// focus and pastes there).
    static PREVIOUS_FOREGROUND: Cell<Option<isize>> = const { Cell::new(None) };
    /// When the popup last closed itself because it lost the focus.
    static FOCUS_HIDE_AT: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Time since the popup last closed itself on focus loss.
pub fn since_focus_hide() -> Option<Duration> {
    FOCUS_HIDE_AT.get().map(|t| t.elapsed())
}

/// The window that was in front when the popup last showed (for Task 205).
#[allow(dead_code)]
pub fn previous_foreground() -> Option<isize> {
    PREVIOUS_FOREGROUND.get()
}

/// Runs `f` on the host; does nothing when it is missing or already borrowed (re-entry).
fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

fn now_and_offset() -> (darkclip_core::Timestamp, i32) {
    (
        crate::SystemClock.now(),
        super::tray_host::local_utc_offset_minutes(),
    )
}

impl Host {
    fn window(&self) -> Option<MainWindow> {
        self.window.upgrade()
    }

    fn row_for(&self, item: &Item, index: usize) -> ClipRow {
        let query = self.model.filters().text.clone();
        let segs: Vec<Seg> = row_segments(&item.preview, &query, ROW_CHARS)
            .into_iter()
            .map(|s| Seg {
                text: s.text.into(),
                hit: s.hit,
            })
            .collect();
        let label: String = segs.iter().map(|s| s.text.as_str()).collect();
        let mut meta = String::new();
        if index < 9 {
            meta.push_str(&format!("Alt+{} · ", index + 1));
        }
        meta.push_str(t(self.lang, kind_msg(item.kind)));
        if let Some(src) = &item.source {
            meta.push_str(" · ");
            meta.push_str(src);
        }
        if item.pinned {
            meta.push_str(" · ");
            meta.push_str(t(self.lang, Msg::UiFilterPinned));
        }
        ClipRow {
            segs: ModelRc::new(VecModel::from(segs)),
            meta: meta.into(),
            label: label.into(),
            pinned: item.pinned,
        }
    }

    /// Rebuilds every row (new results, or the language changed).
    fn rebuild_rows(&self) {
        let rows: Vec<ClipRow> = self
            .model
            .visible()
            .iter()
            .enumerate()
            .map(|(i, item)| self.row_for(item, i))
            .collect();
        self.rows.set_vec(rows);
    }

    fn rebuild_chips(&mut self) {
        let f = self.model.filters().clone();
        let mut chips: Vec<Chip> = KIND_CHIPS
            .iter()
            .enumerate()
            .map(|(i, k)| Chip {
                id: i32::try_from(i).unwrap_or(0),
                label: t(self.lang, kind_msg(*k)).into(),
                on: f.kinds.contains(k),
            })
            .collect();
        chips.push(Chip {
            id: CHIP_PINNED,
            label: t(self.lang, Msg::UiFilterPinned).into(),
            on: f.pinned_only,
        });
        chips.push(Chip {
            id: CHIP_TODAY,
            label: t(self.lang, Msg::UiFilterToday).into(),
            on: f.time == TimeFilter::Today,
        });
        chips.push(Chip {
            id: CHIP_WEEK,
            label: t(self.lang, Msg::UiFilter7Days).into(),
            on: f.time == TimeFilter::Week,
        });
        self.sources = self.model.sources(MAX_SOURCES);
        for (i, s) in self.sources.iter().enumerate() {
            chips.push(Chip {
                id: CHIP_SOURCE + i32::try_from(i).unwrap_or(0),
                label: s.as_str().into(),
                on: f.source.as_ref() == Some(s),
            });
        }
        // Only touch the model when something changed (keeps the chip focus stable).
        let same = self.chips.row_count() == chips.len()
            && chips
                .iter()
                .enumerate()
                .all(|(i, c)| self.chips.row_data(i).as_ref() == Some(c));
        if !same {
            self.chips.set_vec(chips);
        }
    }

    /// Selection, state and footer properties.
    fn sync_state(&self) {
        let Some(w) = self.window() else {
            return;
        };
        let state = match self.model.view(self.slow) {
            View::Rows => 0,
            View::Loading => 1,
            View::EmptyHistory => 2,
            View::NoResults => 3,
            View::Error(_) => 4,
        };
        w.set_view_state(state);
        let sel = self.model.selected();
        w.set_selected(i32::try_from(sel).unwrap_or(0));
        let item = self.model.selected_item();
        w.set_has_selection(item.is_some());
        w.set_selected_pinned(item.is_some_and(|i| i.pinned));
    }

    fn scroll_to_selected(&self) {
        if let Some(w) = self.window() {
            w.invoke_scroll_to(i32::try_from(self.model.selected()).unwrap_or(0));
        }
    }

    fn run(&mut self, req: Request) {
        let (gen, append) = (req.gen, req.append);
        let query = req.to_query();
        let ui = self.window.clone();
        self.poster.post_quiet(move |app| {
            let result = app.search_clips(&Caller::Ui, query).map_err(|e| e.code());
            let _ = ui.upgrade_in_event_loop(move |_| on_results(gen, append, result));
        });
        if !append {
            self.slow = false;
            self.loading
                .start(TimerMode::SingleShot, LOADING_AFTER, move || {
                    with_host(|h| {
                        if h.model.is_waiting(gen) {
                            h.slow = true;
                            h.sync_state();
                        }
                    });
                });
        }
    }

    /// A new search with `f` (new results start at the first row).
    fn search(&mut self, f: Filters) {
        self.debounce.stop();
        let (now, off) = now_and_offset();
        let req = self.model.search(f, now, off);
        self.run(req);
        self.rebuild_chips();
    }

    fn refresh(&mut self) {
        let (now, off) = now_and_offset();
        let req = self.model.refresh(now, off);
        self.run(req);
    }

    fn show_toast(&mut self, msg: Msg, undo: bool) {
        let Some(w) = self.window() else {
            return;
        };
        w.set_toast(t(self.lang, msg).into());
        w.set_toast_undo(undo);
        let wait = if undo { UNDO_FOR } else { ERROR_FOR };
        self.toast.start(TimerMode::SingleShot, wait, || {
            with_host(|h| {
                h.commit_delete();
                h.clear_toast();
            });
        });
    }

    fn clear_toast(&mut self) {
        self.toast.stop();
        if let Some(w) = self.window() {
            w.set_toast(SharedString::new());
            w.set_toast_undo(false);
        }
    }

    /// Sends the pending delete to the worker (the undo window is over).
    fn commit_delete(&mut self) {
        let Some(id) = self.pending_delete.take() else {
            return;
        };
        let ui = self.window.clone();
        self.poster.post(move |app| {
            if app.delete_clip(&Caller::Ui, id).is_err() {
                let _ = ui.upgrade_in_event_loop(move |_| {
                    with_host(|h| {
                        // Show the clip again and say it did not work.
                        if let Some(at) = h.model.unhide(id) {
                            h.reinsert(at);
                            h.sync_state();
                        }
                        h.show_toast(Msg::UiActionFailed, false);
                    });
                });
            }
        });
    }

    fn delete_selected(&mut self) {
        let Some(id) = self.model.selected_item().map(|i| i.id) else {
            return;
        };
        self.commit_delete();
        if let Some(at) = self.model.hide(id) {
            if at < self.rows.row_count() {
                self.rows.remove(at);
            }
            self.fix_hints(at);
            self.pending_delete = Some(id);
            self.sync_state();
            self.show_toast(Msg::UiDeleted, true);
        }
    }

    fn undo_delete(&mut self) {
        let Some(id) = self.pending_delete.take() else {
            return;
        };
        if let Some(at) = self.model.unhide(id) {
            self.reinsert(at);
            self.model.select(at);
            self.sync_state();
            self.scroll_to_selected();
        }
        self.clear_toast();
    }

    /// Puts the row at visible index `at` back into the Slint model.
    fn reinsert(&self, at: usize) {
        if let Some(item) = self.model.nth(at) {
            let row = self.row_for(item, at);
            self.rows.insert(at.min(self.rows.row_count()), row);
        }
        self.fix_hints(at);
    }

    /// Rows from `from` on moved: their Alt+1..9 hints are redrawn (only the first nine have one).
    fn fix_hints(&self, from: usize) {
        for i in from..9 {
            match self.model.nth(i) {
                Some(item) if i < self.rows.row_count() => {
                    self.rows.set_row_data(i, self.row_for(item, i));
                }
                _ => break,
            }
        }
    }

    fn toggle_pin(&mut self) {
        let Some((id, pinned)) = self.model.selected_item().map(|i| (i.id, i.pinned)) else {
            return;
        };
        let ui = self.window.clone();
        // Not quiet: the status refresh afterwards re-runs the search (see `app_changed`).
        self.poster.post(move |app| {
            if app.pin_clip(&Caller::Ui, id, !pinned).is_err() {
                let _ = ui.upgrade_in_event_loop(|_| {
                    with_host(|h| h.show_toast(Msg::UiActionFailed, false));
                });
            }
        });
    }

    fn copy(&mut self, item: Option<ClipId>, kind: CopyKind) {
        let Some(id) = item else {
            return;
        };
        // Ctrl+Enter copies until Task 205 adds the paste.
        let mode = match kind {
            CopyKind::Plain => CopyMode::PlainText,
            CopyKind::Original | CopyKind::Paste => CopyMode::Original,
        };
        let ui = self.window.clone();
        self.poster.post_quiet(move |app| {
            let ok = app.copy_clip_to_clipboard(&Caller::Ui, id, mode).is_ok();
            let _ = ui.upgrade_in_event_loop(move |w| {
                if ok {
                    // J1: the clip is on the clipboard, the popup closes.
                    hide(&w);
                } else {
                    with_host(|h| h.show_toast(Msg::UiActionFailed, false));
                }
            });
        });
    }

    fn after_move(&mut self) {
        self.sync_state();
        self.scroll_to_selected();
        self.maybe_load_more(self.model.selected());
    }

    fn maybe_load_more(&mut self, last_shown: usize) {
        if self.model.near_end(last_shown) {
            if let Some(req) = self.model.load_more() {
                self.run(req);
            }
        }
    }

    /// Whole rows that fit in the list.
    fn page_rows(&self) -> usize {
        self.window()
            .map_or(8, |w| usize::try_from(w.get_page_rows()).unwrap_or(8))
    }

    fn act(&mut self, a: Action) -> bool {
        match a {
            Action::Move(m) => {
                let page = self.page_rows();
                self.model.move_by(m, page);
                self.after_move();
            }
            Action::Copy(kind) => {
                let id = self.model.selected_item().map(|i| i.id);
                self.copy(id, kind);
            }
            Action::CopyNth(n) => {
                let id = self.model.nth(n).map(|i| i.id);
                if id.is_some() {
                    self.model.select(n);
                    self.sync_state();
                }
                self.copy(id, CopyKind::Original);
            }
            Action::Delete => self.delete_selected(),
            Action::TogglePin => self.toggle_pin(),
            Action::Undo => self.undo_delete(),
            Action::Close => {
                // Hide outside the borrow (it calls back into the host).
                let ui = self.window.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = ui.upgrade() {
                        hide(&w);
                    }
                });
            }
        }
        true
    }

    fn toggle_chip(&mut self, id: i32) {
        let mut f = self.model.filters().clone();
        // Typing that has not run yet belongs to this search.
        if let Some(w) = self.window() {
            f.text = w.get_query().to_string();
        }
        match id {
            CHIP_PINNED => f.pinned_only = !f.pinned_only,
            CHIP_TODAY | CHIP_WEEK => {
                let want = if id == CHIP_TODAY {
                    TimeFilter::Today
                } else {
                    TimeFilter::Week
                };
                f.time = if f.time == want {
                    TimeFilter::Any
                } else {
                    want
                };
            }
            id if id >= CHIP_SOURCE => {
                let i = usize::try_from(id - CHIP_SOURCE).unwrap_or(usize::MAX);
                let Some(src) = self.sources.get(i).cloned() else {
                    return;
                };
                f.source = if f.source.as_ref() == Some(&src) {
                    None
                } else {
                    Some(src)
                };
            }
            id => {
                let Some(k) = usize::try_from(id).ok().and_then(|i| KIND_CHIPS.get(i)) else {
                    return;
                };
                if let Some(pos) = f.kinds.iter().position(|x| x == k) {
                    f.kinds.remove(pos);
                } else {
                    f.kinds.push(*k);
                }
            }
        }
        self.search(f);
    }
}

/// A search answered (event loop).
fn on_results(gen: u64, append: bool, result: Result<Page<ClipSummaryView>, &'static str>) {
    with_host(|h| {
        match h.model.apply(gen, append, result) {
            Applied::Ignored => return,
            Applied::Replaced => {
                h.loading.stop();
                h.slow = false;
                h.rebuild_rows();
                h.scroll_to_selected();
            }
            Applied::Appended { from } => {
                let visible = h.model.visible();
                let new: Vec<ClipRow> = visible
                    .iter()
                    .enumerate()
                    .skip(from)
                    .map(|(i, item)| h.row_for(item, i))
                    .collect();
                for r in new {
                    h.rows.push(r);
                }
            }
        }
        h.rebuild_chips();
        h.sync_state();
        // A short list (e.g. a source filter) keeps loading pages until the view is filled.
        let last_shown = h.model.selected().saturating_add(h.page_rows());
        h.maybe_load_more(last_shown);
    });
}

/// Creates the host and connects the window's callbacks. Call once, on the main thread.
pub fn install(w: &MainWindow, poster: Poster, lang: Lang, log: Arc<Log>) {
    let rows = Rc::new(VecModel::<ClipRow>::default());
    let chips = Rc::new(VecModel::<Chip>::default());
    w.set_rows(ModelRc::from(rows.clone()));
    w.set_chips(ModelRc::from(chips.clone()));
    set_strings(w, lang);
    let mut host = Host {
        model: ListModel::new(),
        rows,
        chips,
        sources: Vec::new(),
        poster,
        window: w.as_weak(),
        lang,
        debounce: Timer::default(),
        loading: Timer::default(),
        toast: Timer::default(),
        slow: false,
        pending_delete: None,
    };
    host.rebuild_chips();
    host.sync_state();
    HOST.with(|cell| *cell.borrow_mut() = Some(host));

    w.on_query_edited(|_| {
        with_host(|h| {
            // Debounced: the search runs when typing pauses (each edit restarts the timer).
            h.debounce.start(TimerMode::SingleShot, DEBOUNCE, || {
                with_host(|h| {
                    let mut f = h.model.filters().clone();
                    if let Some(w) = h.window() {
                        f.text = w.get_query().to_string();
                    }
                    h.search(f);
                });
            });
        });
    });
    w.on_key(
        |text, ctrl, shift, alt, meta, search_focused, chips_focused| {
            with_host(|h| {
                let query_empty = h.window().is_none_or(|w| w.get_query().is_empty());
                let ctx = KeyCtx {
                    query_empty,
                    search_focused,
                    undo_available: h.pending_delete.is_some(),
                    chips_focused,
                };
                let mods = Mods {
                    ctrl,
                    shift,
                    alt,
                    meta,
                };
                match keys::map_key(keys::key_from_text(&text), mods, ctx) {
                    Some(a) => h.act(a),
                    None => false,
                }
            })
            .unwrap_or(false)
        },
    );
    w.on_chip_toggled(|id| {
        with_host(|h| h.toggle_chip(id));
    });
    w.on_row_clicked(|i| {
        with_host(|h| {
            h.model.select(usize::try_from(i).unwrap_or(0));
            h.sync_state();
        });
    });
    w.on_row_activated(|i| {
        with_host(|h| {
            let n = usize::try_from(i).unwrap_or(0);
            h.model.select(n);
            h.sync_state();
            h.act(Action::Copy(CopyKind::Original));
        });
    });
    w.on_act_copy(|| {
        with_host(|h| h.act(Action::Copy(CopyKind::Original)));
    });
    w.on_act_copy_plain(|| {
        with_host(|h| h.act(Action::Copy(CopyKind::Plain)));
    });
    w.on_act_pin(|| {
        with_host(|h| h.act(Action::TogglePin));
    });
    w.on_act_delete(|| {
        with_host(|h| h.act(Action::Delete));
    });
    w.on_act_undo(|| {
        with_host(|h| h.act(Action::Undo));
    });
    w.on_act_close(|| {
        with_host(|h| h.act(Action::Close));
    });
    w.on_shown_until(|last| {
        with_host(|h| h.maybe_load_more(usize::try_from(last).unwrap_or(0)));
    });

    // NFR-01: the first frame after a show request is the "popup visible" moment.
    let perf_log = log;
    let notifier = w.window().set_rendering_notifier(move |state, _| {
        if matches!(state, slint::RenderingState::AfterRendering) {
            if let Some((t0, trigger)) = SHOW_ASKED.take() {
                let us = u64::try_from(t0.elapsed().as_micros()).unwrap_or(u64::MAX);
                perf_log.event(
                    "perf",
                    &[
                        ("what", Val::Name("popup_shown")),
                        ("src", Val::Name(trigger.name())),
                        ("us", Val::Num(us)),
                    ],
                );
            }
        }
    });
    if notifier.is_err() {
        // A renderer without notifications: the app works, only the measurement is missing.
        SHOW_ASKED.set(None);
    }
}

/// The saved language changed (or a status refresh): relabel chips and rows.
pub fn set_lang(lang: Lang) {
    with_host(|h| {
        if h.lang != lang {
            h.lang = lang;
            h.rebuild_chips();
            h.rebuild_rows();
        }
    });
}

/// The data may have changed (a clip was added, pinned or deleted): refresh a visible popup.
pub fn app_changed() {
    with_host(|h| {
        let visible = h.window().is_some_and(|w| w.window().is_visible());
        if visible && !h.debounce.running() {
            h.refresh();
        }
    });
}

/// Shows the popup near the cursor with an empty search (J1). `asked` is when the shortcut,
/// tray or `--toggle` asked for it.
pub fn show(w: &MainWindow, asked: Instant, trigger: Trigger) {
    #[cfg(windows)]
    {
        PREVIOUS_FOREGROUND.set(crate::winsys::foreground_window_of_other_process());
        if let Some(m) = crate::winsys::monitor_at_cursor() {
            use crate::popup::place::{popup_origin, scale, POPUP_LOGICAL};
            let to_i32 = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
            let size = (
                scale(to_i32(POPUP_LOGICAL.0), m.dpi),
                scale(to_i32(POPUP_LOGICAL.1), m.dpi),
            );
            let (x, y) = popup_origin(m.cursor, size, m.work, m.dpi);
            w.window().set_position(slint::PhysicalPosition::new(x, y));
        }
    }
    w.set_query(SharedString::new());
    with_host(|h| {
        h.model.clear_hidden();
        h.clear_toast();
        h.search(Filters::default());
        h.sync_state();
    });
    SHOW_ASKED.set(Some((asked, trigger)));
    let _ = w.show();
    focus_os_window(w);
    w.invoke_focus_search();
}

/// Brings the popup's OS window to the front with the keyboard focus (allowed after a shortcut
/// or tray click; Windows may refuse it after `--toggle` from another process).
fn focus_os_window(w: &MainWindow) {
    use slint::winit_030::WinitWindowAccessor;
    let _ = w.window().with_winit_window(|ww| ww.focus_window());
}

/// Hides the popup; a delete waiting for its undo is sent now.
pub fn hide(w: &MainWindow) {
    with_host(|h| {
        h.debounce.stop();
        h.commit_delete();
        h.clear_toast();
    });
    let _ = w.hide();
}

/// Closes the popup when its window loses the focus (setting in Task 207).
pub fn close_on_focus_loss(w: &MainWindow) {
    use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
    let ui = w.as_weak();
    w.window().on_winit_window_event(move |_, ev| {
        if let winit::event::WindowEvent::Focused(false) = ev {
            let ui = ui.clone();
            // Not inside winit's event dispatch: hop to the next loop turn.
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = ui.upgrade() {
                    if w.window().is_visible() {
                        FOCUS_HIDE_AT.set(Some(Instant::now()));
                        hide(&w);
                    }
                }
            });
        }
        EventResult::Propagate
    });
}

/// The window was closed by the system (Alt+F4): like `hide`, without hiding twice.
pub fn shutdown_pending() {
    with_host(|h| {
        h.debounce.stop();
        h.commit_delete();
        h.clear_toast();
    });
}

/// Before quitting: a delete waiting for its undo is sent (the worker runs it before stopping).
pub fn shutdown() {
    with_host(|h| {
        h.commit_delete();
        h.debounce.stop();
        h.loading.stop();
        h.toast.stop();
    });
}
