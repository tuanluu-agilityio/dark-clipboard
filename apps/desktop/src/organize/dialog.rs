//! The organisation dialog's state and key map (Task 403). One modal dialog serves every flow:
//! new or renamed collection (a name), a note (several lines), the tags of one clip (chips and an
//! input) and the pickers for "add to collection" and "remove from collection". Pure.

use crate::popup::keys::Key;
use darkclip_core::i18n::Msg;
use darkclip_core::{ClipId, CollectionId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    NewCollection,
    RenameCollection(CollectionId),
    Note(ClipId),
    /// The chips of one clip.
    Tags(ClipId),
    /// One tag for the marked clips.
    BulkTag,
    PickAdd,
    PickRemove,
}

impl DialogKind {
    /// The dialog has a text box.
    pub fn has_input(self) -> bool {
        !matches!(self, DialogKind::PickAdd | DialogKind::PickRemove)
    }

    /// The text box takes several lines (the note).
    pub fn multiline(self) -> bool {
        matches!(self, DialogKind::Note(_))
    }

    /// The title.
    pub fn title(self) -> Msg {
        match self {
            DialogKind::NewCollection => Msg::OrgNewCollection,
            DialogKind::RenameCollection(_) => Msg::OrgRenameCollection,
            DialogKind::Note(_) => Msg::OrgNote,
            DialogKind::Tags(_) => Msg::OrgTagsEdit,
            DialogKind::BulkTag => Msg::OrgAddTag,
            DialogKind::PickAdd => Msg::OrgAddToCollection,
            DialogKind::PickRemove => Msg::OrgRemoveFromCollection,
        }
    }

    /// The key hint under the dialog.
    pub fn hint(self) -> Msg {
        match self {
            DialogKind::Note(_) => Msg::OrgNoteHint,
            DialogKind::Tags(_) => Msg::OrgTagsHint,
            DialogKind::PickAdd | DialogKind::PickRemove => Msg::OrgPickHint,
            DialogKind::NewCollection | DialogKind::RenameCollection(_) | DialogKind::BulkTag => {
                Msg::OrgKeyHintsConfirm
            }
        }
    }
}

/// What a key does while the dialog is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKey {
    Submit,
    Cancel,
    Up,
    Down,
    /// Backspace in an empty tag box removes the last chip.
    RemoveLastTag,
    /// The key belongs to the focused text box (or does nothing).
    Pass,
}

/// The meaning of a key for the open dialog. `input_empty`: the text box holds nothing.
pub fn dialog_key(kind: DialogKind, key: Key, ctrl: bool, input_empty: bool) -> DialogKey {
    let picker = !kind.has_input();
    match key {
        Key::Escape => DialogKey::Cancel,
        Key::Enter if matches!(kind, DialogKind::Note(_)) => {
            if ctrl {
                DialogKey::Submit
            } else {
                DialogKey::Pass
            }
        }
        Key::Enter => DialogKey::Submit,
        Key::Up if picker => DialogKey::Up,
        Key::Down if picker => DialogKey::Down,
        Key::Char('\u{8}') if !ctrl && input_empty && matches!(kind, DialogKind::Tags(_)) => {
            DialogKey::RemoveLastTag
        }
        _ => DialogKey::Pass,
    }
}

/// The list of a picker dialog and its highlighted row.
#[derive(Clone, PartialEq, Eq)]
pub struct Picker {
    items: Vec<(CollectionId, String)>,
    selected: usize,
}

/// Never prints names.
impl std::fmt::Debug for Picker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picker")
            .field("items", &self.items.len())
            .field("selected", &self.selected)
            .finish()
    }
}

impl Picker {
    pub fn new(items: Vec<(CollectionId, String)>) -> Self {
        Self { items, selected: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn down(&mut self) {
        if self.selected + 1 < self.items.len() {
            self.selected += 1;
        }
    }

    /// A click on row `i`; a row that does not exist changes nothing.
    pub fn select(&mut self, i: usize) {
        if i < self.items.len() {
            self.selected = i;
        }
    }

    /// The highlighted collection, or `None` for an empty list.
    pub fn chosen(&self) -> Option<(CollectionId, String)> {
        self.items.get(self.selected).cloned()
    }

    pub fn labels(&self) -> Vec<String> {
        self.items.iter().map(|(_, n)| n.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_core::Timestamp;

    const T: i64 = 1_795_320_000_000;

    /// The same id for the same `n` (a v7 id has random bits, so `new_v7` would differ).
    fn cid(n: i64) -> CollectionId {
        CollectionId::parse(&format!("01a20169-2e01-7000-8000-{n:012}")).unwrap()
    }

    fn clip() -> ClipId {
        ClipId::new_v7(Timestamp(T))
    }

    #[test]
    fn which_dialogs_have_a_text_box() {
        for k in [
            DialogKind::NewCollection,
            DialogKind::RenameCollection(cid(0)),
            DialogKind::Note(clip()),
            DialogKind::Tags(clip()),
            DialogKind::BulkTag,
        ] {
            assert!(k.has_input(), "{k:?}");
        }
        assert!(!DialogKind::PickAdd.has_input());
        assert!(!DialogKind::PickRemove.has_input());
        assert!(DialogKind::Note(clip()).multiline());
        assert!(!DialogKind::NewCollection.multiline());
    }

    #[test]
    fn titles_follow_the_flow() {
        assert_eq!(DialogKind::NewCollection.title(), Msg::OrgNewCollection);
        assert_eq!(
            DialogKind::RenameCollection(cid(0)).title(),
            Msg::OrgRenameCollection
        );
        assert_eq!(DialogKind::Note(clip()).title(), Msg::OrgNote);
        assert_eq!(DialogKind::Tags(clip()).title(), Msg::OrgTagsEdit);
        assert_eq!(DialogKind::BulkTag.title(), Msg::OrgAddTag);
        assert_eq!(DialogKind::PickAdd.title(), Msg::OrgAddToCollection);
        assert_eq!(DialogKind::PickRemove.title(), Msg::OrgRemoveFromCollection);
    }

    #[test]
    fn escape_always_cancels() {
        for k in [
            DialogKind::NewCollection,
            DialogKind::Note(clip()),
            DialogKind::Tags(clip()),
            DialogKind::PickAdd,
        ] {
            assert_eq!(dialog_key(k, Key::Escape, false, true), DialogKey::Cancel);
            assert_eq!(dialog_key(k, Key::Escape, true, false), DialogKey::Cancel);
        }
    }

    #[test]
    fn enter_submits_a_single_line_dialog_and_a_picker() {
        for k in [
            DialogKind::NewCollection,
            DialogKind::RenameCollection(cid(0)),
            DialogKind::Tags(clip()),
            DialogKind::BulkTag,
            DialogKind::PickAdd,
            DialogKind::PickRemove,
        ] {
            assert_eq!(
                dialog_key(k, Key::Enter, false, false),
                DialogKey::Submit,
                "{k:?}"
            );
        }
    }

    #[test]
    fn in_a_note_enter_is_a_new_line_and_ctrl_enter_saves() {
        let k = DialogKind::Note(clip());
        assert_eq!(dialog_key(k, Key::Enter, false, false), DialogKey::Pass);
        assert_eq!(dialog_key(k, Key::Enter, true, false), DialogKey::Submit);
    }

    #[test]
    fn arrows_move_the_picker_only() {
        assert_eq!(
            dialog_key(DialogKind::PickAdd, Key::Up, false, true),
            DialogKey::Up
        );
        assert_eq!(
            dialog_key(DialogKind::PickRemove, Key::Down, false, true),
            DialogKey::Down
        );
        // In a note or a name box the arrows move the caret.
        assert_eq!(
            dialog_key(DialogKind::Note(clip()), Key::Up, false, false),
            DialogKey::Pass
        );
        assert_eq!(
            dialog_key(DialogKind::NewCollection, Key::Down, false, false),
            DialogKey::Pass
        );
    }

    #[test]
    fn backspace_in_an_empty_tag_box_removes_the_last_chip() {
        let k = DialogKind::Tags(clip());
        assert_eq!(
            dialog_key(k, Key::Char('\u{8}'), false, true),
            DialogKey::RemoveLastTag
        );
        assert_eq!(
            dialog_key(k, Key::Char('\u{8}'), false, false),
            DialogKey::Pass
        );
        // Other dialogs leave Backspace to the text box.
        assert_eq!(
            dialog_key(DialogKind::BulkTag, Key::Char('\u{8}'), false, true),
            DialogKey::Pass
        );
    }

    #[test]
    fn ordinary_typing_goes_to_the_text_box() {
        assert_eq!(
            dialog_key(DialogKind::NewCollection, Key::Char('a'), false, true),
            DialogKey::Pass
        );
    }

    #[test]
    fn the_picker_moves_within_its_list_without_wrapping() {
        let mut p = Picker::new(vec![
            (cid(1), "Work".into()),
            (cid(2), "Ideas".into()),
            (cid(3), "Misc".into()),
        ]);
        assert!(!p.is_empty());
        assert_eq!(p.selected(), 0);
        p.up();
        assert_eq!(p.selected(), 0);
        p.down();
        p.down();
        p.down();
        assert_eq!(p.selected(), 2);
        assert_eq!(p.chosen(), Some((cid(3), "Misc".to_string())));
        p.select(1);
        assert_eq!(p.chosen().map(|c| c.1), Some("Ideas".to_string()));
        p.select(9);
        assert_eq!(p.selected(), 1, "an out-of-range click changes nothing");
        assert_eq!(p.labels(), ["Work", "Ideas", "Misc"]);
    }

    #[test]
    fn an_empty_picker_chooses_nothing() {
        let mut p = Picker::new(Vec::new());
        assert!(p.is_empty());
        p.down();
        assert_eq!(p.chosen(), None);
        assert_eq!(p.selected(), 0);
    }

    #[test]
    fn debug_never_prints_names() {
        let p = Picker::new(vec![(cid(1), "Secret plans".into())]);
        assert!(!format!("{p:?}").contains("Secret"));
    }
}
