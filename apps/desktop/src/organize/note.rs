//! The note editor's state (Task 403): length limit, counter, Save and Cancel. Pure.

use darkclip_core::NOTE_MAX_CHARS;

/// A note being edited. Cancel simply drops the editor; nothing was saved.
#[derive(Clone, PartialEq, Eq)]
pub struct NoteEditor {
    original: String,
    text: String,
}

/// Never prints the note (GC-06).
impl std::fmt::Debug for NoteEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoteEditor")
            .field("chars", &self.count())
            .finish_non_exhaustive()
    }
}

impl NoteEditor {
    /// An editor on the clip's current note (`None`: no note yet).
    pub fn new(existing: Option<&str>) -> Self {
        let _ = (existing, NOTE_MAX_CHARS);
        Self {
            original: String::new(),
            text: String::new(),
        }
    }

    pub fn set_text(&mut self, text: &str) {
        let _ = text;
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Characters typed (not bytes).
    pub fn count(&self) -> usize {
        0
    }

    pub fn max(&self) -> usize {
        0
    }

    /// "123 / 4000".
    pub fn counter(&self) -> String {
        String::new()
    }

    pub fn over_limit(&self) -> bool {
        false
    }

    /// Save is offered when the note changed and is within the limit.
    pub fn can_save(&self) -> bool {
        false
    }

    /// What Save sends: `Some(Some(text))` sets the note, `Some(None)` clears it (the text is
    /// empty or only spaces), `None` when Save is not allowed.
    pub fn save_value(&self) -> Option<Option<String>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_editor_holds_the_current_note_and_nothing_to_save() {
        let e = NoteEditor::new(Some("call back"));
        assert_eq!(e.text(), "call back");
        assert_eq!(e.count(), 9);
        assert!(!e.can_save(), "unchanged");
        assert_eq!(e.save_value(), None);
        assert_eq!(NoteEditor::new(None).text(), "");
    }

    #[test]
    fn the_counter_counts_characters_and_shows_the_limit() {
        let mut e = NoteEditor::new(None);
        e.set_text("đã xong");
        assert_eq!(e.count(), 7);
        assert_eq!(e.max(), 4000);
        assert_eq!(e.counter(), "7 / 4000");
    }

    #[test]
    fn exactly_4000_characters_can_be_saved_and_4001_cannot() {
        let mut e = NoteEditor::new(None);
        e.set_text(&"đ".repeat(4000));
        assert!(!e.over_limit());
        assert!(e.can_save());
        assert_eq!(e.save_value(), Some(Some("đ".repeat(4000))));
        e.set_text(&"đ".repeat(4001));
        assert!(e.over_limit());
        assert!(!e.can_save());
        assert_eq!(e.save_value(), None);
    }

    #[test]
    fn emptying_the_note_clears_it() {
        let mut e = NoteEditor::new(Some("old"));
        e.set_text("  \n ");
        assert!(e.can_save());
        assert_eq!(e.save_value(), Some(None));
        // Nothing to clear when there was no note.
        let mut n = NoteEditor::new(None);
        n.set_text("   ");
        assert!(!n.can_save(), "blank over no note is no change");
    }

    #[test]
    fn going_back_to_the_original_text_is_not_a_change() {
        let mut e = NoteEditor::new(Some("old"));
        e.set_text("older");
        assert!(e.can_save());
        e.set_text("old");
        assert!(!e.can_save());
    }

    #[test]
    fn debug_never_prints_the_note() {
        let e = NoteEditor::new(Some("TOPSECRET-NOTE"));
        assert!(!format!("{e:?}").contains("TOPSECRET"));
    }
}
