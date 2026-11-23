//! Organisation wiring of the history window (Task 403): the details strip (favorite, note,
//! tags), the collection actions, the marked-clip actions and the one modal dialog. The
//! decisions are the pure view-models in `crate::organize` (validation, plans, key maps); this
//! file mirrors them into Slint and queues the `App` calls on the worker. Everything runs on the
//! Slint event loop thread. Names, tags and notes are user metadata: shown here, never logged
//! (GC-06). Masked clips take no add action (D-060): the plans leave them out.

use super::*;
use crate::organize::bulk::{self, BulkFlow, BulkOp, BulkPlan, Target};
use crate::organize::collections::{self, CollectionDelete};
use crate::organize::dialog::{self, DialogKey, DialogKind, Picker};
use crate::organize::nav::Nav;
use crate::organize::note::NoteEditor;
use crate::organize::tags::{self, TagEditor};
use darkclip_app::api::{GetOptions, TagView};

/// The open dialog.
pub(super) struct Dlg {
    kind: DialogKind,
    note: Option<NoteEditor>,
    tags: Option<TagEditor>,
    picker: Option<Picker>,
    /// The message under the text box.
    error: Option<Msg>,
}

impl Dlg {
    fn new(kind: DialogKind) -> Self {
        Dlg {
            kind,
            note: None,
            tags: None,
            picker: None,
            error: None,
        }
    }
}

/// What the organisation part of the window holds. Dropped when the window closes (D-052).
#[derive(Default)]
pub(super) struct OrgState {
    pub nav: Nav,
    pub cdel: CollectionDelete,
    pub bulk: BulkFlow,
    pub dlg: Option<Dlg>,
    /// The note of the pane's clip, once loaded.
    note: Option<(ClipId, Option<String>)>,
    /// A note load is running for this clip.
    note_pending: Option<ClipId>,
}

/// Sets the static strings of the organisation widgets.
pub(super) fn set_strings(w: &HistoryWindow, lang: Lang) {
    let s = w.global::<OrgStrings>();
    s.set_note_label(t(lang, Msg::OrgNote).into());
    s.set_no_note(t(lang, Msg::OrgNoNote).into());
    s.set_edit_note(t(lang, Msg::OrgEditNote).into());
    s.set_tags_label(t(lang, Msg::OrgGroupTags).into());
    s.set_no_tags(t(lang, Msg::OrgNoTags).into());
    s.set_edit_tags(t(lang, Msg::OrgTagsEdit).into());
    s.set_remove_tag(t(lang, Msg::OrgRemoveTag).into());
    s.set_new_collection(t(lang, Msg::OrgNewCollection).into());
    s.set_rename_collection(t(lang, Msg::OrgRenameCollection).into());
    s.set_delete_collection(t(lang, Msg::OrgDeleteCollection).into());
    s.set_add_to_collection(t(lang, Msg::OrgAddToCollection).into());
    s.set_remove_from_collection(t(lang, Msg::OrgRemoveFromCollection).into());
    s.set_org_hints(t(lang, Msg::OrgKeyHintsHistory).into());
    s.set_collection_keys(t(lang, Msg::OrgCollectionKeys).into());
}

impl Host {
    // ---- state ----

    /// What the selected clips are, for the plans.
    fn target_infos(&self) -> Vec<Target> {
        self.targets()
            .iter()
            .filter_map(|id| self.item(*id))
            .map(|i| Target::from(&i))
            .collect()
    }

    /// A delete, collection delete or bulk confirmation is open.
    pub(super) fn confirming(&self) -> bool {
        self.flow.is_confirming() || self.org.cdel.is_confirming() || self.org.bulk.is_confirming()
    }

    /// Another modal (the "Copy as..." panel, the editor or this dialog) is open.
    fn modal_open(&self) -> bool {
        self.tf.is_some() || self.win.get_edit_open() || self.org.dlg.is_some()
    }

    /// (question, button, key hint) of the open confirmation; empty when there is none.
    pub(super) fn confirm_view(&self) -> (String, String, String) {
        let lang = self.lang;
        let delete = |q: String| {
            (
                q,
                t(lang, Msg::UiDelete).to_string(),
                t(lang, Msg::UiKeyHintsConfirm).to_string(),
            )
        };
        if let Some(n) = self.flow.count() {
            return delete(match n {
                1 => t(lang, Msg::UiConfirmDeleteOne).to_string(),
                n => fill(t(lang, Msg::UiConfirmDeleteMany), n as u64),
            });
        }
        if let Some(q) = self.org.cdel.text(lang) {
            return delete(q);
        }
        if let Some(q) = self.org.bulk.text(lang) {
            return (
                q,
                t(lang, Msg::OrgApply).to_string(),
                t(lang, Msg::OrgKeyHintsConfirm).to_string(),
            );
        }
        (String::new(), String::new(), String::new())
    }

    /// Mirrors the organisation state of the selection into the window.
    pub(super) fn sync_org(&self) {
        let w = &self.win;
        let lang = self.lang;
        let targets = self.target_infos();
        let fav = bulk::favorite_target(&targets);
        w.set_org_can_favorite(fav.is_some());
        let fav_msg = if fav == Some(false) {
            Msg::OrgRemoveFavorite
        } else {
            Msg::OrgAddFavorite
        };
        w.set_org_fav_text(t(lang, fav_msg).into());
        w.set_org_can_add(targets.iter().any(|x| !x.masked));
        w.set_org_can_remove(!targets.is_empty());
        w.set_col_shown(
            self.org
                .nav
                .shown_user_collection(self.model.filters())
                .is_some(),
        );
        match targets.as_slice() {
            [one] => {
                w.set_org_strip(true);
                w.set_org_can_edit(!one.masked);
                w.set_org_favorite(one.favorite);
                let tags: Vec<SharedString> = one.tags.iter().map(|s| s.as_str().into()).collect();
                w.set_org_tags(ModelRc::new(VecModel::from(tags)));
                let note = match &self.org.note {
                    Some((id, n)) if *id == one.id && !one.masked => n.clone().unwrap_or_default(),
                    _ => String::new(),
                };
                w.set_org_note(note.into());
            }
            _ => {
                w.set_org_strip(false);
                w.set_org_can_edit(false);
                w.set_org_favorite(false);
                w.set_org_tags(ModelRc::new(VecModel::<SharedString>::default()));
                w.set_org_note(SharedString::new());
            }
        }
        let (text, label, hint) = self.confirm_view();
        w.set_confirm_text(text.into());
        w.set_confirm_label(label.into());
        w.set_confirm_hint(hint.into());
    }

    /// Loads the note of the single selected clip for the strip (never for a masked one).
    pub(super) fn ensure_note(&mut self) {
        let Some(item) = self.subject() else {
            self.org.note = None;
            return;
        };
        if item.masked {
            self.org.note = None;
            return;
        }
        let loaded = self.org.note.as_ref().is_some_and(|(id, _)| *id == item.id);
        if loaded || self.org.note_pending == Some(item.id) {
            return;
        }
        self.load_note(item.id, false);
    }

    fn load_note(&mut self, id: ClipId, open: bool) {
        self.org.note_pending = Some(id);
        let ui = self.win.as_weak();
        self.poster.post_quiet(move |app| {
            let r = app
                .get_clip(&Caller::Ui, id, GetOptions::default())
                .map(|v| v.clip.note);
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| h.on_note_loaded(id, open, r));
            });
        });
    }

    fn on_note_loaded(&mut self, id: ClipId, open: bool, r: Result<Option<String>, ApiError>) {
        if self.org.note_pending == Some(id) {
            self.org.note_pending = None;
        }
        // A load that finishes after the window closed or the selection moved is dropped.
        if !self.win.window().is_visible() || self.subject().map(|i| i.id) != Some(id) {
            return;
        }
        match r {
            Ok(note) => {
                self.org.note = Some((id, note.clone()));
                if open && !self.modal_open() {
                    let mut d = Dlg::new(DialogKind::Note(id));
                    let editor = NoteEditor::new(note.as_deref());
                    let text = editor.text().to_string();
                    d.note = Some(editor);
                    self.open_dialog(d, &text);
                }
                self.sync_org();
            }
            Err(_) => {
                if open {
                    self.show_toast(t(self.lang, Msg::UiActionFailed).to_string());
                }
            }
        }
    }

    /// The data changed: the strip's note is read again with the refreshed list.
    pub(super) fn org_changed(&mut self) {
        self.org.note = None;
        self.reload_nav();
    }

    /// Drops everything the organisation part holds (the window closed).
    pub(super) fn org_leave(&mut self) {
        self.org = OrgState::default();
        self.win.set_dlg(OrgDialogState::default());
        self.win.set_dlg_open(false);
        self.win.set_dlg_text(SharedString::new());
    }

    // ---- collections and tags in the left pane ----

    pub(super) fn reload_nav(&self) {
        let ui = self.win.as_weak();
        self.poster.post_quiet(move |app| {
            let collections = app.list_collections(&Caller::Ui);
            let tags = app.list_tags(&Caller::Ui);
            if let (Ok(collections), Ok(tags)) = (collections, tags) {
                let nav = Nav { collections, tags };
                let _ = ui.upgrade_in_event_loop(move |_| {
                    with_host(|h| h.on_nav(nav));
                });
            }
        });
    }

    fn on_nav(&mut self, n: Nav) {
        // After the window closed nothing may come back into it.
        if !self.win.window().is_visible() || n == self.org.nav {
            return;
        }
        self.org.nav = n;
        let mut f = self.model.filters().clone();
        let nav = &self.org.nav;
        let col_gone = f
            .collection
            .is_some_and(|c| !nav.collections.iter().any(|x| x.id == c));
        let tag_gone = f
            .tag
            .as_ref()
            .is_some_and(|tag| !nav.tags.iter().any(|x| &x.name == tag));
        if col_gone || tag_gone {
            // The shown collection was deleted (or the last clip lost the tag).
            if col_gone {
                f.collection = None;
            }
            if tag_gone {
                f.tag = None;
            }
            f.text = self.win.get_query().to_string();
            self.search(f);
        } else {
            self.rebuild_filters();
        }
        self.sync_props();
    }

    // ---- the dialog ----

    fn open_dialog(&mut self, d: Dlg, text: &str) {
        self.org.dlg = Some(d);
        self.win.set_dlg_text(text.into());
        self.render_dialog();
    }

    pub(super) fn close_dialog(&mut self) {
        self.org.dlg = None;
        self.win.set_dlg_text(SharedString::new());
        // Chips, names and the counter do not stay in the hidden dialog's property (D-052).
        self.win.set_dlg(OrgDialogState::default());
        self.render_dialog();
        // The dialog's text box had the focus; give it back to the window.
        self.win.invoke_focus_search();
    }

    fn render_dialog(&self) {
        let Some(d) = &self.org.dlg else {
            self.win.set_dlg_open(false);
            return;
        };
        let lang = self.lang;
        let text = self.win.get_dlg_text().to_string();
        let kind = d.kind;
        let chips: Vec<SharedString> = d
            .tags
            .iter()
            .flat_map(|e| e.tags().iter().map(|s| s.as_str().into()))
            .collect();
        let choices: Vec<SharedString> = d
            .picker
            .iter()
            .flat_map(|p| p.labels().into_iter().map(SharedString::from))
            .collect();
        let (counter, counter_bad) = d
            .note
            .as_ref()
            .map_or((String::new(), false), |n| (n.counter(), n.over_limit()));
        let label = match kind {
            DialogKind::NewCollection | DialogKind::RenameCollection(_) => Msg::OrgCollectionName,
            DialogKind::Note(_) => Msg::OrgNote,
            _ => Msg::OrgAddTag,
        };
        let placeholder = match kind {
            DialogKind::Tags(_) | DialogKind::BulkTag => t(lang, Msg::OrgTagPlaceholder),
            DialogKind::NewCollection | DialogKind::RenameCollection(_) => {
                t(lang, Msg::OrgCollectionName)
            }
            _ => "",
        };
        let submit = match kind {
            DialogKind::Tags(_) | DialogKind::BulkTag => Msg::OrgAddTag,
            DialogKind::PickAdd | DialogKind::PickRemove => Msg::OrgApply,
            _ => Msg::OrgSave,
        };
        let cancel = match kind {
            DialogKind::Tags(_) => Msg::UiClose,
            _ => Msg::UiCancel,
        };
        let can_submit = match kind {
            DialogKind::NewCollection | DialogKind::RenameCollection(_) => {
                !text.trim().is_empty() && d.error.is_none()
            }
            DialogKind::Note(_) => d.note.as_ref().is_some_and(NoteEditor::can_save),
            DialogKind::Tags(_) | DialogKind::BulkTag => !text.trim().is_empty(),
            DialogKind::PickAdd | DialogKind::PickRemove => {
                d.picker.as_ref().is_some_and(|p| !p.is_empty())
            }
        };
        let note_error = d
            .note
            .as_ref()
            .filter(|n| n.over_limit())
            .map(|_| Msg::OrgNoteTooLong);
        let error = d.error.or(note_error).map_or("", |m| t(lang, m));
        let state = OrgDialogState {
            title: t(lang, kind.title()).into(),
            hint: t(lang, kind.hint()).into(),
            label: t(lang, label).into(),
            placeholder: placeholder.into(),
            multiline: kind.multiline(),
            has_input: kind.has_input(),
            error: error.into(),
            counter: counter.into(),
            counter_bad,
            chips: ModelRc::new(VecModel::from(chips)),
            choices: ModelRc::new(VecModel::from(choices)),
            selected: d
                .picker
                .as_ref()
                .map_or(0, |p| i32::try_from(p.selected()).unwrap_or(0)),
            empty_text: t(lang, Msg::OrgNoCollections).into(),
            submit_text: t(lang, submit).into(),
            cancel_text: t(lang, cancel).into(),
            can_submit,
        };
        self.win.set_dlg(state);
        self.win.set_dlg_open(true);
    }

    /// A key while the dialog is open. False: the key belongs to the text box.
    pub(super) fn dialog_key(&mut self, key: keys::Key, ctrl: bool) -> bool {
        let Some(d) = &self.org.dlg else {
            return false;
        };
        let empty = self.win.get_dlg_text().is_empty();
        match dialog::dialog_key(d.kind, key, ctrl, empty) {
            DialogKey::Submit => self.dialog_submit(),
            DialogKey::Cancel => self.close_dialog(),
            DialogKey::Up => {
                if let Some(p) = self.org.dlg.as_mut().and_then(|d| d.picker.as_mut()) {
                    p.up();
                }
                self.render_dialog();
            }
            DialogKey::Down => {
                if let Some(p) = self.org.dlg.as_mut().and_then(|d| d.picker.as_mut()) {
                    p.down();
                }
                self.render_dialog();
            }
            DialogKey::RemoveLastTag => {
                let list = self
                    .org
                    .dlg
                    .as_mut()
                    .and_then(|d| d.tags.as_mut())
                    .and_then(TagEditor::remove_last);
                if let (Some(list), Some(DialogKind::Tags(id))) =
                    (list, self.org.dlg.as_ref().map(|d| d.kind))
                {
                    self.apply_tags(id, list);
                }
                self.render_dialog();
            }
            DialogKey::Pass => return false,
        }
        true
    }

    /// The text box changed: validate as the user types and update the counter.
    pub(super) fn dialog_edited(&mut self, text: &str) {
        let names = self.org.nav.names();
        let Some(d) = self.org.dlg.as_mut() else {
            return;
        };
        match d.kind {
            DialogKind::NewCollection | DialogKind::RenameCollection(_) => {
                let except = match d.kind {
                    DialogKind::RenameCollection(id) => Some(id),
                    _ => None,
                };
                d.error = if text.trim().is_empty() {
                    None
                } else {
                    collections::validate_name(text, &names, except).err()
                };
            }
            DialogKind::Note(_) => {
                if let Some(n) = d.note.as_mut() {
                    n.set_text(text);
                }
            }
            DialogKind::Tags(_) | DialogKind::BulkTag => {
                d.error = if text.trim().is_empty() {
                    None
                } else {
                    tags::normalize(text).err()
                };
            }
            DialogKind::PickAdd | DialogKind::PickRemove => {}
        }
        self.render_dialog();
    }

    pub(super) fn dialog_choose(&mut self, i: usize) {
        if let Some(p) = self.org.dlg.as_mut().and_then(|d| d.picker.as_mut()) {
            p.select(i);
        }
        self.dialog_submit();
    }

    pub(super) fn dialog_remove_chip(&mut self, i: usize) {
        let Some(DialogKind::Tags(id)) = self.org.dlg.as_ref().map(|d| d.kind) else {
            return;
        };
        let list = self
            .org
            .dlg
            .as_mut()
            .and_then(|d| d.tags.as_mut())
            .and_then(|e| {
                let name = e.tags().get(i)?.clone();
                e.remove(&name)
            });
        if let Some(list) = list {
            self.apply_tags(id, list);
        }
        self.render_dialog();
    }

    /// Enter or the submit button.
    pub(super) fn dialog_submit(&mut self) {
        let Some(kind) = self.org.dlg.as_ref().map(|d| d.kind) else {
            return;
        };
        let text = self.win.get_dlg_text().to_string();
        match kind {
            DialogKind::NewCollection => self.submit_collection(None, &text),
            DialogKind::RenameCollection(id) => self.submit_collection(Some(id), &text),
            DialogKind::Note(id) => self.submit_note(id),
            DialogKind::Tags(id) => self.submit_tag(id, &text),
            DialogKind::BulkTag => self.submit_bulk_tag(&text),
            DialogKind::PickAdd | DialogKind::PickRemove => self.submit_pick(kind),
        }
    }

    fn set_dialog_error(&mut self, m: Msg) {
        if let Some(d) = self.org.dlg.as_mut() {
            d.error = Some(m);
        }
        self.render_dialog();
    }

    fn submit_collection(&mut self, rename: Option<darkclip_core::CollectionId>, text: &str) {
        let names = self.org.nav.names();
        let name = match collections::validate_name(text, &names, rename) {
            Ok(n) => n,
            Err(m) => return self.set_dialog_error(m),
        };
        self.close_dialog();
        let ui = self.win.as_weak();
        self.poster.post(move |app| {
            let ok = match rename {
                Some(id) => app.rename_collection(&Caller::Ui, id, &name).is_ok(),
                None => app.create_collection(&Caller::Ui, &name).is_ok(),
            };
            let msg = match (ok, rename.is_some()) {
                (false, _) => Msg::UiActionFailed,
                (true, true) => Msg::OrgCollectionRenamed,
                (true, false) => Msg::OrgCollectionCreated,
            };
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| h.show_toast(t(h.lang, msg).to_string()));
            });
        });
    }

    fn submit_note(&mut self, id: ClipId) {
        let value = self
            .org
            .dlg
            .as_ref()
            .and_then(|d| d.note.as_ref())
            .and_then(NoteEditor::save_value);
        let Some(value) = value else {
            return;
        };
        self.close_dialog();
        self.org.note = Some((id, value.clone()));
        self.sync_org();
        let ui = self.win.as_weak();
        self.poster.post(move |app| {
            let ok = app.update_clip_note(&Caller::Ui, id, value).is_ok();
            let msg = if ok {
                Msg::OrgSaved
            } else {
                Msg::UiActionFailed
            };
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| h.show_toast(t(h.lang, msg).to_string()));
            });
        });
    }

    fn submit_tag(&mut self, id: ClipId, text: &str) {
        let added = self
            .org
            .dlg
            .as_mut()
            .and_then(|d| d.tags.as_mut())
            .map(|e| e.add(text));
        match added {
            Some(Ok(list)) => {
                if let Some(d) = self.org.dlg.as_mut() {
                    d.error = None;
                }
                self.win.set_dlg_text(SharedString::new());
                if let Some(list) = list {
                    self.apply_tags(id, list);
                }
                self.render_dialog();
            }
            Some(Err(m)) => self.set_dialog_error(m),
            None => {}
        }
    }

    fn submit_bulk_tag(&mut self, text: &str) {
        let tag = match tags::normalize(text) {
            Ok(t) => t,
            Err(m) => return self.set_dialog_error(m),
        };
        let targets = self.target_infos();
        self.close_dialog();
        self.start_plan(BulkPlan::new(BulkOp::AddTag(tag), &targets));
    }

    fn submit_pick(&mut self, kind: DialogKind) {
        let chosen = self
            .org
            .dlg
            .as_ref()
            .and_then(|d| d.picker.as_ref())
            .and_then(Picker::chosen);
        let Some((id, name)) = chosen else {
            return;
        };
        let targets = self.target_infos();
        self.close_dialog();
        let op = if kind == DialogKind::PickAdd {
            BulkOp::AddToCollection { id, name }
        } else {
            BulkOp::RemoveFromCollection { id, name }
        };
        self.start_plan(BulkPlan::new(op, &targets));
    }

    // ---- tags of one clip ----

    /// Sends the complete tag list of one clip; the answer is what the dialog shows.
    fn apply_tags(&mut self, id: ClipId, list: Vec<String>) {
        let ui = self.win.as_weak();
        self.poster.post(move |app| {
            let r = app.update_clip_tags(&Caller::Ui, id, list);
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| h.on_tags_applied(id, r));
            });
        });
    }

    fn on_tags_applied(&mut self, id: ClipId, r: Result<Vec<TagView>, ApiError>) {
        let known: Option<Vec<String>> = match &r {
            Ok(v) => Some(v.iter().map(|t| t.name.clone()).collect()),
            // The change did not happen: the dialog shows what the list says again.
            Err(_) => self.item(id).map(|i| i.tags),
        };
        if let (Some(known), Some(d)) = (known, self.org.dlg.as_mut()) {
            if d.kind == DialogKind::Tags(id) {
                if let Some(e) = d.tags.as_mut() {
                    e.set(known);
                }
            }
        }
        if r.is_err() {
            self.show_toast(t(self.lang, Msg::UiActionFailed).to_string());
        }
        self.render_dialog();
    }

    /// A chip's remove button in the strip.
    pub(super) fn strip_remove_tag(&mut self, i: usize) {
        let Some(item) = self.subject().filter(|i| !i.masked) else {
            return;
        };
        let Some(name) = item.tags.get(i).cloned() else {
            return;
        };
        if let Some(list) = TagEditor::new(item.tags).remove(&name) {
            self.apply_tags(item.id, list);
        }
    }

    // ---- actions on the selection ----

    fn nothing_to_do(&mut self) {
        self.show_toast(t(self.lang, Msg::OrgNothingToDo).to_string());
    }

    pub(super) fn act_favorite(&mut self) {
        let targets = self.target_infos();
        if targets.is_empty() {
            return;
        }
        let plan = bulk::favorite_target(&targets)
            .and_then(|want| BulkPlan::new(BulkOp::Favorite(want), &targets));
        self.start_plan(plan);
    }

    /// Ctrl+Shift+E (Task 405): exports the collection the list is filtered to, or else the
    /// marked clips (or the clip under the cursor). The user picks the file; masked and
    /// sensitive clips are left out and counted in the message.
    pub(super) fn export_ask(&mut self) {
        use crate::prefs::transfer;
        use darkclip_app::api::ExportSelection;
        use darkclip_core::Clock;
        if self.modal_open() || self.confirming() {
            return;
        }
        let what = match self.model.filters().collection {
            Some(id) => ExportSelection::Collection(id),
            None => {
                let ids = self.targets();
                if ids.is_empty() {
                    return;
                }
                ExportSelection::Clips(ids)
            }
        };
        let lang = self.lang;
        let req = crate::filedlg::Pick {
            save: true,
            title: t(lang, Msg::TransferDialogExport).to_string(),
            filter_name: t(lang, Msg::TransferFilterExport).to_string(),
            ext: transfer::EXPORT_EXT,
            default_name: Some(transfer::export_file_name(crate::SystemClock.now())),
        };
        let poster = self.poster.clone();
        let ui = self.win.as_weak();
        let started = crate::filedlg::pick(req, move |path| {
            let Some(path) = path else {
                return;
            };
            with_host(|h| h.show_toast(t(h.lang, Msg::TransferWorking).to_string()));
            let finish = move |r: Result<darkclip_app::TransferReply, ApiError>| {
                let _ = ui.upgrade_in_event_loop(move |_| {
                    with_host(|h| {
                        let text = match r {
                            Ok(darkclip_app::TransferReply::Export(r)) => {
                                transfer::export_text(h.lang, &r)
                            }
                            Ok(_) => t(h.lang, Msg::TransferFailed).to_string(),
                            Err(e) => t(h.lang, transfer::error_msg(&e)).to_string(),
                        };
                        h.show_toast(text);
                    });
                });
            };
            // A quick send on the worker; the wait runs on its own thread.
            poster.post_quiet(move |app| {
                let start = darkclip_app::UiTransfer::Export { what, dest: path };
                match app.ui_start(&Caller::Ui, start) {
                    Ok(wait) => crate::filedlg::wait_transfer(wait, finish),
                    Err(e) => finish(Err(e)),
                }
            });
        });
        if started == crate::filedlg::Started::Unavailable {
            self.show_toast(t(lang, Msg::TransferNoDialog).to_string());
        }
    }

    /// Runs a plan (several clips: after the confirmation that shows the count).
    fn start_plan(&mut self, plan: Option<BulkPlan>) {
        let Some(plan) = plan else {
            return self.nothing_to_do();
        };
        if plan.needs_confirmation() {
            if self.org.bulk.request(plan) {
                self.sync_props();
            }
        } else {
            self.run_plan(plan);
        }
    }

    pub(super) fn run_plan(&mut self, plan: BulkPlan) {
        let skipped = plan.skipped_text(self.lang);
        let n = plan.count();
        let exact = plan.count_is_exact();
        let ui = self.win.as_weak();
        // Not quiet: the status refresh afterwards re-runs the search and reloads the lists.
        self.poster.post(move |app| {
            let c = Caller::Ui;
            let op = plan.op().clone();
            let failed = plan
                .jobs()
                .iter()
                .filter(|j| {
                    let r = match &op {
                        BulkOp::Favorite(on) => app.set_favorite(&c, j.id, *on),
                        BulkOp::AddToCollection { id, .. } => {
                            app.add_clip_to_collection(&c, j.id, *id)
                        }
                        BulkOp::RemoveFromCollection { id, .. } => {
                            app.remove_clip_from_collection(&c, *id, j.id)
                        }
                        BulkOp::AddTag(_) => {
                            app.update_clip_tags(&c, j.id, j.tags.clone()).map(|_| ())
                        }
                    };
                    r.is_err()
                })
                .count();
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| h.on_plan_done(n, exact, failed, skipped));
            });
        });
    }

    fn on_plan_done(&mut self, n: usize, exact: bool, failed: usize, skipped: Option<String>) {
        let mut msg = if failed > 0 {
            t(self.lang, Msg::UiActionFailed).to_string()
        } else if !exact {
            // Marked clips that were not in the collection are unchanged: no number.
            t(self.lang, Msg::OrgDoneInexact).to_string()
        } else if n == 1 {
            t(self.lang, Msg::OrgSaved).to_string()
        } else {
            fill(t(self.lang, Msg::OrgDone), n as u64)
        };
        if let Some(s) = skipped {
            msg.push(' ');
            msg.push_str(&s);
        }
        self.show_toast(msg);
    }

    /// Enter on a confirmation: delete clips, delete a collection or run the plan.
    pub(super) fn confirm_any(&mut self) {
        if self.flow.is_confirming() {
            self.confirm_delete();
        } else if let Some(id) = self.org.cdel.confirm() {
            self.delete_collection(id);
        } else if let Some(plan) = self.org.bulk.confirm() {
            self.run_plan(plan);
        }
        self.sync_props();
    }

    pub(super) fn cancel_any(&mut self) {
        self.flow.cancel();
        self.org.cdel.cancel();
        self.org.bulk.cancel();
        self.sync_props();
    }

    fn delete_collection(&mut self, id: darkclip_core::CollectionId) {
        // The list was filtered to it: show everything again at once.
        if self.model.filters().collection == Some(id) {
            let mut f = self.model.filters().clone();
            f.collection = None;
            f.text = self.win.get_query().to_string();
            self.search(f);
        }
        let ui = self.win.as_weak();
        self.poster.post(move |app| {
            let ok = app.delete_collection(&Caller::Ui, id).is_ok();
            let msg = if ok {
                Msg::OrgCollectionDeleted
            } else {
                Msg::UiActionFailed
            };
            let _ = ui.upgrade_in_event_loop(move |_| {
                with_host(|h| h.show_toast(t(h.lang, msg).to_string()));
            });
        });
    }

    // ---- opening the dialogs ----

    pub(super) fn open_new_collection(&mut self) {
        if self.modal_open() || self.confirming() {
            return;
        }
        self.open_dialog(Dlg::new(DialogKind::NewCollection), "");
    }

    pub(super) fn open_rename_collection(&mut self) {
        if self.modal_open() || self.confirming() {
            return;
        }
        let Some((id, name)) = self
            .org
            .nav
            .shown_user_collection(self.model.filters())
            .map(|c| (c.id, c.name.clone()))
        else {
            return;
        };
        self.open_dialog(Dlg::new(DialogKind::RenameCollection(id)), &name);
    }

    pub(super) fn request_delete_collection(&mut self) {
        if self.modal_open() || self.confirming() {
            return;
        }
        let Some((id, name, clips, system)) = self
            .org
            .nav
            .shown_user_collection(self.model.filters())
            .map(|c| (c.id, c.name.clone(), c.clip_count, c.system))
        else {
            return;
        };
        if self.org.cdel.request(id, &name, clips, system) {
            self.sync_props();
        }
    }

    pub(super) fn open_note(&mut self) {
        if self.modal_open() || self.confirming() {
            return;
        }
        let Some(item) = self.subject() else {
            return;
        };
        if item.masked {
            return self.nothing_to_do();
        }
        // The strip may have loaded it already.
        if let Some((id, note)) = self.org.note.clone().filter(|(id, _)| *id == item.id) {
            let mut d = Dlg::new(DialogKind::Note(id));
            let editor = NoteEditor::new(note.as_deref());
            let text = editor.text().to_string();
            d.note = Some(editor);
            self.open_dialog(d, &text);
        } else {
            self.load_note(item.id, true);
        }
    }

    pub(super) fn open_tags(&mut self) {
        if self.modal_open() || self.confirming() {
            return;
        }
        let targets = self.target_infos();
        match targets.as_slice() {
            [one] if !one.masked => {
                let mut d = Dlg::new(DialogKind::Tags(one.id));
                d.tags = Some(TagEditor::new(one.tags.clone()));
                self.open_dialog(d, "");
            }
            many if many.len() > 1 && many.iter().any(|t| !t.masked) => {
                self.open_dialog(Dlg::new(DialogKind::BulkTag), "");
            }
            [] => {}
            _ => self.nothing_to_do(),
        }
    }

    pub(super) fn open_pick(&mut self, add: bool) {
        if self.modal_open() || self.confirming() {
            return;
        }
        let targets = self.target_infos();
        if targets.is_empty() {
            return;
        }
        if add && targets.iter().all(|t| t.masked) {
            return self.nothing_to_do();
        }
        let items = self
            .org
            .nav
            .user_collections()
            .into_iter()
            .map(|c| (c.id, c.name.clone()))
            .collect();
        let kind = if add {
            DialogKind::PickAdd
        } else {
            DialogKind::PickRemove
        };
        let mut d = Dlg::new(kind);
        d.picker = Some(Picker::new(items));
        self.open_dialog(d, "");
    }
}

/// Connects the organisation callbacks of a new window.
pub(super) fn connect(w: &HistoryWindow) {
    w.on_act_favorite(|| {
        with_host(|h| h.act_favorite());
    });
    w.on_act_note(|| {
        with_host(|h| h.open_note());
    });
    w.on_act_tags(|| {
        with_host(|h| h.open_tags());
    });
    w.on_act_add_collection(|| {
        with_host(|h| h.open_pick(true));
    });
    w.on_act_remove_collection(|| {
        with_host(|h| h.open_pick(false));
    });
    w.on_act_new_collection(|| {
        with_host(|h| h.open_new_collection());
    });
    w.on_act_rename_collection(|| {
        with_host(|h| h.open_rename_collection());
    });
    w.on_act_delete_collection(|| {
        with_host(|h| h.request_delete_collection());
    });
    w.on_org_remove_tag(|i| {
        with_host(|h| h.strip_remove_tag(usize::try_from(i).unwrap_or(usize::MAX)));
    });
    w.on_dlg_edited(|s| {
        with_host(|h| h.dialog_edited(&s));
    });
    w.on_dlg_submit(|| {
        with_host(|h| h.dialog_submit());
    });
    w.on_dlg_cancel(|| {
        with_host(|h| h.close_dialog());
    });
    w.on_dlg_choose(|i| {
        with_host(|h| h.dialog_choose(usize::try_from(i).unwrap_or(usize::MAX)));
    });
    w.on_dlg_remove_chip(|i| {
        with_host(|h| h.dialog_remove_chip(usize::try_from(i).unwrap_or(usize::MAX)));
    });
}
