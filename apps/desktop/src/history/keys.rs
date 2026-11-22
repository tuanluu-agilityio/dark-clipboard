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
    /// Shift+arrow or Shift+Page: moves the cursor and extends the marked range.
    Extend(Move),
    /// Enter (restore) or Shift+Enter (plain text).
    Copy(CopyKind),
    /// Delete: opens the confirmation.
    Delete,
    TogglePin,
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
    match k {
        Key::Up if plain => Some(step(Move::Up)),
        Key::Down if plain => Some(step(Move::Down)),
        Key::PageUp if plain => Some(step(Move::PageUp)),
        Key::PageDown if plain => Some(step(Move::PageDown)),
        Key::Home if (plain && list_keys) || (m.ctrl && !m.alt) => {
            Some(HistoryAction::Move(Move::Home))
        }
        Key::End if (plain && list_keys) || (m.ctrl && !m.alt) => {
            Some(HistoryAction::Move(Move::End))
        }
        Key::Char(' ') if plain && !m.shift && list_keys => Some(HistoryAction::ToggleMark),
        Key::Enter if m.alt || m.meta || m.ctrl => None,
        Key::Enter if m.shift => Some(HistoryAction::Copy(CopyKind::Plain)),
        Key::Enter => Some(HistoryAction::Copy(CopyKind::Original)),
        Key::Escape => Some(HistoryAction::Escape),
        Key::Delete if plain && !m.shift && list_keys => Some(HistoryAction::Delete),
        Key::Menu if plain => Some(HistoryAction::OpenMenu),
        Key::Char('p' | 'P' | '\u{10}') if m.ctrl && !m.alt => Some(HistoryAction::TogglePin),
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
}
