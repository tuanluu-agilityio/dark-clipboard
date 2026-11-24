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

use super::common::{self, history_clip_row, Px};
use super::generated::{IconName, MenuEntry, StatusHint};
use super::{ClipRow, FilterItem, HistoryWindow, OrgDialogState, OrgStrings, Poster, Strings};
use crate::history::keys::{self as hkeys, EscLayer, HistoryAction, HistoryCtx, Layers};
use crate::history::layout::{self, LayoutMode, Region};
use crate::history::menu::{self as hmenu, MenuCtx, RowCmd, RowMenu, RowMenuEvent};
use crate::history::select::{can_copy, pin_plan, DeleteFlow, MultiSelect};
use crate::history::sidebar::{self, Group, SidebarIcon};
use crate::organize::bulk;
use crate::popup::detail::{DetailModel, DetailRequest};
use crate::popup::keys::{self, ChipFocus, CopyKind, Mods};
use crate::popup::list::{Applied, Filters, Item, ListModel, Request, View};
use crate::popup::menu::{self, MenuAction, MenuKey, MenuKind, OpenMenu};
use crate::popup::row::fill;
use crate::popup::transform::{self as tf, PanelKey, Session};
use crate::shortcuts::{sheet_key, SheetKey, Win};
use darkclip_app::api::{Caller, CopyMode, SaveOutcome};
use darkclip_app::ApiError;
use darkclip_app::{ClipApi, ClipSummaryView, Page};
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::policy::SkipReason;
use darkclip_core::{ClipId, CollectionId};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
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
    /// Sidebar groups the user collapsed (UI state only: not a filter, reset on hide).
    collapsed: BTreeSet<Group>,
    /// The open date menu, if any. Holds no text.
    date_menu: Option<OpenMenu>,
    /// The open row menu (Task 704), if any. Holds no text: labels are rebuilt from i18n.
    row_menu: Option<RowMenu>,
    /// The collections of the menu's second level, in order (user data: never logged).
    row_collections: Vec<(CollectionId, String)>,
    /// The window width in logical pixels, as `resized` last reported it.
    width: u32,
    /// Ctrl+B at a wide width collapsed the sidebar to the rail.
    rail_forced: bool,
    /// The full sidebar is open over the list (rail widths).
    overlay_open: bool,
    /// The detail drawer is open (narrow layout).
    drawer_open: bool,
    /// The toolbar's select mode: checkboxes show even with fewer than two rows marked.
    select_mode: bool,
    /// The id of the control that reported the keyboard focus last (`filter-focus`).
    focus_id: Option<i32>,
    /// The F1 shortcuts sheet is open (Task 706). Not persisted; closed when the window hides.
    sheet: bool,
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
    common::set_sheet_strings(w, lang);
    let s = w.global::<Strings>();
    s.set_history_title(t(lang, Msg::UiHistoryTitle).into());
    s.set_select_all(t(lang, Msg::UiSelectAll).into());
    s.set_clear_selection(t(lang, Msg::UiClearSelection).into());
    s.set_cancel(t(lang, Msg::UiCancel).into());
    s.set_history_hint(t(lang, Msg::UiHistoryHint).into());
    s.set_key_hints(t(lang, Msg::UiKeyHintsHistory).into());
    s.set_confirm_hints(t(lang, Msg::UiKeyHintsConfirm).into());
    s.set_hist_tip_date(t(lang, Msg::UiTipDateFilter).into());
    s.set_hist_tip_select(t(lang, Msg::UiTipSelectMode).into());
    s.set_hist_tip_collapse(t(lang, Msg::UiTipGroupCollapse).into());
    s.set_hist_tip_expand(t(lang, Msg::UiTipGroupExpand).into());
    s.set_hist_tip_edit_copy(t(lang, Msg::UiTipEditCopy).into());
    s.set_hist_fact_type(t(lang, Msg::UiGroupType).into());
    s.set_hist_fact_from(t(lang, Msg::UiFactFrom).into());
    s.set_hist_fact_copied(t(lang, Msg::UiFactCopied).into());
    s.set_hist_no_collections(t(lang, Msg::UiNotInCollection).into());
    s.set_hist_key_enter(t(lang, Msg::UiKeyEnter).into());
    s.set_hist_key_shift_enter(t(lang, Msg::UiKeyShiftEnter).into());
    s.set_hist_key_ctrl_t(t(lang, Msg::UiKeyCtrlT).into());
    s.set_hist_key_ctrl_e(t(lang, Msg::UiKeyCtrlE).into());
    s.set_hist_key_ctrl_p(t(lang, Msg::UiKeyCtrlP).into());
    s.set_hist_key_del(t(lang, Msg::UiKeyDel).into());
    s.set_hist_favorite(t(lang, Msg::UiMenuFavorite).into());
    s.set_hist_add_to_collection(t(lang, Msg::HistMenuAddToCollection).into());
    s.set_hist_bulk_tag(t(lang, Msg::HistBulkTag).into());
    s.set_hist_export(t(lang, Msg::HistMenuExport).into());
    s.set_hist_drawer_open(t(lang, Msg::HistDrawerOpen).into());
    s.set_hist_drawer_close(t(lang, Msg::HistDrawerClose).into());
    s.set_hist_check_label(t(lang, Msg::HistCheckLabel).into());
    org::set_strings(w, lang);
}

/// The kit icon of a sidebar row.
fn sidebar_icon(i: SidebarIcon) -> IconName {
    match i {
        SidebarIcon::None | SidebarIcon::All => IconName::Layers,
        SidebarIcon::Pinned => IconName::Pin,
        SidebarIcon::Favorites => IconName::Star,
        SidebarIcon::Kind(k) => common::icon_name(k),
        SidebarIcon::Collection => IconName::Folder,
        SidebarIcon::Tag => IconName::Tag,
        SidebarIcon::App => IconName::ClipboardPaste,
    }
}

/// One sidebar row as the window's model item. The count is only shown where one exists (D-080).
fn filter_item(r: sidebar::SidebarRow) -> FilterItem {
    // The rail shows the icon only: its tooltip and accessible name carry the label and count.
    let tip = match r.count {
        Some(c) if !r.header => format!("{} ({c})", r.label),
        _ => r.label.clone(),
    };
    FilterItem {
        tip: tip.into(),
        id: r.id,
        label: r.label.into(),
        on: r.on,
        header: r.header,
        count: r.count.map_or(-1, |c| i32::try_from(c).unwrap_or(i32::MAX)),
        icon: sidebar_icon(r.icon),
        group: r.group.index(),
        collapsed: r.collapsed,
        badge: r.count.map(|c| c.to_string()).unwrap_or_default().into(),
        add: r.add,
    }
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
        let mut r = history_clip_row(item, index, self.lang, &self.model.filters().text);
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
        let rows = sidebar::sidebar_groups(
            self.model.filters(),
            &self.org.nav,
            &self.sources,
            &self.collapsed,
            self.lang,
        );
        self.win.set_filter_title(
            sidebar::filter_title(
                self.model.filters(),
                &self.org.nav,
                &self.sources,
                self.lang,
            )
            .into(),
        );
        self.win
            .set_date_text(sidebar::date_label(self.model.filters(), self.lang).into());
        let items: Vec<FilterItem> = rows.into_iter().map(filter_item).collect();
        // Changed items are updated in place, so a toggled filter does not rebuild the list.
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
                // Ids below -10 are the controls that report their focus (sidebar, toolbar,
                // sections); they are not rows.
                self.chip_focus.retain(|id| id < -10);
                self.filters.set_vec(items);
            }
        }
    }

    /// Selection, state, footer and detail.
    fn sync(&mut self) {
        self.sync_props();
        self.sync_detail();
    }

    fn sync_props(&mut self) {
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
            common::status_summary(targets.len(), false, self.lang)
        } else {
            String::new()
        };
        w.set_summary(summary.into());
        let marked = self.marked_count();
        w.set_select_mode(self.select_mode || marked >= 1);
        w.set_show_checks(self.select_mode || marked >= 2);
        self.sync_bulk(marked);
        self.sync_status();
        self.sync_org();
    }

    /// How many rows are marked.
    fn marked_count(&self) -> usize {
        self.multi
            .flags(&self.ids())
            .into_iter()
            .filter(|on| *on)
            .count()
    }

    /// The bulk bar: present from two marked rows (`history::menu::bulk_bar`).
    fn sync_bulk(&mut self, marked: usize) {
        match hmenu::bulk_bar(marked) {
            Some(bar) => {
                let infos = self.target_infos();
                let can_favorite = bulk::favorite_target(&infos).is_some();
                let can_add = infos.iter().any(|t| !t.masked);
                let flags: Vec<bool> = bar
                    .buttons
                    .iter()
                    .map(|b| {
                        b.enabled
                            && match b.id {
                                hmenu::BulkId::Favorite => can_favorite,
                                hmenu::BulkId::AddToCollection | hmenu::BulkId::Tags => can_add,
                                _ => true,
                            }
                    })
                    .collect();
                let text = fill(t(self.lang, Msg::HistSelectedCount), bar.count as u64);
                self.win.set_bulk_text(text.into());
                self.win
                    .set_bulk_enabled(ModelRc::new(VecModel::from(flags)));
                self.win.set_bulk_visible(true);
            }
            None => {
                self.win.set_bulk_visible(false);
                self.win.set_bulk_text(SharedString::new());
                self.win
                    .set_bulk_enabled(ModelRc::new(VecModel::<bool>::default()));
                // The bar is gone with whatever button had the focus.
                self.forget_focus(|id| (-136..=-130).contains(&id));
            }
        }
    }

    /// Controls that were removed from the window cannot report that they lost the focus.
    fn forget_focus(&mut self, gone: impl Fn(i32) -> bool) {
        self.chip_focus.retain(|id| !gone(id));
        if self.focus_id.is_some_and(gone) {
            self.focus_id = None;
        }
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
                let facts = common::history_facts(&item, self.lang, now_and_offset().0);
                self.win.set_fact_type(facts.kind.into());
                self.win.set_fact_from(facts.from.into());
                self.win.set_fact_copied(facts.copied.into());
                let (state, text) =
                    common::render_state(&item, &mut self.detail, self.lang, now_and_offset().1);
                self.win.set_detail(state);
                if let Some(t) = common::set_text_if_changed(&self.win.get_detail_text(), text) {
                    self.win.set_detail_text(t);
                }
            }
            None => {
                // Nothing stays in the pane: no text, picture, file names or facts.
                self.win.set_detail_open(false);
                self.win.set_fact_type(SharedString::new());
                self.win.set_fact_from(SharedString::new());
                self.win.set_fact_copied(SharedString::new());
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
                if self.cursor_id().is_some() && self.prepare_row_menu() {
                    self.win.invoke_open_menu_at_selected();
                }
            }
            // One layer per press: the row menu, the sidebar overlay, the drawer, the marks and
            // last the window.
            HistoryAction::Escape => {
                let has_marks = !self.multi.is_empty() || self.select_mode;
                match hkeys::esc_layer(self.layers(), has_marks) {
                    EscLayer::Menu => self.close_row_menu(),
                    EscLayer::Overlay => self.close_overlay(),
                    EscLayer::Drawer => self.close_drawer(),
                    EscLayer::Marks => self.clear_marks(),
                    // Hide outside the borrow (it calls back into the host).
                    EscLayer::Window => {
                        let _ = slint::invoke_from_event_loop(hide);
                    }
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
            HistoryAction::ToggleSidebar => self.toggle_sidebar(),
            HistoryAction::FocusSearch => self.win.invoke_focus_search(),
            HistoryAction::OpenSettings => {
                let _ = slint::invoke_from_event_loop(super::settings_host::show);
            }
            HistoryAction::NextRegion => self.cycle_region(false),
            HistoryAction::PrevRegion => self.cycle_region(true),
            HistoryAction::OpenDrawer => self.open_drawer(),
            HistoryAction::CloseDrawer => self.close_drawer(),
            HistoryAction::Shortcuts => self.show_shortcuts(),
        }
        true
    }

    /// F1 and Ctrl+/: the shortcuts sheet (Task 706). The key map never gives this action while
    /// a dialog, the "Copy as..." panel or the editor has the keys.
    fn show_shortcuts(&mut self) {
        if self.modal_open() || self.confirming() {
            return;
        }
        self.sheet = true;
        self.render_sheet();
    }

    /// Draws the sheet's state: its two columns in the current language and whether it is open.
    fn render_sheet(&self) {
        if self.sheet {
            let (left, right) = common::sheet_columns(Win::History, self.lang);
            self.win.set_sheet_left(left);
            self.win.set_sheet_right(right);
        }
        self.win.set_sheet_open(self.sheet);
    }

    fn close_sheet(&mut self) {
        if self.sheet {
            self.sheet = false;
            self.render_sheet();
        }
    }

    fn toggle_filter(&mut self, id: i32) {
        // Typing that has not run yet belongs to this search.
        let f = self.current_filters();
        let Some(f) = sidebar::toggle(&f, id, &self.org.nav, &self.sources) else {
            return;
        };
        self.search(f);
        // A filter chosen in the overlay: show the list it filtered.
        self.close_overlay();
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
        self.close_sheet();
        self.date_menu = None;
        self.row_menu = None;
        self.drawer_open = false;
        self.overlay_open = false;
        self.rail_forced = false;
        self.select_mode = false;
        self.focus_id = None;
        self.apply_layout();
        self.collapsed = sidebar::default_collapsed();
        self.win.invoke_reset_ui();
        self.win.set_filter_title(SharedString::new());
        self.win.set_date_text(SharedString::new());
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
        self.win.set_fact_type(SharedString::new());
        self.win.set_fact_from(SharedString::new());
        self.win.set_fact_copied(SharedString::new());
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

// ---- Responsive layout, regions and the row menu (Task 704) ----

/// The kit icon of a row menu entry.
fn menu_icon(cmd: RowCmd) -> IconName {
    match cmd {
        RowCmd::Copy | RowCmd::CopyPlain => IconName::Copy,
        RowCmd::CopyAs => IconName::WandSparkles,
        RowCmd::Edit => IconName::Pencil,
        RowCmd::TogglePin => IconName::Pin,
        RowCmd::ToggleFavorite => IconName::Star,
        RowCmd::AddToCollection => IconName::FolderPlus,
        RowCmd::RemoveFromCollection => IconName::Folder,
        RowCmd::Tags => IconName::Tag,
        RowCmd::Note => IconName::StickyNote,
        RowCmd::Export => IconName::Upload,
        RowCmd::SelectAll => IconName::ListChecks,
        RowCmd::Delete => IconName::Trash2,
    }
}

impl Host {
    /// The layers that change what a key means.
    fn layers(&self) -> Layers {
        Layers {
            narrow: layout::geometry(self.width, self.rail_forced).drawer,
            drawer_open: self.drawer_open,
            overlay_open: self.overlay_open,
            menu_open: self.row_menu.is_some(),
        }
    }

    /// Hands the pane widths to the window (resize, Ctrl+B, show). Layers that no longer fit
    /// close (an overlay at wide widths, a drawer beside the list), and controls that left the
    /// window stop counting as focused.
    fn apply_layout(&mut self) {
        let g = layout::geometry(self.width, self.rail_forced);
        if !g.drawer {
            self.drawer_open = false;
        }
        if !g.rail {
            self.overlay_open = false;
        }
        let w = &self.win;
        w.set_sidebar_w(g.sidebar_w as f32);
        w.set_detail_w(g.detail_w as f32);
        w.set_compact(g.rail);
        w.set_drawer_mode(g.drawer);
        w.set_drawer_open(self.drawer_open);
        w.set_overlay_open(self.overlay_open);
        if !g.drawer {
            self.forget_focus(|id| id == -103 || id == -140);
        } else if !self.drawer_open {
            self.forget_focus(|id| id == -140 || id == -110 || (-122..=-120).contains(&id));
        }
        if !self.overlay_open {
            self.forget_focus(|id| id == -52);
        }
    }

    fn toggle_sidebar(&mut self) {
        if layout::layout_mode(self.width) == LayoutMode::Wide {
            self.rail_forced = !self.rail_forced;
            self.overlay_open = false;
            self.apply_layout();
        } else if self.overlay_open {
            self.close_overlay();
        } else {
            self.overlay_open = true;
            self.apply_layout();
        }
    }

    fn close_overlay(&mut self) {
        if !self.overlay_open {
            return;
        }
        self.overlay_open = false;
        self.apply_layout();
        self.win.invoke_focus_region(1);
    }

    fn open_drawer(&mut self) {
        if !layout::geometry(self.width, self.rail_forced).drawer || self.drawer_open {
            return;
        }
        self.drawer_open = true;
        self.apply_layout();
    }

    fn close_drawer(&mut self) {
        if !self.drawer_open {
            return;
        }
        self.drawer_open = false;
        self.apply_layout();
        self.win.invoke_focus_region(1);
    }

    /// F6 and Shift+F6: the next (or previous) region that is on screen.
    fn cycle_region(&mut self, back: bool) {
        if self.overlay_open {
            self.close_overlay();
        }
        let g = layout::geometry(self.width, self.rail_forced);
        let from = layout::region_of_focus(self.focus_id);
        let to = layout::next_region(g.mode, self.drawer_open, from, back);
        let index = match to {
            Region::Sidebar => 0,
            Region::List => 1,
            Region::Detail => 2,
        };
        self.win.invoke_focus_region(index);
    }

    // ---- the row menu (D-079: the host owns the state, Slint only draws) ----

    /// What the menu entries depend on, from the same flags the toolbar uses.
    fn menu_ctx(&self) -> MenuCtx {
        let targets = self.target_infos();
        let ids: Vec<ClipId> = targets.iter().map(|t| t.id).collect();
        let pinned: Vec<(ClipId, bool)> = ids
            .iter()
            .filter_map(|id| self.item(*id).map(|i| (i.id, i.pinned)))
            .collect();
        let fav = bulk::favorite_target(&targets);
        MenuCtx {
            can_copy: can_copy(&ids),
            can_transform: self.transformable().is_some(),
            has_selection: !ids.is_empty(),
            will_unpin: !pinned.is_empty() && !pin_plan(&pinned).0,
            will_unfavorite: fav == Some(false),
            can_favorite: fav.is_some(),
            can_add: targets.iter().any(|t| !t.masked),
            can_remove: !targets.is_empty(),
            can_edit: matches!(targets.as_slice(), [one] if !one.masked),
            can_export: !targets.is_empty() || self.model.filters().collection.is_some(),
        }
    }

    /// Builds the menu for the current selection and fills the window's entries. False when a
    /// dialog or confirmation is open (no menu then).
    fn prepare_row_menu(&mut self) -> bool {
        if self.confirming() || self.modal_open() {
            return false;
        }
        let items = hmenu::history_menu_items(&self.menu_ctx());
        self.row_collections = self.collection_choices();
        self.row_menu = Some(RowMenu::new(items, self.row_collections.len()));
        self.render_row_menu();
        true
    }

    /// Mirrors the open menu (level, entries, highlight) into the window.
    fn render_row_menu(&self) {
        let Some(m) = self.row_menu.as_ref() else {
            return;
        };
        let (entries, disabled): (Vec<MenuEntry>, Vec<bool>) = if m.level() == 0 {
            m.items()
                .iter()
                .map(|i| {
                    (
                        MenuEntry {
                            label: t(self.lang, i.label).into(),
                            icon: menu_icon(i.cmd),
                            shortcut: i.key.map(|k| t(self.lang, k).into()).unwrap_or_default(),
                            danger: i.danger,
                            submenu: i.submenu,
                            checked: false,
                        },
                        !i.enabled,
                    )
                })
                .unzip()
        } else {
            self.row_collections
                .iter()
                .map(|(_, name)| {
                    (
                        MenuEntry {
                            label: name.as_str().into(),
                            icon: IconName::Folder,
                            shortcut: SharedString::new(),
                            danger: false,
                            submenu: false,
                            checked: false,
                        },
                        false,
                    )
                })
                .unzip()
        };
        self.win
            .set_row_entries(ModelRc::new(VecModel::from(entries)));
        self.win
            .set_row_disabled(ModelRc::new(VecModel::from(disabled)));
        self.win
            .set_row_highlighted(i32::try_from(m.highlighted()).unwrap_or(0));
    }

    fn close_row_menu(&mut self) {
        self.row_menu = None;
        self.win.invoke_close_row_menu();
    }

    fn set_row_highlight(&mut self, i: usize) {
        if let Some(m) = self.row_menu.as_mut() {
            m.set_highlight(i);
            self.win
                .set_row_highlighted(i32::try_from(m.highlighted()).unwrap_or(0));
        }
    }

    /// One key of the open row menu. True when it was handled.
    fn row_menu_key(&mut self, text: &str) -> bool {
        let Some(m) = self.row_menu.as_ref() else {
            return false;
        };
        // The texts of the current level, for the letter jump.
        let labels: Vec<String> = if m.level() == 0 {
            m.items()
                .iter()
                .map(|i| t(self.lang, i.label).to_string())
                .collect()
        } else {
            self.row_collections.iter().map(|c| c.1.clone()).collect()
        };
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let ev = match self.row_menu.as_mut() {
            Some(m) => m.key(menu::menu_key_from_text(text), &refs),
            None => return false,
        };
        self.row_menu_event(ev)
    }

    /// An entry was clicked (the kit closed the menu already unless it opens a second level).
    fn row_menu_activated(&mut self, i: usize) {
        let ev = match self.row_menu.as_mut() {
            Some(m) => m.activate(i),
            None => return,
        };
        self.row_menu_event(ev);
    }

    fn row_menu_event(&mut self, ev: RowMenuEvent) -> bool {
        match ev {
            RowMenuEvent::Handled => self.render_row_menu(),
            RowMenuEvent::Unhandled => return false,
            RowMenuEvent::Close => self.close_row_menu(),
            RowMenuEvent::Run(cmd) => {
                self.close_row_menu();
                self.run_row_cmd(cmd);
            }
            RowMenuEvent::AddTo(i) => {
                self.close_row_menu();
                if let Some((id, name)) = self.row_collections.get(i).cloned() {
                    self.add_to_collection_at(id, name);
                }
            }
        }
        true
    }

    /// Runs a menu entry: the same host methods the toolbar buttons and the keys call.
    fn run_row_cmd(&mut self, cmd: RowCmd) {
        match cmd {
            RowCmd::Copy => self.copy(false),
            RowCmd::CopyPlain => self.copy(true),
            RowCmd::CopyAs => self.open_copy_as(),
            RowCmd::Edit => self.open_edit(),
            RowCmd::TogglePin => self.toggle_pin(),
            RowCmd::ToggleFavorite => self.act_favorite(),
            RowCmd::AddToCollection => self.open_pick(true),
            RowCmd::RemoveFromCollection => self.open_pick(false),
            RowCmd::Tags => self.open_tags(),
            RowCmd::Note => self.open_note(),
            RowCmd::Export => self.export_ask(),
            RowCmd::SelectAll => {
                self.act(HistoryAction::SelectAll);
            }
            RowCmd::Delete => self.request_delete(),
        }
    }
}

// ---- Status line, date menu and sidebar groups (Task 703) ----

impl Host {
    /// "128 clips" and at most two key hints (only keys that exist: Space marks, Menu opens the
    /// context menu).
    fn sync_status(&self) {
        let n = self.model.visible().len();
        self.win
            .set_status_text(common::status_summary(n, false, self.lang).into());
        let hint = |key: Msg, label: Msg| StatusHint {
            key: t(self.lang, key).into(),
            label: t(self.lang, label).into(),
        };
        let hints = vec![
            hint(Msg::UiKeySpace, Msg::UiHintMark),
            hint(Msg::UiKeyMenu, Msg::UiHintActions),
        ];
        self.win
            .set_status_hints(ModelRc::new(VecModel::from(hints)));
    }

    /// A group header was clicked or Enter / Left / Right pressed on it: collapse or expand.
    fn toggle_group(&mut self, group: i32) {
        let Some(g) = Group::from_index(group) else {
            return;
        };
        if !self.collapsed.remove(&g) {
            self.collapsed.insert(g);
        }
        self.rebuild_filters();
    }

    /// The select-mode toggle: marks the clip under the cursor (like Space), or clears the marks.
    fn toggle_select_mode(&mut self) {
        if self.confirming() {
            return;
        }
        if self.multi.is_empty() && !self.select_mode {
            self.select_mode = true;
            self.act(HistoryAction::ToggleMark);
            self.sync_props();
        } else {
            self.clear_marks();
        }
    }

    /// Esc, the bulk bar's Clear and the toolbar toggle (when on): the marks go, select mode ends.
    fn clear_marks(&mut self) {
        self.select_mode = false;
        let id = self.cursor_id();
        self.multi.plain(id);
        self.refresh_marks();
        self.sync();
    }

    /// The filters as they are now, with the search text the user has typed so far.
    fn current_filters(&self) -> Filters {
        let mut f = self.model.filters().clone();
        f.text = self.win.get_query().to_string();
        f
    }

    // ---- the toolbar's date menu (D-079: the host owns the state, Slint only draws) ----

    fn date_entries(&self) -> Vec<MenuEntry> {
        let active = sidebar::date_active(self.model.filters());
        sidebar::DATE_ITEMS
            .iter()
            .enumerate()
            .map(|(i, m)| MenuEntry {
                label: t(self.lang, *m).into(),
                icon: IconName::Calendar,
                shortcut: SharedString::new(),
                danger: false,
                submenu: false,
                checked: i == active,
            })
            .collect()
    }

    fn open_date_menu(&mut self) {
        if self.confirming() || self.modal_open() {
            return;
        }
        let entries = self.date_entries();
        let active = sidebar::date_active(self.model.filters());
        let mut m = OpenMenu::new(MenuKind::More, entries.len());
        m.highlighted = active;
        self.date_menu = Some(m);
        self.win
            .set_date_entries(ModelRc::new(VecModel::from(entries)));
        self.win
            .set_date_highlighted(i32::try_from(active).unwrap_or(0));
        self.win.invoke_open_date_menu();
    }

    /// Closes the menu and forgets its state.
    fn close_date_menu(&mut self) {
        self.date_menu = None;
        self.win.invoke_close_date_menu();
    }

    fn set_date_highlight(&mut self, i: usize) {
        if let Some(m) = self.date_menu.as_mut() {
            m.highlighted = i;
            self.win.set_date_highlighted(i32::try_from(i).unwrap_or(0));
        }
    }

    /// One key of the open date menu. True when it was handled.
    fn date_menu_key(&mut self, text: &str) -> bool {
        let Some(m) = self.date_menu else {
            return false;
        };
        let action = match menu::menu_key_from_text(text) {
            MenuKey::Letter(c) => {
                let labels: Vec<String> = self
                    .win
                    .get_date_entries()
                    .iter()
                    .map(|e| e.label.to_string())
                    .collect();
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                menu::letter_jump(&refs, m.highlighted, c)
                    .map_or(MenuAction::None, MenuAction::Move)
            }
            k => menu::menu_key(k, m.highlighted, m.count),
        };
        match action {
            MenuAction::Move(i) => self.set_date_highlight(i),
            MenuAction::Activate(i) => self.choose_date(i),
            MenuAction::Close => self.close_date_menu(),
            // One level only: Left and Right do nothing here, but the menu keeps the key.
            MenuAction::Back | MenuAction::Submenu(_) => {}
            MenuAction::None => return false,
        }
        true
    }

    /// An item of the date menu was chosen (key or click): the same toggle as the old chips.
    fn choose_date(&mut self, i: usize) {
        self.close_date_menu();
        if let Some(f) = sidebar::choose_date(&self.current_filters(), i) {
            self.search(f);
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
        collapsed: sidebar::default_collapsed(),
        date_menu: None,
        row_menu: None,
        row_collections: Vec::new(),
        width: crate::history::place::DEFAULT_LOGICAL.0,
        rail_forced: false,
        overlay_open: false,
        drawer_open: false,
        select_mode: false,
        focus_id: None,
        sheet: false,
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
        with_host(|h| {
            h.chip_focus.report(id, on);
            if on {
                h.focus_id = Some(id);
            } else if h.focus_id == Some(id) {
                h.focus_id = None;
            }
        });
    });
    w.on_resized(|width| {
        with_host(|h| {
            h.width = u32::try_from(width).unwrap_or(0);
            h.apply_layout();
        });
    });
    w.on_act_open_drawer(|| {
        with_host(Host::open_drawer);
    });
    w.on_act_close_drawer(|| {
        with_host(Host::close_drawer);
    });
    w.on_overlay_dismissed(|| {
        with_host(Host::close_overlay);
    });
    w.on_act_clear_marks(|| {
        with_host(Host::clear_marks);
    });
    w.on_act_export(|| {
        with_host(Host::export_marked);
    });
    w.on_row_checked(|i| {
        with_host(|h| {
            if !h.confirming() {
                h.press_row(usize::try_from(i).unwrap_or(usize::MAX), true, false);
            }
        });
    });
    w.on_row_menu_requested(|x, y| {
        with_host(|h| {
            if h.prepare_row_menu() {
                h.win.invoke_open_row_menu(x, y);
            }
        });
    });
    w.on_row_menu_key(|text| with_host(|h| h.row_menu_key(&text)).unwrap_or(false));
    w.on_row_menu_hovered(|i| {
        with_host(|h| h.set_row_highlight(usize::try_from(i).unwrap_or(0)));
    });
    w.on_row_menu_activated(|i| {
        with_host(|h| h.row_menu_activated(usize::try_from(i).unwrap_or(usize::MAX)));
    });
    w.on_key(|text, ctrl, shift, alt, meta, search_focused| {
        with_host(|h| {
            // Keys go to an open menu, never here: a menu still recorded was closed by a click
            // outside, so forget it.
            h.date_menu = None;
            h.row_menu = None;
            let key = hkeys::history_key(&text);
            // The shortcuts sheet is modal: Esc, F1 and Ctrl+/ close it, every other key is
            // swallowed (so Esc closes the sheet before the window).
            if h.sheet {
                let mods = Mods {
                    ctrl,
                    shift,
                    alt,
                    meta,
                };
                if sheet_key(true, key, mods) == SheetKey::Close {
                    h.close_sheet();
                }
                return true;
            }
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
            match hkeys::map_key_in(key, mods, ctx, h.layers()) {
                Some(a) => h.act(a),
                None => false,
            }
        })
        .unwrap_or(false)
    });
    w.on_filter_toggled(|id| {
        with_host(|h| h.toggle_filter(id));
    });
    w.on_group_toggled(|g| {
        with_host(|h| h.toggle_group(g));
    });
    w.on_act_select_mode(|| {
        with_host(Host::toggle_select_mode);
    });
    w.on_date_menu_requested(|| {
        with_host(Host::open_date_menu);
    });
    w.on_date_menu_key(|text| with_host(|h| h.date_menu_key(&text)).unwrap_or(false));
    w.on_date_menu_hovered(|i| {
        with_host(|h| h.set_date_highlight(usize::try_from(i).unwrap_or(0)));
    });
    w.on_date_menu_activated(|i| {
        with_host(|h| h.choose_date(usize::try_from(i).unwrap_or(usize::MAX)));
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
    w.on_sheet_close(|| {
        with_host(Host::close_sheet);
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
            h.render_sheet();
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
    with_host(|h| {
        h.width = u32::try_from(h.win.get_win_width()).unwrap_or(0);
        h.apply_layout();
    });
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
