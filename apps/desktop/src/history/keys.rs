//! The history window's keyboard map (Task 211). Pure, like the popup's (`popup::keys`, whose
//! `Key`, `Mods` and `Move` it reuses): a key plus a little context gives an action, or `None`
//! when the key belongs to the search box, a focused chip or Slint's Tab navigation.
//!
//! While the delete confirmation is open the window is modal: Enter confirms, Esc cancels and
//! every other key is swallowed, so nothing else can happen behind the dialog.

use crate::popup::keys::{CopyKind, Key, Mods, Move};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HistoryCtx {
    pub query_empty: bool,
    pub search_focused: bool,
    pub chips_focused: bool,
    /// The delete confirmation is open.
    pub confirming: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryAction {
    Move(Move),
    /// Shift+arrow, Shift+Page, Shift+Home/End: moves the cursor and extends the marked range.
    Extend(Move),
    /// Ctrl+arrow, Ctrl+Page, Ctrl+Home/End: moves the cursor and keeps the marks.
    MoveKeep(Move),
    /// Enter (restore) or Shift+Enter (plain text).
    Copy(CopyKind),
    /// Delete: opens the confirmation.
    Delete,
    TogglePin,
    /// Ctrl+T: "Copy as..." for the clip (Task 404).
    CopyAs,
    /// Ctrl+E: edit a copy of the clip's text (Task 404).
    Edit,
    /// Ctrl+D: favorite or unfavorite the marked clips (Task 403).
    ToggleFavorite,
    /// Ctrl+N: edit the note of the clip.
    EditNote,
    /// Ctrl+G: edit the tags of the clip, or add a tag to the marked clips.
    EditTags,
    /// Ctrl+L: add the marked clips to a collection.
    AddToCollection,
    /// Ctrl+Shift+L: remove the marked clips from a collection.
    RemoveFromCollection,
    /// Ctrl+Shift+N: create a collection.
    NewCollection,
    /// Ctrl+Shift+R: rename the collection the list is filtered to.
    RenameCollection,
    /// Ctrl+Shift+Delete: delete that collection (asks first; its clips stay).
    DeleteCollection,
    /// Ctrl+Shift+E: export the collection the list is filtered to, or else the marked clips
    /// (Task 405).
    Export,
    SelectAll,
    /// Space: marks or unmarks the clip under the cursor.
    ToggleMark,
    /// Menu key: the context menu of the clip under the cursor.
    OpenMenu,
    /// Esc with no dialog open: drops the marks, or closes the window when none are left.
    Escape,
    Confirm,
    Cancel,
    /// A key the modal dialog takes without acting on it.
    Swallow,
}

/// The history action for a key, or `None` to let the focused element handle it.
pub fn map_key(k: Key, m: Mods, ctx: HistoryCtx) -> Option<HistoryAction> {
    if ctx.confirming {
        return Some(match k {
            Key::Enter if !m.ctrl && !m.alt && !m.meta => HistoryAction::Confirm,
            Key::Escape => HistoryAction::Cancel,
            _ => HistoryAction::Swallow,
        });
    }
    if ctx.chips_focused && k != Key::Escape {
        return None;
    }
    let plain = !m.ctrl && !m.alt && !m.meta;
    let list_keys = !ctx.search_focused || ctx.query_empty;
    let step = |mv: Move| {
        if m.shift {
            HistoryAction::Extend(mv)
        } else {
            HistoryAction::Move(mv)
        }
    };
    let ctrl_only = m.ctrl && !m.alt && !m.meta && !m.shift;
    let ctrl_shift = m.ctrl && m.shift && !m.alt && !m.meta;
    let nav = |k: Key| match k {
        Key::Up => Some(Move::Up),
        Key::Down => Some(Move::Down),
        Key::PageUp => Some(Move::PageUp),
        Key::PageDown => Some(Move::PageDown),
        Key::Home => Some(Move::Home),
        Key::End => Some(Move::End),
        _ => None,
    };
    match k {
        Key::Up | Key::Down | Key::PageUp | Key::PageDown if plain => nav(k).map(step),
        // Home and End belong to the search box text unless the list owns the keys.
        Key::Home | Key::End if plain && list_keys => nav(k).map(step),
        Key::Up | Key::Down | Key::PageUp | Key::PageDown | Key::Home | Key::End if ctrl_only => {
            nav(k).map(HistoryAction::MoveKeep)
        }
        Key::Char(' ') if plain && !m.shift && list_keys => Some(HistoryAction::ToggleMark),
        Key::Enter if m.alt || m.meta || m.ctrl => None,
        Key::Enter if m.shift => Some(HistoryAction::Copy(CopyKind::Plain)),
        Key::Enter => Some(HistoryAction::Copy(CopyKind::Original)),
        Key::Escape => Some(HistoryAction::Escape),
        Key::Delete if plain && !m.shift && list_keys => Some(HistoryAction::Delete),
        Key::Menu if plain => Some(HistoryAction::OpenMenu),
        Key::Char('p' | 'P' | '\u{10}') if m.ctrl && !m.alt => Some(HistoryAction::TogglePin),
        Key::Char('t' | 'T' | '\u{14}') if ctrl_only => Some(HistoryAction::CopyAs),
        Key::Char('e' | 'E' | '\u{5}') if ctrl_only => Some(HistoryAction::Edit),
        Key::Char('d' | 'D' | '\u{4}') if ctrl_only => Some(HistoryAction::ToggleFavorite),
        Key::Char('n' | 'N' | '\u{e}') if ctrl_only => Some(HistoryAction::EditNote),
        Key::Char('g' | 'G' | '\u{7}') if ctrl_only => Some(HistoryAction::EditTags),
        Key::Char('l' | 'L' | '\u{c}') if ctrl_only => Some(HistoryAction::AddToCollection),
        Key::Char('l' | 'L' | '\u{c}') if ctrl_shift => Some(HistoryAction::RemoveFromCollection),
        Key::Char('n' | 'N' | '\u{e}') if ctrl_shift => Some(HistoryAction::NewCollection),
        Key::Char('r' | 'R' | '\u{12}') if ctrl_shift => Some(HistoryAction::RenameCollection),
        Key::Delete if ctrl_shift => Some(HistoryAction::DeleteCollection),
        Key::Char('e' | 'E' | '\u{5}') if ctrl_shift => Some(HistoryAction::Export),
        // Ctrl+A selects text in a non-empty search box; otherwise it selects every loaded row.
        Key::Char('a' | 'A' | '\u{1}') if m.ctrl && !m.alt && list_keys => {
            Some(HistoryAction::SelectAll)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use HistoryAction as A;

    const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
    };
    const CTRL: Mods = Mods { ctrl: true, ..NONE };
    const SHIFT: Mods = Mods {
        shift: true,
        ..NONE
    };
    const LIST: HistoryCtx = HistoryCtx {
        query_empty: true,
        search_focused: false,
        chips_focused: false,
        confirming: false,
    };
    const TYPING: HistoryCtx = HistoryCtx {
        query_empty: false,
        search_focused: true,
        ..LIST
    };
    const CONFIRM: HistoryCtx = HistoryCtx {
        confirming: true,
        ..LIST
    };

    #[test]
    fn arrows_move_and_shift_arrows_extend() {
        assert_eq!(map_key(Key::Down, NONE, LIST), Some(A::Move(Move::Down)));
        assert_eq!(map_key(Key::Up, SHIFT, LIST), Some(A::Extend(Move::Up)));
        assert_eq!(
            map_key(Key::PageDown, SHIFT, LIST),
            Some(A::Extend(Move::PageDown))
        );
        // Arrows work while typing in the search box too.
        assert_eq!(map_key(Key::Down, NONE, TYPING), Some(A::Move(Move::Down)));
    }

    #[test]
    fn enter_copies_and_shift_enter_copies_plain() {
        assert_eq!(
            map_key(Key::Enter, NONE, LIST),
            Some(A::Copy(CopyKind::Original))
        );
        assert_eq!(
            map_key(Key::Enter, SHIFT, LIST),
            Some(A::Copy(CopyKind::Plain))
        );
        // There is no paste target in this window: Ctrl+Enter does nothing.
        assert_eq!(map_key(Key::Enter, CTRL, LIST), None);
    }

    #[test]
    fn delete_and_space_act_on_the_list_only_when_the_search_is_not_in_use() {
        assert_eq!(map_key(Key::Delete, NONE, LIST), Some(A::Delete));
        assert_eq!(map_key(Key::Delete, NONE, TYPING), None);
        assert_eq!(map_key(Key::Char(' '), NONE, LIST), Some(A::ToggleMark));
        assert_eq!(map_key(Key::Char(' '), NONE, TYPING), None);
        // Shift+Delete is not Delete.
        assert_eq!(map_key(Key::Delete, SHIFT, LIST), None);
    }

    #[test]
    fn ctrl_a_selects_rows_but_not_while_editing_the_search_text() {
        assert_eq!(map_key(Key::Char('a'), CTRL, LIST), Some(A::SelectAll));
        assert_eq!(map_key(Key::Char('a'), CTRL, TYPING), None);
        // An empty, focused search box lets the list have it.
        let empty_focused = HistoryCtx {
            search_focused: true,
            ..LIST
        };
        assert_eq!(
            map_key(Key::Char('A'), CTRL, empty_focused),
            Some(A::SelectAll)
        );
    }

    #[test]
    fn copy_as_and_edit_keys() {
        for c in ['t', 'T', '\u{14}'] {
            assert_eq!(map_key(Key::Char(c), CTRL, TYPING), Some(A::CopyAs));
        }
        for c in ['e', 'E', '\u{5}'] {
            assert_eq!(map_key(Key::Char(c), CTRL, TYPING), Some(A::Edit));
            assert_eq!(map_key(Key::Char(c), CTRL, LIST), Some(A::Edit));
        }
        assert_eq!(map_key(Key::Char('e'), NONE, TYPING), None);
        assert_eq!(map_key(Key::Char('t'), SHIFT, TYPING), None);
        // The modal delete dialog still swallows them.
        let confirming = HistoryCtx {
            confirming: true,
            ..LIST
        };
        assert_eq!(map_key(Key::Char('t'), CTRL, confirming), Some(A::Swallow));
    }

    #[test]
    fn pin_menu_and_escape() {
        assert_eq!(map_key(Key::Char('p'), CTRL, TYPING), Some(A::TogglePin));
        assert_eq!(map_key(Key::Char('\u{10}'), CTRL, LIST), Some(A::TogglePin));
        assert_eq!(map_key(Key::Menu, NONE, LIST), Some(A::OpenMenu));
        assert_eq!(map_key(Key::Escape, NONE, TYPING), Some(A::Escape));
    }

    #[test]
    fn a_focused_chip_keeps_its_keys_except_escape() {
        let chips = HistoryCtx {
            chips_focused: true,
            ..LIST
        };
        assert_eq!(map_key(Key::Enter, NONE, chips), None);
        assert_eq!(map_key(Key::Char(' '), NONE, chips), None);
        assert_eq!(map_key(Key::Escape, NONE, chips), Some(A::Escape));
    }

    #[test]
    fn the_confirmation_is_modal() {
        assert_eq!(map_key(Key::Enter, NONE, CONFIRM), Some(A::Confirm));
        assert_eq!(map_key(Key::Escape, NONE, CONFIRM), Some(A::Cancel));
        // Delete, typing, Ctrl+Enter, Ctrl+A and arrows cannot confirm or act behind the dialog.
        for (k, m) in [
            (Key::Delete, NONE),
            (Key::Char('y'), NONE),
            (Key::Enter, CTRL),
            (Key::Char('a'), CTRL),
            (Key::Down, NONE),
        ] {
            assert_eq!(map_key(k, m, CONFIRM), Some(A::Swallow), "{k:?}");
        }
    }

    #[test]
    fn organisation_keys() {
        let ctrl_shift = Mods {
            ctrl: true,
            shift: true,
            ..NONE
        };
        for (c, a) in [
            ('d', A::ToggleFavorite),
            ('D', A::ToggleFavorite),
            ('n', A::EditNote),
            ('g', A::EditTags),
            ('l', A::AddToCollection),
            ('L', A::AddToCollection),
        ] {
            assert_eq!(map_key(Key::Char(c), CTRL, LIST), Some(a), "{c}");
            // They do not type text, so they also work from the search box.
            assert_eq!(map_key(Key::Char(c), CTRL, TYPING), Some(a), "{c}");
        }
        for (c, a) in [
            ('l', A::RemoveFromCollection),
            ('L', A::RemoveFromCollection),
            ('n', A::NewCollection),
            ('N', A::NewCollection),
            ('r', A::RenameCollection),
            ('R', A::RenameCollection),
        ] {
            assert_eq!(map_key(Key::Char(c), ctrl_shift, LIST), Some(a), "{c}");
            assert_eq!(map_key(Key::Char(c), ctrl_shift, TYPING), Some(a), "{c}");
        }
        assert_eq!(
            map_key(Key::Delete, ctrl_shift, LIST),
            Some(A::DeleteCollection)
        );
        // Without Ctrl they are ordinary typing.
        for c in ['d', 'n', 'g', 'l'] {
            assert_eq!(map_key(Key::Char(c), NONE, TYPING), None, "{c}");
        }
        // Plain Delete is still the clip delete; Ctrl+Delete is not Ctrl+Shift+Delete.
        assert_eq!(map_key(Key::Delete, NONE, LIST), Some(A::Delete));
        assert_eq!(map_key(Key::Delete, CTRL, LIST), None);
    }

    #[test]
    fn ctrl_shift_e_exports_and_ctrl_e_still_edits() {
        let ctrl_shift = Mods {
            ctrl: true,
            shift: true,
            ..NONE
        };
        for c in ['e', 'E', '\u{5}'] {
            assert_eq!(
                map_key(Key::Char(c), ctrl_shift, LIST),
                Some(A::Export),
                "{c:?}"
            );
            assert_eq!(
                map_key(Key::Char(c), ctrl_shift, TYPING),
                Some(A::Export),
                "{c:?}"
            );
        }
        assert_eq!(map_key(Key::Char('e'), CTRL, LIST), Some(A::Edit));
        assert_eq!(
            map_key(Key::Char('e'), ctrl_shift, CONFIRM),
            Some(A::Swallow)
        );
    }

    #[test]
    fn the_modal_confirmation_swallows_the_organisation_keys() {
        let ctrl_shift = Mods {
            ctrl: true,
            shift: true,
            ..NONE
        };
        assert_eq!(map_key(Key::Char('d'), CTRL, CONFIRM), Some(A::Swallow));
        assert_eq!(
            map_key(Key::Char('n'), ctrl_shift, CONFIRM),
            Some(A::Swallow)
        );
        assert_eq!(map_key(Key::Delete, ctrl_shift, CONFIRM), Some(A::Swallow));
    }

    #[test]
    fn a_focused_chip_hands_the_organisation_shortcuts_to_slint() {
        let chips = HistoryCtx {
            chips_focused: true,
            ..LIST
        };
        assert_eq!(map_key(Key::Char('d'), CTRL, chips), None);
    }

    #[test]
    fn ctrl_arrows_move_without_touching_the_marks() {
        for (k, mv) in [
            (Key::Up, Move::Up),
            (Key::Down, Move::Down),
            (Key::PageUp, Move::PageUp),
            (Key::PageDown, Move::PageDown),
            (Key::Home, Move::Home),
            (Key::End, Move::End),
        ] {
            assert_eq!(map_key(k, CTRL, LIST), Some(A::MoveKeep(mv)), "{k:?}");
            assert_eq!(map_key(k, CTRL, TYPING), Some(A::MoveKeep(mv)), "{k:?}");
        }
    }

    #[test]
    fn shift_home_and_end_extend_when_the_list_owns_the_keys() {
        assert_eq!(map_key(Key::Home, SHIFT, LIST), Some(A::Extend(Move::Home)));
        assert_eq!(map_key(Key::End, SHIFT, LIST), Some(A::Extend(Move::End)));
        assert_eq!(map_key(Key::Home, NONE, LIST), Some(A::Move(Move::Home)));
        // In a non-empty search box they select text.
        assert_eq!(map_key(Key::Home, SHIFT, TYPING), None);
        assert_eq!(map_key(Key::End, NONE, TYPING), None);
    }
}
