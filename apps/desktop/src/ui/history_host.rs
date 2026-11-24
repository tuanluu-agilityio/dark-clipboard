//! Wires the history window to the view-models and the worker (Task 211).
//!
//! The decisions are the popup's pure view-models, reused as they are: filters
//! (`popup::chips`), paging and refresh (`popup::list::ListModel`), the detail pane
//! (`popup::detail`, with the masked rules) and the row text (`popup::row`). What is new and
//! pure lives in `crate::history` (multi-select, the delete confirmation, the keyboard map,
//! window placement). This file only mirrors them into Slint.
//!
//! Everything here runs on the Slint event loop thread; the state lives in a thread-local.
//! `App` calls (search, copy, pin, delete, saving the window rectangle) are queued on the
//! worker with `Poster`; answers return with `upgrade_in_event_loop`. The UI thread never
//! waits for them.
//!
//! Privacy (same rules as the popup): a masked clip never asks for content; when the window
//! closes the rows, the search text, the selection and the detail pane (text, pictures, file
//! names) are dropped, so nothing stays in the hidden window. Nothing is logged here: queries,
//! previews, source names and paths are never logged or formatted into messages (GC-06).
//! Delete never runs without the confirmation dialog (`history::select::DeleteFlow`).

use super::common::{self, clip_row, Px};
use super::{ClipRow, FilterItem, HistoryWindow, OrgDialogState, OrgStrings, Poster, Strings};
use crate::history::keys::{self as hkeys, HistoryAction, HistoryCtx};
use crate::history::select::{can_copy, pin_plan, DeleteFlow, MultiSelect};
use crate::organize::nav;
use crate::popup::chips;
use crate::popup::detail::{DetailModel, DetailRequest};
use crate::popup::keys::{self, ChipFocus, CopyKind, Mods};
use crate::popup::list::{Applied, Filters, Item, ListModel, Request, View};
use crate::popup::row::fill;
use crate::popup::transform::{self as tf, PanelKey, Session};
use darkclip_app::api::{Caller, CopyMode, SaveOutcome};
use darkclip_app::ApiError;
use darkclip_app::{ClipApi, ClipSummaryView, Page};
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::policy::SkipReason;
use darkclip_core::ClipId;
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod org;

/// Typing pause before a search runs.
const DEBOUNCE: Duration = Duration::from_millis(60);
/// A search that runs longer than this shows the loading state.
const LOADING_AFTER: Duration = Duration::from_millis(150);
/// How long a message in the footer stays.
const TOAST_FOR: Duration = Duration::from_secs(4);
/// Source app filters shown at most.
const MAX_SOURCES: usize = 12;

/// What the window needs when it is first opened (it is created on demand).
struct Init {
    poster: Poster,
    lang: Lang,
}

struct Host {
    win: HistoryWindow,
    model: ListModel,
    multi: MultiSelect,
    flow: DeleteFlow,
    rows: Rc<VecModel<ClipRow>>,
    filters: Rc<VecModel<FilterItem>>,
    /// The source names behind the source filters, in order.
    sources: Vec<String>,
    poster: Poster,
    lang: Lang,
    debounce: Timer,
    loading: Timer,
    toast: Timer,
    slow: bool,
    chip_focus: ChipFocus,
    /// The open "Copy as..." panel (Task 404), if any.
    tf: Option<Session>,
    detail: DetailModel<Px>,
    /// The clip the pane wants loaded now (queued jobs for other clips are skipped).
    wanted: Arc<Mutex<Option<ClipId>>>,
    /// Collections, tags, dialogs and confirmations of the organisation (Task 403).
    org: org::OrgState,
}

thread_local! {
    static INIT: RefCell<Option<Init>> = const { RefCell::new(None) };
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    /// The remembered window rectangles per monitor, as the last status refresh read them.
    static REMEMBERED: RefCell<BTreeMap<String, darkclip_app::settings::WindowRect>> =
        const { RefCell::new(BTreeMap::new()) };
}

/// Runs `f` on the host; does nothing when it is missing or already borrowed (re-entry).
fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

fn now_and_offset() -> (darkclip_core::Timestamp, i32) {
    use darkclip_core::Clock;
    (
        crate::SystemClock.now(),
        super::tray_host::local_utc_offset_minutes(),
    )
}

/// Sets the window's strings (creation, language change).
fn set_strings(w: &HistoryWindow, lang: Lang) {
    common::set_shared_strings(w, lang);
    let s = w.global::<Strings>();
    s.set_history_title(t(lang, Msg::UiHistoryTitle).into());
    s.set_select_all(t(lang, Msg::UiSelectAll).into());
    s.set_clear_selection(t(lang, Msg::UiClearSelection).into());
    s.set_cancel(t(lang, Msg::UiCancel).into());
    s.set_history_hint(t(lang, Msg::UiHistoryHint).into());
    s.set_key_hints(t(lang, Msg::UiKeyHintsHistory).into());
    s.set_confirm_hints(t(lang, Msg::UiKeyHintsConfirm).into());
    org::set_strings(w, lang);
}

impl Host {
    /// The listed clips' ids, in list order.
    fn ids(&self) -> Vec<ClipId> {
        self.model.visible().iter().map(|i| i.id).collect()
    }

    fn cursor_id(&self) -> Option<ClipId> {
        self.model.selected_item().map(|i| i.id)
    }

    fn targets(&self) -> Vec<ClipId> {
        self.multi.targets(&self.ids(), self.cursor_id())
    }

    fn item(&self, id: ClipId) -> Option<Item> {
        self.model
            .visible()
            .into_iter()
            .find(|i| i.id == id)
            .cloned()
    }

    /// The one clip the pane describes: the single target, if there is exactly one.
    fn subject(&self) -> Option<Item> {
        match self.targets().as_slice() {
            [id] => self.item(*id),
            _ => None,
        }
    }

    fn row_for(&self, item: &Item, index: usize, marked: bool) -> ClipRow {
        let mut r = clip_row(item, index, self.lang, &self.model.filters().text);
        r.marked = marked;
        r
    }

    /// Rebuilds every row (new results, or the language changed).
    fn rebuild_rows(&self) {
        let visible = self.model.visible();
        let ids: Vec<ClipId> = visible.iter().map(|i| i.id).collect();
        let flags = self.multi.flags(&ids);
        let rows: Vec<ClipRow> = visible
            .iter()
            .enumerate()
            .map(|(i, item)| self.row_for(item, i, flags.get(i).copied().unwrap_or(false)))
            .collect();
        self.rows.set_vec(rows);
    }

    /// Updates the rows whose marked state changed (a selection change keeps the rows).
    fn refresh_marks(&self) {
        let flags = self.multi.flags(&self.ids());
        for (i, on) in flags.into_iter().enumerate() {
            if let Some(mut row) = self.rows.row_data(i) {
                if row.marked != on {
                    row.marked = on;
                    self.rows.set_row_data(i, row);
                }
            }
        }
    }

    fn rebuild_filters(&mut self) {
        self.sources = self.model.sources(MAX_SOURCES);
        let mut specs = chips::history_filters(self.model.filters(), &self.sources, self.lang);
        // Favorites, collections and tags come right after "All" and "Pinned".
        let at = specs.len().min(2);
        let org_chips = nav::history_nav_chips(self.model.filters(), &self.org.nav, self.lang);
        specs.splice(at..at, org_chips);
        let items: Vec<FilterItem> = specs
            .into_iter()
            .map(|c| FilterItem {
                id: c.id,
                label: c.label.into(),
                on: c.on,
                header: c.header,
            })
            .collect();
        // Changed items are updated in place, so a toggled filter keeps the keyboard focus.
        let old: Vec<FilterItem> = self.filters.iter().collect();
        match crate::popup::in_place_changes(&old, &items, |c| (c.id, c.header)) {
            Some(changed) => {
                for i in changed {
                    if let Some(c) = items.get(i) {
                        self.filters.set_row_data(i, c.clone());
                    }
                }
            }
            None => {
                self.chip_focus
                    .retain(|id| items.iter().any(|c| c.id == id && !c.header));
                self.filters.set_vec(items);
            }
        }
    }

    /// Selection, state, footer and detail.
    fn sync(&mut self) {
        self.sync_props();
        self.sync_detail();
    }

    fn sync_props(&self) {
        let w = &self.win;
        let state = match self.model.view(self.slow) {
            View::Rows => 0,
            View::Loading => 1,
            View::EmptyHistory => 2,
            View::NoResults => 3,
            View::Error(_) => 4,
        };
        w.set_view_state(state);
        w.set_selected(i32::try_from(self.model.selected()).unwrap_or(0));
        let targets = self.targets();
        w.set_has_selection(!targets.is_empty());
        w.set_can_copy(can_copy(&targets));
        w.set_can_transform(self.transformable().is_some());
        let pinned: Vec<(ClipId, bool)> = targets
            .iter()
            .filter_map(|id| self.item(*id).map(|i| (i.id, i.pinned)))
            .collect();
        w.set_will_unpin(!pinned.is_empty() && !pin_plan(&pinned).0);
        let summary = if targets.len() > 1 {
            fill(t(self.lang, Msg::UiSelectedCount), targets.len() as u64)
        } else {
            String::new()
        };
        w.set_summary(summary.into());
        self.sync_org();
    }

    fn sync_detail(&mut self) {
        self.ensure_note();
        let subject = self.subject();
        let reqs = self.detail.sync(subject.as_ref());
        for r in reqs {
            self.run_detail(r);
        }
        self.render_detail();
    }

    fn load_full_text(&mut self) {
        if let Some(item) = self.subject() {
            if let Some(r) = self.detail.load_full_text(&item) {
                self.run_detail(r);
            }
        }
        self.render_detail();
    }

    fn load_full_image(&mut self) {
        if let Some(item) = self.subject() {
            if let Some(r) = self.detail.load_full_image(&item) {
                self.run_detail(r);
            }
        }
        self.render_detail();
    }

    /// Runs one load on the worker (see `common::run_detail`).
    fn run_detail(&mut self, r: DetailRequest) {
        let ui = self.win.as_weak();
        common::run_detail(&self.poster, &self.wanted, r, move |a| {
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| {
                    // A load that finishes after the window closed is dropped: no decoded
                    // picture or text may stay in memory behind a hidden window.
                    if !h.win.window().is_visible() {
                        return;
                    }
                    if common::apply_answer(&mut h.detail, a) {
                        h.render_detail();
                    }
                });
            });
        });
    }

    /// Mirrors the detail model into the window's properties.
    fn render_detail(&mut self) {
        let subject = self.subject();
        if let Ok(mut wanted) = self.wanted.lock() {
            *wanted = subject.as_ref().map(|i| i.id);
        }
        match subject {
            Some(item) => {
                self.win.set_detail_open(true);
                let (state, text) =
                    common::render_state(&item, &mut self.detail, self.lang, now_and_offset().1);
                self.win.set_detail(state);
                if let Some(t) = common::set_text_if_changed(&self.win.get_detail_text(), text) {
                    self.win.set_detail_text(t);
                }
            }
            None => {
                // Nothing stays in the pane: no text, picture or file names.
                self.win.set_detail_open(false);
                self.win.set_detail(common::empty_detail());
                self.win.set_detail_text(SharedString::new());
            }
        }
    }

    fn scroll_to_selected(&self) {
        self.win
            .invoke_scroll_to(i32::try_from(self.model.selected()).unwrap_or(0));
    }

    fn run(&mut self, req: Request) {
        let (gen, append) = (req.gen, req.append);
        let query = req.to_query();
        let ui = self.win.as_weak();
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
                            h.sync_props();
                        }
                    });
                });
        }
    }

    /// A new search with `f`: the selection goes back to the first row.
    fn search(&mut self, f: Filters) {
        self.debounce.stop();
        self.multi.plain(None);
        let (now, off) = now_and_offset();
        let req = self.model.search(f, now, off);
        self.run(req);
        self.rebuild_filters();
    }

    fn refresh(&mut self) {
        let (now, off) = now_and_offset();
        let req = self.model.refresh(now, off);
        self.run(req);
    }

    fn show_toast(&mut self, text: String) {
        self.win.set_toast(text.into());
        self.toast.start(TimerMode::SingleShot, TOAST_FOR, || {
            with_host(|h| h.clear_toast());
        });
    }

    fn clear_toast(&mut self) {
        self.toast.stop();
        self.win.set_toast(SharedString::new());
    }

    fn after_move(&mut self) {
        self.refresh_marks();
        self.sync();
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

    fn page_rows(&self) -> usize {
        usize::try_from(self.win.get_page_rows()).unwrap_or(8)
    }

    // ---- actions on the selection ----

    // ---- "Copy as..." and "Edit..." (Task 404) ----

    fn transformable(&self) -> Option<Item> {
        self.subject()
            .filter(|i| tf::can_transform(i.kind, i.masked))
    }

    fn open_copy_as(&mut self) {
        let Some(item) = self.transformable() else {
            return;
        };
        if self.tf.is_some() || self.win.get_edit_open() {
            return;
        }
        self.tf = Some(Session::new(item.id));
        self.win.set_tf_labels(common::transform_labels(self.lang));
        self.win.set_tf_selected(0);
        self.show_preview(common::empty_preview());
        self.win.set_tf_open(true);
        self.request_preview();
    }

    fn show_preview(&self, v: common::PreviewView) {
        self.win.set_tf_preview(v.text);
        self.win.set_tf_note(v.note);
        self.win.set_tf_note_error(v.is_error);
        self.win.set_tf_can_copy(v.can_copy);
    }

    fn request_preview(&mut self) {
        let Some(s) = self.tf.as_mut() else {
            return;
        };
        let (id, transform, gen) = (s.clip(), s.transform(), s.next_gen());
        let ui = self.win.as_weak();
        self.poster.post_quiet(move |app| {
            let r = app.ui_transform_preview(&Caller::Ui, id, transform);
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| {
                    if h.tf.as_ref().is_some_and(|s| s.is_current(gen)) {
                        h.show_preview(common::preview_view(h.lang, &r));
                    }
                });
            });
        });
    }

    fn choose_transform(&mut self, i: usize) {
        let moved = self.tf.as_mut().is_some_and(|s| s.select(i));
        self.after_highlight(moved);
    }

    fn after_highlight(&mut self, moved: bool) {
        if !moved {
            return;
        }
        if let Some(s) = self.tf.as_ref() {
            self.win
                .set_tf_selected(i32::try_from(s.selected()).unwrap_or(0));
            self.show_preview(common::empty_preview());
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

    /// Writes the previewed output to the clipboard (a self-write; nothing is stored).
    fn copy_transformed(&mut self) {
        let Some(s) = self.tf.as_ref() else {
            return;
        };
        if !self.win.get_tf_can_copy() {
            return;
        }
        let (id, transform) = (s.clip(), s.transform());
        let ui = self.win.as_weak();
        self.poster.post_quiet(move |app| {
            let ok = app.ui_copy_transformed(&Caller::Ui, id, transform).is_ok();
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| {
                    h.close_copy_as();
                    let msg = if ok {
                        Msg::TfCopiedAs
                    } else {
                        Msg::UiActionFailed
                    };
                    h.show_toast(t(h.lang, msg).to_string());
                });
            });
        });
    }

    /// Closes the panel; no output text stays in the window.
    fn close_copy_as(&mut self) {
        self.tf = None;
        self.win.set_tf_open(false);
        self.show_preview(common::empty_preview());
    }

    /// Loads the text (on the worker) and opens the editor on a copy of it.
    fn open_edit(&mut self) {
        let Some(item) = self.transformable() else {
            return;
        };
        if self.tf.is_some() || self.win.get_edit_open() {
            return;
        }
        let id = item.id;
        let ui = self.win.as_weak();
        self.poster.post_quiet(move |app| {
            let r = app.ui_edit_text(&Caller::Ui, id);
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| {
                    // The selection moved on while the text was loading: drop the answer.
                    if h.transformable().map(|i| i.id) != Some(id) {
                        return;
                    }
                    match r {
                        Ok(text) => {
                            h.win.set_edit_note(SharedString::new());
                            h.win.set_edit_text(text.into());
                            h.win.set_edit_open(true);
                        }
                        Err(ApiError::InvalidArgument(_)) => {
                            h.show_toast(t(h.lang, Msg::TfEditTooLarge).to_string());
                        }
                        Err(_) => h.show_toast(t(h.lang, Msg::UiActionFailed).to_string()),
                    }
                });
            });
        });
    }

    /// Saves the editor's text as a new clip. A refusal stays in the editor with its reason.
    fn save_edit(&mut self) {
        if !self.win.get_edit_open() {
            return;
        }
        let text = self.win.get_edit_text().to_string();
        let ui = self.win.as_weak();
        // Not quiet: the status refresh afterwards re-runs the search, so the new clip shows.
        self.poster.post(move |app| {
            let r = app.ui_save_edited(&Caller::Ui, text);
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| h.on_saved(r));
            });
        });
    }

    fn on_saved(&mut self, r: Result<SaveOutcome, ApiError>) {
        let msg = match r {
            Ok(SaveOutcome::Exists(id)) => {
                self.close_edit();
                self.show_toast(t(self.lang, Msg::TfAlreadyExists).to_string());
                // Show the clip that already holds this text, if it is in the loaded list.
                if let Some(i) = self.ids().iter().position(|c| *c == id) {
                    self.model.select(i);
                    self.multi.plain(Some(id));
                    self.after_move();
                }
                return;
            }
            Ok(SaveOutcome::Dropped(SkipReason::Private)) => Msg::TfNotSavedPrivate,
            Ok(SaveOutcome::Saved(_)) => {
                self.close_edit();
                self.show_toast(t(self.lang, Msg::TfSaved).to_string());
                return;
            }
            Ok(SaveOutcome::Dropped(SkipReason::Sensitive)) => Msg::TfNotSavedSensitive,
            Ok(SaveOutcome::Dropped(_)) => Msg::TfNotSavedKind,
            Err(ApiError::InvalidArgument("nothing to save")) => Msg::TfNotSavedEmpty,
            Err(ApiError::InvalidArgument("too large")) => Msg::TfNotSavedLarge,
            Err(_) => Msg::UiActionFailed,
        };
        self.win.set_edit_note(t(self.lang, msg).into());
    }

    /// Closes the editor; the text it held is dropped.
    fn close_edit(&mut self) {
        self.win.set_edit_open(false);
        self.win.set_edit_text(SharedString::new());
        self.win.set_edit_note(SharedString::new());
        // The editor's text box had the focus; give it back to the window.
        self.win.invoke_focus_search();
    }

    fn copy(&mut self, plain: bool) {
        let targets = self.targets();
        let [id] = targets.as_slice() else {
            return;
        };
        let id = *id;
        let mode = if plain {
            CopyMode::PlainText
        } else {
            CopyMode::Original
        };
        let ui = self.win.as_weak();
        self.poster.post_quiet(move |app| {
            let ok = app.copy_clip_to_clipboard(&Caller::Ui, id, mode).is_ok();
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| {
                    let msg = if ok {
                        Msg::UiCopied
                    } else {
                        Msg::UiActionFailed
                    };
                    h.show_toast(t(h.lang, msg).to_string());
                });
            });
        });
    }

    fn toggle_pin(&mut self) {
        let pinned: Vec<(ClipId, bool)> = self
            .targets()
            .iter()
            .filter_map(|id| self.item(*id).map(|i| (i.id, i.pinned)))
            .collect();
        let (pin, change) = pin_plan(&pinned);
        if change.is_empty() {
            return;
        }
        let ui = self.win.as_weak();
        // Not quiet: the status refresh afterwards re-runs the search (see `app_changed`).
        self.poster.post(move |app| {
            let failed = change
                .iter()
                .filter(|id| app.pin_clip(&Caller::Ui, **id, pin).is_err())
                .count();
            if failed > 0 {
                let _ = ui.upgrade_in_event_loop(|_| {
                    with_host(|h| h.show_toast(t(h.lang, Msg::UiActionFailed).to_string()));
                });
            }
        });
    }

    /// Opens the confirmation; nothing is deleted yet.
    fn request_delete(&mut self) {
        let targets = self.targets();
        if self.flow.request(targets) {
            self.sync_props();
        }
    }

    /// The user confirmed: the rows leave the list at once and the worker deletes them.
    fn confirm_delete(&mut self) {
        let Some(ids) = self.flow.confirm() else {
            return;
        };
        for id in &ids {
            self.model.hide(*id);
            self.detail.forget(*id);
        }
        let left = self.ids();
        self.multi.prune(&left);
        self.rebuild_rows();
        self.sync();
        let n = ids.len();
        let ui = self.win.as_weak();
        self.poster.post(move |app| {
            let failed: Vec<ClipId> = ids
                .into_iter()
                .filter(|id| app.delete_clip(&Caller::Ui, *id).is_err())
                .collect();
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| h.on_deleted(n, &failed));
            });
        });
    }

    fn on_deleted(&mut self, n: usize, failed: &[ClipId]) {
        for id in failed {
            self.model.unhide(*id);
        }
        let msg = if !failed.is_empty() {
            self.rebuild_rows();
            self.sync();
            t(self.lang, Msg::UiActionFailed).to_string()
        } else if n == 1 {
            t(self.lang, Msg::UiDeleted).to_string()
        } else {
            fill(t(self.lang, Msg::UiDeletedMany), n as u64)
        };
        self.show_toast(msg);
    }

    fn act(&mut self, a: HistoryAction) -> bool {
        match a {
            HistoryAction::Move(m) => {
                let page = self.page_rows();
                self.model.move_by(m, page);
                let id = self.cursor_id();
                self.multi.plain(id);
                self.after_move();
            }
            HistoryAction::MoveKeep(m) => {
                // The cursor moves; the marks stay (Space marks the row under it).
                let page = self.page_rows();
                self.model.move_by(m, page);
                self.after_move();
            }
            HistoryAction::Extend(m) => {
                let before = self.cursor_id();
                let page = self.page_rows();
                self.model.move_by(m, page);
                let ids = self.ids();
                self.multi.extend(&ids, self.model.selected(), before);
                self.after_move();
            }
            HistoryAction::Copy(kind) => self.copy(kind == CopyKind::Plain),
            HistoryAction::Delete => self.request_delete(),
            HistoryAction::TogglePin => self.toggle_pin(),
            HistoryAction::SelectAll => {
                let ids = self.ids();
                self.multi.select_all(&ids);
                self.refresh_marks();
                self.sync();
            }
            HistoryAction::ToggleMark => {
                if let Some(id) = self.cursor_id() {
                    self.multi.toggle(id, Some(id));
                    self.refresh_marks();
                    self.sync();
                }
            }
            HistoryAction::OpenMenu => {
                if self.cursor_id().is_some() {
                    self.win.invoke_open_menu_at_selected();
                }
            }
            HistoryAction::Escape => {
                if self.multi.is_empty() {
                    // Hide outside the borrow (it calls back into the host).
                    let _ = slint::invoke_from_event_loop(hide);
                } else {
                    let id = self.cursor_id();
                    self.multi.plain(id);
                    self.refresh_marks();
                    self.sync();
                }
            }
            HistoryAction::Confirm => self.confirm_any(),
            HistoryAction::Cancel => self.cancel_any(),
            HistoryAction::CopyAs => self.open_copy_as(),
            HistoryAction::Edit => self.open_edit(),
            HistoryAction::ToggleFavorite => self.act_favorite(),
            HistoryAction::EditNote => self.open_note(),
            HistoryAction::EditTags => self.open_tags(),
            HistoryAction::AddToCollection => self.open_pick(true),
            HistoryAction::RemoveFromCollection => self.open_pick(false),
            HistoryAction::NewCollection => self.open_new_collection(),
            HistoryAction::RenameCollection => self.open_rename_collection(),
            HistoryAction::DeleteCollection => self.request_delete_collection(),
            HistoryAction::Export => self.export_ask(),
            HistoryAction::Swallow => {}
        }
        true
    }

    fn toggle_filter(&mut self, id: i32) {
        let mut f = self.model.filters().clone();
        // Typing that has not run yet belongs to this search.
        f.text = self.win.get_query().to_string();
        let toggled = if nav::is_nav(id) {
            nav::toggle_nav(&f, id, &self.org.nav)
        } else {
            chips::toggle(&f, id, &self.sources)
        };
        let Some(f) = toggled else {
            return;
        };
        self.search(f);
    }

    /// A click on row `i`; `ctrl` toggles it, `shift` extends the range.
    fn press_row(&mut self, i: usize, ctrl: bool, shift: bool) {
        let ids = self.ids();
        let Some(id) = ids.get(i).copied() else {
            return;
        };
        let before = self.cursor_id();
        self.model.select(i);
        if shift {
            self.multi.extend(&ids, i, before);
        } else if ctrl {
            self.multi.toggle(id, before);
        } else {
            self.multi.plain(Some(id));
        }
        self.refresh_marks();
        self.sync();
    }

    /// A right click on row `i`: the menu acts on the marked clips when the row is one of
    /// them, otherwise on this row alone.
    fn context_row(&mut self, i: usize) {
        let ids = self.ids();
        let Some(id) = ids.get(i).copied() else {
            return;
        };
        self.model.select(i);
        if !self.multi.contains(id) {
            self.multi.plain(Some(id));
        }
        self.refresh_marks();
        self.sync();
    }

    // ---- showing and hiding ----

    /// Drops everything the window holds: rows, search text, selection, pane, messages.
    fn leave(&mut self) {
        self.close_copy_as();
        self.close_edit();
        self.org_leave();
        self.debounce.stop();
        self.loading.stop();
        self.clear_toast();
        self.save_geometry();
        self.flow.cancel();
        self.multi.plain(None);
        self.model.clear();
        self.rows.set_vec(Vec::new());
        self.win.set_query(SharedString::new());
        self.detail.clear();
        if let Ok(mut wanted) = self.wanted.lock() {
            *wanted = None;
        }
        self.win.set_detail_open(false);
        self.win.set_detail(common::empty_detail());
        self.win.set_detail_text(SharedString::new());
        self.filters.set_vec(Vec::new());
        self.sources.clear();
        self.chip_focus = ChipFocus::default();
        self.sync_props();
    }

    /// Remembers the window's rectangle for the monitor it is on (worker saves it).
    fn save_geometry(&self) {
        #[cfg(windows)]
        {
            use crate::history::place;
            let w = self.win.window();
            if !w.is_visible() || w.is_minimized() || w.is_maximized() {
                return;
            }
            let (pos, size) = (w.position(), w.size());
            let cx = pos
                .x
                .saturating_add(i32::try_from(size.width / 2).unwrap_or(0));
            let cy = pos
                .y
                .saturating_add(i32::try_from(size.height / 2).unwrap_or(0));
            let Some(m) = crate::winsys::monitor_at(cx, cy) else {
                return;
            };
            let key = place::monitor_key(m.full);
            let rect = place::to_saved((pos.x, pos.y), (size.width, size.height), m.dpi);
            let changed = REMEMBERED.with(|r| {
                let mut map = r.borrow_mut();
                if map.get(&key) == Some(&rect) {
                    return false;
                }
                place::remember(&mut map, key.clone(), rect);
                true
            });
            if changed {
                self.poster.post_quiet(move |app| {
                    let mut s = app.settings();
                    place::remember(&mut s.ui.history_windows, key, rect);
                    let _ = app.update_settings(s);
                });
            }
        }
    }
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
                let ids = h.ids();
                h.multi.prune(&ids);
                h.flow.retain(&ids);
                h.org.bulk.retain(&ids);
                h.rebuild_rows();
                h.scroll_to_selected();
            }
            Applied::Appended { from } => {
                let visible = h.model.visible();
                let ids: Vec<ClipId> = visible.iter().map(|i| i.id).collect();
                let flags = h.multi.flags(&ids);
                let new: Vec<ClipRow> = visible
                    .iter()
                    .enumerate()
                    .skip(from)
                    .map(|(i, item)| h.row_for(item, i, flags.get(i).copied().unwrap_or(false)))
                    .collect();
                for r in new {
                    h.rows.push(r);
                }
            }
        }
        h.rebuild_filters();
        h.sync();
        // A short list (e.g. a source filter) keeps loading pages until the view is filled.
        let last_shown = h.model.selected().saturating_add(h.page_rows());
        h.maybe_load_more(last_shown);
    });
}

/// Stores what the window needs to be created later. Call once, on the main thread.
pub fn install(poster: Poster, lang: Lang) {
    INIT.with(|i| *i.borrow_mut() = Some(Init { poster, lang }));
}

/// Creates the window and connects its callbacks (first open only).
fn create() -> Option<()> {
    let Init { poster, lang } = INIT.with(|i| {
        i.borrow().as_ref().map(|i| Init {
            poster: i.poster.clone(),
            lang: i.lang,
        })
    })?;
    let w = HistoryWindow::new().ok()?;
    crate::theme::apply(&w);
    set_strings(&w, lang);
    let rows = Rc::new(VecModel::<ClipRow>::default());
    let filters = Rc::new(VecModel::<FilterItem>::default());
    w.set_rows(ModelRc::from(rows.clone()));
    w.set_filters(ModelRc::from(filters.clone()));
    let mut host = Host {
        win: w.clone_strong(),
        model: ListModel::new(),
        multi: MultiSelect::new(),
        flow: DeleteFlow::new(),
        rows,
        filters,
        sources: Vec::new(),
        poster,
        lang,
        debounce: Timer::default(),
        loading: Timer::default(),
        toast: Timer::default(),
        slow: false,
        chip_focus: ChipFocus::default(),
        tf: None,
        detail: DetailModel::default(),
        wanted: Arc::new(Mutex::new(None)),
        org: org::OrgState::default(),
    };
    host.rebuild_filters();
    host.sync_props();
    HOST.with(|cell| *cell.borrow_mut() = Some(host));
    connect(&w);
    Some(())
}

fn connect(w: &HistoryWindow) {
    w.on_query_edited(|_| {
        with_host(|h| {
            // Debounced: the search runs when typing pauses (each edit restarts the timer).
            h.debounce.start(TimerMode::SingleShot, DEBOUNCE, || {
                with_host(|h| {
                    let mut f = h.model.filters().clone();
                    f.text = h.win.get_query().to_string();
                    h.search(f);
                });
            });
        });
    });
    w.on_filter_focus(|id, on| {
        with_host(|h| h.chip_focus.report(id, on));
    });
    w.on_key(|text, ctrl, shift, alt, meta, search_focused| {
        with_host(|h| {
            let key = keys::key_from_text(&text);
            // The "Copy as..." panel is modal: it takes every key.
            if h.tf.is_some() {
                h.panel_key(tf::panel_key(key));
                return true;
            }
            // The editor takes Esc (cancel) and Ctrl+Enter (save); every other key goes to
            // its text box.
            if h.win.get_edit_open() {
                match key {
                    keys::Key::Escape => h.close_edit(),
                    keys::Key::Enter if ctrl => h.save_edit(),
                    _ => return false,
                }
                return true;
            }
            // The organisation dialog is modal too: Esc, Enter and the picker arrows are its;
            // every other key goes to its text box.
            if h.org.dlg.is_some() {
                return h.dialog_key(key, ctrl);
            }
            let ctx = HistoryCtx {
                query_empty: h.win.get_query().is_empty(),
                search_focused,
                chips_focused: h.chip_focus.focused(search_focused),
                confirming: h.confirming(),
            };
            let mods = Mods {
                ctrl,
                shift,
                alt,
                meta,
            };
            match hkeys::map_key(keys::key_from_text(&text), mods, ctx) {
                Some(a) => h.act(a),
                None => false,
            }
        })
        .unwrap_or(false)
    });
    w.on_filter_toggled(|id| {
        with_host(|h| h.toggle_filter(id));
    });
    w.on_row_pressed(|i, ctrl, shift| {
        with_host(|h| {
            if !h.confirming() {
                h.press_row(usize::try_from(i).unwrap_or(usize::MAX), ctrl, shift);
            }
        });
    });
    w.on_row_context(|i| {
        with_host(|h| {
            if !h.confirming() {
                h.context_row(usize::try_from(i).unwrap_or(usize::MAX));
            }
        });
    });
    w.on_row_activated(|i| {
        with_host(|h| {
            if !h.confirming() {
                h.press_row(usize::try_from(i).unwrap_or(usize::MAX), false, false);
                h.copy(false);
            }
        });
    });
    w.on_act_copy(|| {
        with_host(|h| h.copy(false));
    });
    w.on_act_copy_plain(|| {
        with_host(|h| h.copy(true));
    });
    w.on_act_copy_as(|| {
        with_host(|h| h.open_copy_as());
    });
    w.on_act_edit(|| {
        with_host(|h| h.open_edit());
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
    w.on_edit_save(|| {
        with_host(|h| h.save_edit());
    });
    w.on_edit_cancel(|| {
        with_host(|h| h.close_edit());
    });
    w.on_act_pin(|| {
        with_host(|h| h.toggle_pin());
    });
    w.on_act_delete(|| {
        with_host(|h| h.request_delete());
    });
    w.on_act_select_all(|| {
        with_host(|h| {
            h.act(HistoryAction::SelectAll);
        });
    });
    w.on_act_clear_selection(|| {
        with_host(|h| {
            let id = h.cursor_id();
            h.multi.plain(id);
            h.refresh_marks();
            h.sync();
        });
    });
    w.on_act_confirm(|| {
        with_host(|h| h.confirm_any());
    });
    w.on_act_cancel(|| {
        with_host(|h| h.cancel_any());
    });
    org::connect(w);
    w.on_load_full_text(|| {
        with_host(|h| h.load_full_text());
    });
    w.on_load_full_image(|| {
        with_host(|h| h.load_full_image());
    });
    w.on_shown_until(|last| {
        with_host(|h| h.maybe_load_more(usize::try_from(last).unwrap_or(0)));
    });
    // The close button of the title bar (and Alt+F4): like `hide`, without hiding twice.
    w.window().on_close_requested(|| {
        with_host(|h| h.leave());
        slint::CloseRequestResponse::HideWindow
    });
}

/// The saved language changed (or a status refresh): relabel the window.
pub fn set_lang(lang: Lang) {
    INIT.with(|i| {
        if let Some(i) = i.borrow_mut().as_mut() {
            i.lang = lang;
        }
    });
    with_host(|h| {
        if h.lang != lang {
            h.lang = lang;
            set_strings(&h.win, lang);
            h.rebuild_filters();
            h.rebuild_rows();
            h.sync();
        }
    });
}

/// The remembered window rectangles, from the saved settings (every status refresh).
pub fn set_remembered(map: &BTreeMap<String, darkclip_app::settings::WindowRect>) {
    REMEMBERED.with(|r| {
        let mut r = r.borrow_mut();
        if *r != *map {
            r.clone_from(map);
        }
    });
}

/// The data may have changed (a clip was added, pinned or deleted): refresh a visible window.
pub fn app_changed() {
    with_host(|h| {
        if h.win.window().is_visible() {
            // Collections, tags and the note of the pane's clip may have changed.
            h.org_changed();
            if !h.debounce.running() {
                h.refresh();
            }
        }
    });
}

/// Opens the history window (or brings it to the front when it is already open).
pub fn show() {
    if HOST.with(|h| h.borrow().is_none()) && create().is_none() {
        return;
    }
    let Some(w) = with_host(|h| h.win.clone_strong()) else {
        return;
    };
    if w.window().is_visible() {
        w.window().set_minimized(false);
        focus_os_window(&w);
        w.invoke_focus_search();
        return;
    }
    with_host(|h| {
        h.model.clear();
        h.multi.plain(None);
        h.flow.cancel();
        h.clear_toast();
        h.detail.clear();
        // The pane is always open here: it shows the single selected clip.
        h.detail.set_open(true, None);
        h.render_detail();
        h.reload_nav();
        h.search(Filters::default());
        h.sync();
    });
    w.set_query(SharedString::new());
    place(&w);
    let _ = w.show();
    use slint::winit_030::{winit, WinitWindowAccessor};
    let _ = w
        .window()
        .with_winit_window(|ww| ww.set_theme(Some(winit::window::Theme::Dark)));
    focus_os_window(&w);
    w.invoke_focus_search();
}

/// Puts the window where it was last on the monitor under the cursor (Windows).
fn place(w: &HistoryWindow) {
    #[cfg(windows)]
    {
        use crate::history::place;
        let Some((cx, cy)) = crate::winsys::cursor_pos() else {
            return;
        };
        let Some(m) = crate::winsys::monitor_at(cx, cy) else {
            return;
        };
        let saved = REMEMBERED.with(|r| r.borrow().get(&place::monitor_key(m.full)).copied());
        let p = place::initial(saved, m.work, m.dpi);
        w.window()
            .set_size(slint::PhysicalSize::new(p.width, p.height));
        w.window()
            .set_position(slint::PhysicalPosition::new(p.x, p.y));
    }
    #[cfg(not(windows))]
    {
        let _ = w;
    }
}

/// Brings the window to the front with the keyboard focus.
fn focus_os_window(w: &HistoryWindow) {
    use slint::winit_030::WinitWindowAccessor;
    let _ = w.window().with_winit_window(|ww| ww.focus_window());
}

/// Closes the window: its rectangle is remembered and everything it holds is dropped.
pub fn hide() {
    let Some(w) = with_host(|h| {
        h.leave();
        h.win.clone_strong()
    }) else {
        return;
    };
    let _ = w.hide();
}

/// Before quitting: the window's rectangle is remembered (the worker saves it before it stops).
pub fn shutdown() {
    with_host(|h| {
        h.save_geometry();
        h.debounce.stop();
        h.loading.stop();
        h.toast.stop();
    });
}
