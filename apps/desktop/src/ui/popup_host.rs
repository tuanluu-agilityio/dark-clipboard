//! Wires the popup window to the view-model (`crate::popup`) and the worker (Task 204).
//!
//! Everything here runs on the Slint event loop (main) thread; the state lives in a
//! thread-local. `App` calls (search, copy, pin, delete) are queued on the worker thread with
//! `Poster` and answer with `upgrade_in_event_loop`; the UI thread never waits for them. Search
//! answers carry their generation and stale ones are dropped (`ListModel::apply`). Copy goes
//! through `App::copy_clip_to_clipboard` (the capture actor writes the clipboard); nothing here
//! touches the clipboard. Rows hold previews only.
//!
//! Detail pane (Task 206): `popup::detail::DetailModel` says what to load; the loads (full
//! text up to 32 KiB, the image thumbnail and file list, the downscaled full image) run on
//! the worker and arrive here as generation-free answers keyed by clip id (stale ones are
//! dropped by the model). Images are decoded on the worker into `SharedPixelBuffer`s; the UI
//! thread only wraps them. Masked clips never request anything. Paths are shown, never logged.
//!
//! Copy back and paste (Task 205): after the copy succeeded, Enter returns the focus to the
//! window recorded at show time; Ctrl+Enter (or Enter with "Enter also pastes") also pastes
//! there through the platform `PasteInjector`, on a short-lived `darkclip-paste` thread (it may
//! wait up to about 1.3 s for keys to be released and the focus to move; the UI thread never
//! waits). A refused paste leaves the clip copied and shows a static toast.
//!
//! Logging: only the content-free `perf` and `paste` events (timings, a source name, a coarse
//! outcome). `perf what=popup_shown` (NFR-01) runs from the show request to the first frame
//! drawn after it; `perf what=search_done` (Task 212) from the debounced search of a typed
//! query to its results being applied to the list. Neither carries a query or a count.
//! Queries, previews, source names and window titles are never logged or formatted (GC-06,
//! SR-11).

use super::common::{self, clip_row, Px};
use super::{Chip, ClipRow, MainWindow, Poster, Strings};
use crate::log::{Log, Val};
use crate::popup::chips;
use crate::popup::detail::{DetailModel, DetailRequest};
use crate::popup::keys::{self, Action, ChipFocus, CopyKind, KeyCtx, Mods};
use crate::popup::list::{Applied, Filters, Item, ListModel, Request, View};
use crate::popup::paste::{self as paste_rules, AfterCopy, Via};
use crate::popup::transform::{self as tf, PanelKey, Session};
use darkclip_app::api::{Caller, CopyMode};
use darkclip_app::{ClipApi, ClipSummaryView, Page};
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::{ClipId, Clock};
use darkclip_platform::{PasteInjector, PlatformError};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel, Weak};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Typing pause before a search runs.
const DEBOUNCE: Duration = Duration::from_millis(60);
/// A search that runs longer than this shows the loading state.
const LOADING_AFTER: Duration = Duration::from_millis(150);
/// How long a delete can be undone.
const UNDO_FOR: Duration = Duration::from_secs(5);
/// How long an error message stays.
const ERROR_FOR: Duration = Duration::from_secs(4);
/// Source app chips shown at most.
const MAX_SOURCES: usize = 4;

/// The platform's paste injector, shared with the paste threads (one job at a time).
pub type Injector = Arc<Mutex<Box<dyn PasteInjector>>>;

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

/// Sets the popup's strings (called with every status refresh).
pub fn set_strings(w: &MainWindow, lang: Lang) {
    common::set_shared_strings(w, lang);
    let (can_paste, enter_pastes) = PASTE_PREFS.get();
    w.global::<Strings>()
        .set_settings_open(t(lang, Msg::SettingsOpen).into());
    w.global::<Strings>()
        .set_key_hints(t(lang, paste_rules::key_hints(can_paste, enter_pastes)).into());
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
    /// Which chip has the keyboard focus (from the chips' own reports).
    chip_focus: ChipFocus,
    /// The platform's paste injector; `None` where auto-paste is unsupported.
    injector: Option<Injector>,
    /// A focus return or paste is running: further copies wait for it.
    pasting: bool,
    /// The open "Copy as..." panel (Task 404), if any.
    tf: Option<Session>,
    /// The detail pane's state and its decoded-image cache.
    detail: DetailModel<Px>,
    /// The clip the pane wants loaded now. Worker jobs for any other clip are skipped before
    /// they do their work (the selection moved on while they were queued).
    wanted: Arc<Mutex<Option<ClipId>>>,
    /// The search a typed query started (its generation and when the debounce ran it), for the
    /// `search_done` timing.
    typed_search: Option<(u64, Instant)>,
    log: Arc<Log>,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    /// When the popup was asked to show, and by what; taken by the first frame after it.
    static SHOW_ASKED: Cell<Option<(Instant, Trigger)>> = const { Cell::new(None) };
    /// The window that had the focus before the popup showed: handle value and process id
    /// (Task 205 returns the focus and pastes there). Never a title.
    static PREVIOUS_FOREGROUND: Cell<Option<(u64, u32)>> = const { Cell::new(None) };
    /// (auto-paste supported, "Enter also pastes"), for the key hints and the Enter key.
    static PASTE_PREFS: Cell<(bool, bool)> = const { Cell::new((false, false)) };
    /// (open at the center of the screen, close on focus loss): the settings (Task 207).
    static PLACE_PREFS: Cell<(bool, bool)> = const { Cell::new((false, true)) };
    /// When the popup last closed itself because it lost the focus.
    static FOCUS_HIDE_AT: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Time since the popup last closed itself on focus loss.
pub fn since_focus_hide() -> Option<Duration> {
    FOCUS_HIDE_AT.get().map(|t| t.elapsed())
}

/// The window that was in front when the popup last showed, as a paste target.
fn previous_target() -> Option<darkclip_platform::PasteTarget> {
    paste_rules::target(PREVIOUS_FOREGROUND.get())
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

/// The chips for `f` and the source names behind the source chips.
fn chip_rows(f: &Filters, sources: &[String], lang: Lang) -> Vec<Chip> {
    chips::popup_chips(f, sources, lang)
        .into_iter()
        .map(|c| Chip {
            id: c.id,
            label: c.label.into(),
            on: c.on,
        })
        .collect()
}

/// Forgets the list of a popup session that ended (D-056, like the history window's `leave`):
/// the rows, the source names behind the source chips, the chip focus and the model itself.
/// The model's generation moves on, so an answer still in flight is dropped when it arrives,
/// and the next show starts from an empty list: no clip from before (deleted in History,
/// cleared in Settings or expired meanwhile) can be drawn or acted on before its first page.
fn forget_list(
    model: &mut ListModel,
    rows: &VecModel<ClipRow>,
    chips: &VecModel<Chip>,
    sources: &mut Vec<String>,
    chip_focus: &mut ChipFocus,
    lang: Lang,
) {
    model.clear();
    rows.set_vec(Vec::new());
    sources.clear();
    *chip_focus = ChipFocus::default();
    // The fixed chips stay (all off); no source chip names an app from before.
    chips.set_vec(chip_rows(model.filters(), sources, lang));
}

impl Host {
    fn window(&self) -> Option<MainWindow> {
        self.window.upgrade()
    }

    fn row_for(&self, item: &Item, index: usize) -> ClipRow {
        clip_row(item, index, self.lang, &self.model.filters().text)
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
        self.sources = self.model.sources(MAX_SOURCES);
        let chips = chip_rows(self.model.filters(), &self.sources, self.lang);
        // Changed chips are updated in place, so a toggled chip keeps the keyboard focus; the
        // model is replaced only when the set of chips changes (e.g. other source apps).
        let old: Vec<Chip> = self.chips.iter().collect();
        match crate::popup::in_place_changes(&old, &chips, |c| c.id) {
            Some(changed) => {
                for i in changed {
                    if let Some(c) = chips.get(i) {
                        self.chips.set_row_data(i, c.clone());
                    }
                }
            }
            None => {
                self.chip_focus
                    .retain(|id| chips.iter().any(|c| c.id == id));
                self.chips.set_vec(chips);
            }
        }
    }

    /// Selection, state and footer properties.
    fn sync_state(&mut self) {
        self.sync_state_only();
        self.sync_detail();
    }

    fn sync_state_only(&self) {
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
        w.set_can_transform(item.is_some_and(|i| tf::can_transform(i.kind, i.masked)));
    }

    /// The selection or the pane changed: load what the pane needs and redraw it.
    fn sync_detail(&mut self) {
        let item = self.model.selected_item().cloned();
        let reqs = self.detail.sync(item.as_ref());
        for r in reqs {
            self.run_detail(r);
        }
        self.render_detail();
    }

    fn set_detail_open(&mut self, open: bool) {
        let item = self.model.selected_item().cloned();
        let reqs = self.detail.set_open(open, item.as_ref());
        for r in reqs {
            self.run_detail(r);
        }
        self.render_detail();
    }

    fn toggle_detail(&mut self) {
        let open = !self.detail.is_open();
        self.set_detail_open(open);
    }

    fn load_full_text(&mut self) {
        if let Some(item) = self.model.selected_item().cloned() {
            if let Some(r) = self.detail.load_full_text(&item) {
                self.run_detail(r);
            }
        }
        self.render_detail();
    }

    fn load_full_image(&mut self) {
        if let Some(item) = self.model.selected_item().cloned() {
            if let Some(r) = self.detail.load_full_image(&item) {
                self.run_detail(r);
            }
        }
        self.render_detail();
    }

    /// Runs one load on the worker (see `common::run_detail`).
    fn run_detail(&mut self, r: DetailRequest) {
        let ui = self.window.clone();
        common::run_detail(&self.poster, &self.wanted, r, move |a| {
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| {
                    if common::apply_answer(&mut h.detail, a) {
                        h.render_detail();
                    }
                });
            });
        });
    }

    /// Mirrors the detail model into the window's properties.
    fn render_detail(&mut self) {
        let Some(w) = self.window() else {
            return;
        };
        let item = self.model.selected_item().cloned();
        let open = self.detail.is_open() && item.is_some();
        w.set_detail_open(open);
        if let Ok(mut wanted) = self.wanted.lock() {
            *wanted = item.as_ref().filter(|_| open).map(|i| i.id);
        }
        let Some(item) = item.filter(|_| open) else {
            // Nothing stays in the hidden window: no text, picture or file names.
            w.set_detail(common::empty_detail());
            w.set_detail_text(SharedString::new());
            return;
        };
        let (state, text) = common::render_state(&item, &mut self.detail, self.lang);
        w.set_detail(state);
        if let Some(t) = common::set_text_if_changed(&w.get_detail_text(), text) {
            w.set_detail_text(t);
        }
    }

    /// The popup session ended (hide, or the system closed the window): nothing of it stays.
    /// A delete waiting for its undo is sent first, so the next show cannot list it.
    fn end_session(&mut self) {
        self.debounce.stop();
        self.loading.stop();
        self.commit_delete();
        self.close_copy_as();
        self.clear_toast();
        self.slow = false;
        self.typed_search = None;
        forget_list(
            &mut self.model,
            &self.rows,
            &self.chips,
            &mut self.sources,
            &mut self.chip_focus,
            self.lang,
        );
        // Decoded images and loaded text do not outlive the popup.
        self.detail.clear();
        if let Some(w) = self.window() {
            w.set_query(SharedString::new());
        }
        self.sync_state_only();
        self.render_detail();
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

    /// A new search with `f` (new results start at the first row). Returns its generation.
    fn search(&mut self, f: Filters) -> u64 {
        self.debounce.stop();
        let (now, off) = now_and_offset();
        let req = self.model.search(f, now, off);
        let gen = req.gen;
        self.run(req);
        self.rebuild_chips();
        gen
    }

    /// Logs `search_done` when the answer of the last typed search was applied.
    fn log_search_done(&mut self, gen: u64) {
        if self.typed_search.is_some_and(|(g, _)| g == gen) {
            if let Some((_, t0)) = self.typed_search.take() {
                let ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
                self.log.event(
                    "perf",
                    &[("what", Val::Name("search_done")), ("ms", Val::Num(ms))],
                );
            }
        }
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
        if !undo && self.pending_delete.is_some() {
            // An error during the undo window: say so, but keep Undo and the 5 s counted
            // from the delete (the running timer still sends the delete when it ends).
            w.set_toast(t(self.lang, msg).into());
            w.set_toast_undo(true);
            return;
        }
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
            self.detail.forget(id);
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

    // ---- "Copy as..." (Task 404) ----

    /// Opens the panel for the selected clip (never for a masked one, an image or files).
    fn open_copy_as(&mut self) {
        let Some(item) = self.model.selected_item() else {
            return;
        };
        if self.tf.is_some() || !tf::can_transform(item.kind, item.masked) {
            return;
        }
        self.tf = Some(Session::new(item.id));
        if let Some(w) = self.window() {
            w.set_tf_labels(common::transform_labels(self.lang));
            w.set_tf_selected(0);
            self.show_preview(&w, common::empty_preview());
            w.set_tf_open(true);
        }
        self.request_preview();
    }

    fn show_preview(&self, w: &MainWindow, v: common::PreviewView) {
        w.set_tf_preview(v.text);
        w.set_tf_note(v.note);
        w.set_tf_note_error(v.is_error);
        w.set_tf_can_copy(v.can_copy);
    }

    /// Asks the worker for the highlighted transform's output (nothing is written).
    fn request_preview(&mut self) {
        let Some(s) = self.tf.as_mut() else {
            return;
        };
        let (id, transform, gen) = (s.clip(), s.transform(), s.next_gen());
        let ui = self.window.clone();
        self.poster.post_quiet(move |app| {
            let r = app.ui_transform_preview(&Caller::Ui, id, transform);
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| h.on_preview(gen, &r));
            });
        });
    }

    fn on_preview(
        &mut self,
        gen: u64,
        r: &Result<darkclip_app::api::TransformPreview, darkclip_app::ApiError>,
    ) {
        if !self.tf.as_ref().is_some_and(|s| s.is_current(gen)) {
            return;
        }
        if let Some(w) = self.window() {
            self.show_preview(&w, common::preview_view(self.lang, r));
        }
    }

    fn choose_transform(&mut self, i: usize) {
        let moved = self.tf.as_mut().is_some_and(|s| s.select(i));
        self.after_highlight(moved);
    }

    fn after_highlight(&mut self, moved: bool) {
        if !moved {
            return;
        }
        if let (Some(w), Some(s)) = (self.window(), self.tf.as_ref()) {
            w.set_tf_selected(i32::try_from(s.selected()).unwrap_or(0));
            self.show_preview(&w, common::empty_preview());
        }
        self.request_preview();
    }

    fn panel_key(&mut self, k: PanelKey) {
        match k {
            PanelKey::Previous | PanelKey::Next => {
                let moved = self.tf.as_mut().is_some_and(|s| s.step(k));
                self.after_highlight(moved);
            }
            PanelKey::Copy => self.copy_transformed(),
            PanelKey::Close => self.close_copy_as(),
            PanelKey::Swallow => {}
        }
    }

    /// Writes the previewed output to the clipboard (a self-write; nothing is stored), then
    /// does what a normal copy does: the focus goes back and the popup closes.
    fn copy_transformed(&mut self) {
        let Some(w) = self.window() else {
            return;
        };
        let Some(s) = self.tf.as_ref() else {
            return;
        };
        if !w.get_tf_can_copy() {
            return;
        }
        if self.pasting {
            self.show_toast(paste_rules::BUSY_TOAST, false);
            return;
        }
        let (id, transform) = (s.clip(), s.transform());
        let ui = self.window.clone();
        self.poster.post_quiet(move |app| {
            let ok = app.ui_copy_transformed(&Caller::Ui, id, transform).is_ok();
            let _ = ui.upgrade_in_event_loop(move |w| {
                if ok {
                    with_host(|h| h.close_copy_as());
                    let started =
                        with_host(|h| h.return_focus(AfterCopy::Restore)).unwrap_or(false);
                    if !started {
                        hide(&w);
                    }
                } else {
                    with_host(|h| {
                        h.close_copy_as();
                        h.show_toast(Msg::UiActionFailed, false);
                    });
                }
            });
        });
    }

    /// Closes the panel; no output text stays in the window.
    fn close_copy_as(&mut self) {
        self.tf = None;
        if let Some(w) = self.window() {
            w.set_tf_open(false);
            self.show_preview(&w, common::empty_preview());
        }
    }

    fn copy(&mut self, item: Option<ClipId>, kind: CopyKind, via: Via) {
        let Some(id) = item else {
            return;
        };
        if self.pasting {
            // The previous copy is still returning the focus or pasting.
            self.show_toast(paste_rules::BUSY_TOAST, false);
            return;
        }
        let (_, enter_pastes) = PASTE_PREFS.get();
        let after = paste_rules::after_copy(kind, via, enter_pastes, self.injector.is_some());
        let mode = match kind {
            CopyKind::Plain => CopyMode::PlainText,
            CopyKind::Original | CopyKind::Paste => CopyMode::Original,
        };
        let ui = self.window.clone();
        self.poster.post_quiet(move |app| {
            let ok = app.copy_clip_to_clipboard(&Caller::Ui, id, mode).is_ok();
            let _ = ui.upgrade_in_event_loop(move |w| {
                if ok {
                    // J1: the clip is on the clipboard; the focus goes back (and the paste
                    // runs), then the popup closes.
                    let started = with_host(|h| h.return_focus(after)).unwrap_or(false);
                    if !started {
                        hide(&w);
                    }
                } else {
                    with_host(|h| h.show_toast(Msg::UiActionFailed, false));
                }
            });
        });
    }

    /// Starts the focus return (and the paste for `AfterCopy::Paste`) on a paste thread.
    /// Returns false when nothing was started and the popup should simply close. A paste with
    /// no recorded target shows the "paste manually" toast and keeps the popup open.
    fn return_focus(&mut self, after: AfterCopy) -> bool {
        let (Some(injector), Some(target)) = (self.injector.clone(), previous_target()) else {
            if after == AfterCopy::Paste {
                self.log_paste("refused");
                self.show_toast(Msg::UiPasteManual, false);
                return true;
            }
            return false;
        };
        let ui = self.window.clone();
        let spawned = std::thread::Builder::new()
            .name("darkclip-paste".into())
            .spawn(move || {
                let result = match injector.lock() {
                    Ok(mut inj) => match after {
                        AfterCopy::Paste => inj.paste(&target),
                        AfterCopy::Restore => inj.focus(&target),
                    },
                    Err(_) => Err(PlatformError::Unsupported("paste injector unavailable")),
                };
                let _ = ui.upgrade_in_event_loop(move |w| on_paste_done(&w, after, result));
            });
        if spawned.is_err() {
            return false;
        }
        self.pasting = true;
        true
    }

    fn log_paste(&self, outcome: &'static str) {
        self.log.event("paste", &[("result", Val::Name(outcome))]);
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
                self.copy(id, kind, Via::Enter);
            }
            Action::CopyNth(n) => {
                let id = self.model.nth(n).map(|i| i.id);
                if id.is_some() {
                    self.model.select(n);
                    self.sync_state();
                }
                self.copy(id, CopyKind::Original, Via::Other);
            }
            Action::Delete => self.delete_selected(),
            Action::TogglePin => self.toggle_pin(),
            Action::Undo => self.undo_delete(),
            Action::ToggleDetail => self.toggle_detail(),
            Action::OpenDetail => self.set_detail_open(true),
            Action::CopyAs => self.open_copy_as(),
            Action::CloseDetail => self.set_detail_open(false),
            Action::OpenHistory => {
                // Close the popup (without returning the focus: the history is a window of its
                // own), then open the history. Outside the borrow: both call back into hosts.
                let ui = self.window.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = ui.upgrade() {
                        hide(&w);
                    }
                    super::history_host::show();
                });
            }
            Action::Close => {
                // Hide outside the borrow (it calls back into the host).
                let ui = self.window.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = ui.upgrade() {
                        // Esc returns the focus like Enter (Review Focus 3); never pastes.
                        let started =
                            with_host(|h| !h.pasting && h.return_focus(AfterCopy::Restore))
                                .unwrap_or(false);
                        if !started {
                            hide(&w);
                        }
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
        let Some(f) = chips::toggle(&f, id, &self.sources) else {
            return;
        };
        self.search(f);
    }
}

/// A focus return or paste finished (event loop). The clip is on the clipboard either way.
fn on_paste_done(w: &MainWindow, after: AfterCopy, result: Result<(), PlatformError>) {
    let toast = with_host(|h| {
        h.pasting = false;
        if after == AfterCopy::Restore {
            return None;
        }
        h.log_paste(paste_rules::outcome_name(&result));
        result.as_ref().err().map(paste_rules::paste_toast)
    })
    .flatten();
    match toast {
        // Shown in the popup while it is open (a refusal before the focus moved keeps it open);
        // the user closes it with Esc.
        Some(msg) => {
            with_host(|h| h.show_toast(msg, false));
        }
        None => hide(w),
    }
}

/// The platform's paste injector arrived from the worker (only where auto-paste is native).
pub fn set_injector(injector: Injector) {
    with_host(|h| h.injector = Some(injector));
    let (_, enter) = PASTE_PREFS.get();
    PASTE_PREFS.set((true, enter));
    refresh_hints();
}

/// The popup position and close-on-focus-loss settings (every status refresh).
pub fn set_window_prefs(center: bool, close_on_focus_loss: bool) {
    PLACE_PREFS.set((center, close_on_focus_loss));
}

/// The "Enter also pastes" setting (every status refresh).
pub fn set_enter_pastes(on: bool) {
    let (can, old) = PASTE_PREFS.get();
    if old != on {
        PASTE_PREFS.set((can, on));
        refresh_hints();
    }
}

fn refresh_hints() {
    with_host(|h| {
        if let Some(w) = h.window() {
            let (can, enter) = PASTE_PREFS.get();
            w.global::<Strings>()
                .set_key_hints(t(h.lang, paste_rules::key_hints(can, enter)).into());
        }
    });
}

/// A search answered (event loop).
fn on_results(gen: u64, append: bool, result: Result<Page<ClipSummaryView>, &'static str>) {
    with_host(|h| {
        match h.model.apply(gen, append, result) {
            Applied::Ignored => return,
            Applied::More(req) => {
                // A refresh continues; the rows on screen stay until it is complete.
                h.run(req);
                return;
            }
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
        h.log_search_done(gen);
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
        chip_focus: ChipFocus::default(),
        injector: None,
        pasting: false,
        tf: None,
        detail: DetailModel::default(),
        wanted: Arc::new(Mutex::new(None)),
        typed_search: None,
        log: Arc::clone(&log),
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
                    let started = Instant::now();
                    let gen = h.search(f);
                    h.typed_search = Some((gen, started));
                });
            });
        });
    });
    w.on_chip_focus(|id, on| {
        with_host(|h| h.chip_focus.report(id, on));
    });
    w.on_key(|text, ctrl, shift, alt, meta, search_focused| {
        with_host(|h| {
            // The "Copy as..." panel is modal: it takes every key.
            if h.tf.is_some() {
                h.panel_key(tf::panel_key(keys::key_from_text(&text)));
                return true;
            }
            let query_empty = h.window().is_none_or(|w| w.get_query().is_empty());
            let ctx = KeyCtx {
                query_empty,
                search_focused,
                undo_available: h.pending_delete.is_some(),
                chips_focused: h.chip_focus.focused(search_focused),
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
    });
    w.on_act_copy_as(|| {
        with_host(|h| h.open_copy_as());
    });
    w.on_tf_chosen(|i| {
        with_host(|h| h.choose_transform(usize::try_from(i).unwrap_or(usize::MAX)));
    });
    w.on_tf_copy(|| {
        with_host(|h| h.copy_transformed());
    });
    w.on_tf_cancel(|| {
        with_host(|h| h.close_copy_as());
    });
    w.on_chip_toggled(|id| {
        with_host(|h| h.toggle_chip(id));
    });
    w.on_row_clicked(|i| {
        with_host(|h| {
            h.model.select(usize::try_from(i).unwrap_or(0));
            h.sync_state();
        });
    });
    // A double-click copies and returns the focus; it never pastes (D-056).
    w.on_row_activated(|i| {
        with_host(|h| {
            let n = usize::try_from(i).unwrap_or(0);
            h.model.select(n);
            h.sync_state();
            let id = h.model.selected_item().map(|i| i.id);
            let (kind, via) = paste_rules::row_activation();
            h.copy(id, kind, via);
        });
    });
    // The buttons only restore (never paste, whatever the setting).
    w.on_act_copy(|| {
        with_host(|h| {
            let id = h.model.selected_item().map(|i| i.id);
            h.copy(id, CopyKind::Original, Via::Other);
        });
    });
    w.on_act_copy_plain(|| {
        with_host(|h| {
            let id = h.model.selected_item().map(|i| i.id);
            h.copy(id, CopyKind::Plain, Via::Other);
        });
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
    w.on_act_settings(|| {
        super::settings_host::show();
    });
    w.on_act_close(|| {
        with_host(|h| h.act(Action::Close));
    });
    w.on_load_full_text(|| {
        with_host(|h| h.load_full_text());
    });
    w.on_load_full_image(|| {
        with_host(|h| h.load_full_image());
    });
    w.on_shown_until(|last| {
        with_host(|h| h.maybe_load_more(usize::try_from(last).unwrap_or(0)));
    });
}

/// Logs the NFR-01 `perf what=popup_shown` event for a show request.
fn log_popup_shown(asked: Instant, trigger: Trigger) {
    let us = u64::try_from(asked.elapsed().as_micros()).unwrap_or(u64::MAX);
    with_host(|h| {
        h.log.event(
            "perf",
            &[
                ("what", Val::Name("popup_shown")),
                ("src", Val::Name(trigger.name())),
                ("us", Val::Num(us)),
            ],
        );
    });
}

/// The saved language changed (or a status refresh): relabel chips and rows.
pub fn set_lang(lang: Lang) {
    with_host(|h| {
        if h.lang != lang {
            h.lang = lang;
            h.rebuild_chips();
            h.rebuild_rows();
            h.render_detail();
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
            let (x, y) = if PLACE_PREFS.get().0 {
                crate::popup::place::center_origin(size, m.work)
            } else {
                popup_origin(m.cursor, size, m.work, m.dpi)
            };
            w.window().set_position(slint::PhysicalPosition::new(x, y));
        }
    }
    w.set_query(SharedString::new());
    with_host(|h| {
        // Normally done by the hide before; again here so a show can never start from the
        // rows of an earlier session (D-056).
        h.end_session();
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

/// Hides the popup; a delete waiting for its undo is sent now, and the rows, chips and model
/// of this session are forgotten (D-056).
pub fn hide(w: &MainWindow) {
    // A show that never drew a frame is not timed by a later one.
    SHOW_ASKED.set(None);
    with_host(Host::end_session);
    let _ = w.hide();
}

/// Watches the popup's winit window events. Winit keeps one filter per window, so both uses
/// live here:
/// - focus loss closes the popup (the `close_on_focus_loss` setting);
/// - NFR-01 timing: the first `RedrawRequested` after a show request is the frame that makes
///   the popup visible. Slint draws it (render and present, for the software and the GPU
///   renderer alike) right after this filter returns; the timing is taken on the next loop
///   turn, so it includes that frame. This replaces Slint's rendering notifier, which the
///   software renderer does not provide (Task 212). The first `RedrawRequested` after a show
///   is counted even if Slint's `draw()` then skips it (window occluded or minimised), so in
///   that rare case the value is the time to that event, not to a frame on screen.
///
/// With a non-winit backend (a user's `SLINT_BACKEND=qt`) this does nothing.
pub fn watch_window_events(w: &MainWindow) {
    use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
    let ui = w.as_weak();
    w.window().on_winit_window_event(move |_, ev| {
        if let winit::event::WindowEvent::RedrawRequested = ev {
            if let Some((asked, trigger)) = SHOW_ASKED.take() {
                let _ = slint::invoke_from_event_loop(move || log_popup_shown(asked, trigger));
            }
        }
        if let winit::event::WindowEvent::Focused(false) = ev {
            let ui = ui.clone();
            // Not inside winit's event dispatch: hop to the next loop turn.
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = ui.upgrade() {
                    let close = PLACE_PREFS.get().1;
                    PREVIOUS_FOREGROUND.set(paste_rules::previous_after_focus_loss(
                        PREVIOUS_FOREGROUND.get(),
                        close,
                    ));
                    if w.window().is_visible() && close {
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
    SHOW_ASKED.set(None);
    with_host(Host::end_session);
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

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_core::{ClipType, Timestamp};

    const NOW: Timestamp = Timestamp(1_795_320_000_000);

    fn view(n: u32, source: &str) -> ClipSummaryView {
        ClipSummaryView {
            id: ClipId::new_v7(Timestamp(NOW.0 + i64::from(n))),
            kind: ClipType::Text,
            preview: format!("old clip {n}"),
            pinned: false,
            last_copied_at: NOW,
            masked: false,
            source_app: Some(source.to_string()),
            favorite: false,
            tags: Vec::new(),
        }
    }

    fn page(items: Vec<ClipSummaryView>) -> Page<ClipSummaryView> {
        Page {
            items,
            limit: 50,
            next_cursor: None,
        }
    }

    #[test]
    fn after_hide_the_next_show_draws_no_previous_row_before_its_first_page() {
        // A popup session: one page shown, with a source chip, and a refresh still running
        // when the popup hides.
        let mut model = ListModel::new();
        let first = model.search(Filters::default(), NOW, 0);
        assert!(matches!(
            model.apply(
                first.gen,
                false,
                Ok(page(vec![view(1, "editor.exe"), view(2, "editor.exe")]))
            ),
            Applied::Replaced
        ));
        model.select(1);
        let rows = VecModel::<ClipRow>::default();
        rows.set_vec(
            model
                .visible()
                .iter()
                .enumerate()
                .map(|(i, item)| clip_row(item, i, Lang::En, ""))
                .collect::<Vec<_>>(),
        );
        let mut sources = model.sources(MAX_SOURCES);
        let chips = VecModel::<Chip>::default();
        chips.set_vec(chip_rows(model.filters(), &sources, Lang::En));
        let mut focus = ChipFocus::default();
        if let Some(last) = chips.iter().last() {
            focus.report(last.id, true);
        }
        let in_flight = model.refresh(NOW, 0);
        assert_eq!(rows.row_count(), 2);
        assert_eq!(sources, vec!["editor.exe".to_string()]);

        // Hide.
        forget_list(
            &mut model,
            &rows,
            &chips,
            &mut sources,
            &mut focus,
            Lang::En,
        );

        // The next show starts its search; its first page has not arrived yet.
        let _second = model.search(Filters::default(), NOW, 0);
        assert_eq!(
            rows.row_count(),
            0,
            "no row of the previous session is drawn"
        );
        assert!(model.visible().is_empty());
        // Enter and Alt+n have nothing to act on.
        assert!(model.selected_item().is_none());
        assert!(model.nth(0).is_none());
        assert!(sources.is_empty());
        assert!(!focus.focused(false));
        assert!(chips
            .iter()
            .all(|c| !c.label.as_str().contains("editor.exe")));
        // The answer of the refresh that was running at hide is dropped.
        assert!(matches!(
            model.apply(in_flight.gen, false, Ok(page(vec![view(1, "editor.exe")]))),
            Applied::Ignored
        ));
        assert!(model.visible().is_empty());
        assert_eq!(rows.row_count(), 0);
    }
}
