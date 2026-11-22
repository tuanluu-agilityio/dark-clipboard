//! Popup keyboard map (MVP §2, Task 204). Pure: a key plus modifiers plus a little context gives
//! an action, or `None` when the key belongs to the search box or to Slint's Tab navigation.

use slint::platform::Key as SlintKey;

/// A key as the popup sees it (from Slint's `KeyEvent.text`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Escape,
    Delete,
    Tab,
    Char(char),
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

/// What else decides a key's meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyCtx {
    /// The search box is empty (then Home, End and Delete act on the list).
    pub query_empty: bool,
    /// The search box has the keyboard focus.
    pub search_focused: bool,
    /// A delete can still be undone (then Ctrl+Z undoes it instead of editing the text).
    pub undo_available: bool,
    /// A filter chip has the focus (Enter and Space toggle it; only Esc closes).
    pub chips_focused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyKind {
    /// Enter: restore the clip to the clipboard.
    Original,
    /// Shift+Enter: plain text only.
    Plain,
    /// Ctrl+Enter: copy and paste (Task 205 adds the paste; until then it copies).
    Paste,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Move(Move),
    Copy(CopyKind),
    /// Alt+1..9: copy the n-th visible result (0-based).
    CopyNth(usize),
    Delete,
    TogglePin,
    Undo,
    Close,
}

/// Which filter chip has the keyboard focus, from per-chip gained/lost reports. The reports of
/// two chips may arrive in any order (the old chip's "lost" after the new chip's "gained"), so
/// a "lost" only clears the chip it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChipFocus(Option<i32>);

impl ChipFocus {
    pub fn report(&mut self, chip: i32, has_focus: bool) {
        if has_focus {
            self.0 = Some(chip);
        } else if self.0 == Some(chip) {
            self.0 = None;
        }
    }

    /// Chips were rebuilt: a focused chip that no longer exists has no focus.
    pub fn retain(&mut self, exists: impl Fn(i32) -> bool) {
        if self.0.is_some_and(|c| !exists(c)) {
            self.0 = None;
        }
    }

    /// True when a chip has the focus. The search box having the focus wins (a chip can then
    /// not have it, whatever was reported).
    pub fn focused(&self, search_focused: bool) -> bool {
        !search_focused && self.0.is_some()
    }
}

/// Maps Slint's key text to a `Key`.
pub fn key_from_text(s: &str) -> Key {
    let mut it = s.chars();
    let (Some(c), None) = (it.next(), it.next()) else {
        return Key::Other;
    };
    let named = [
        (SlintKey::UpArrow, Key::Up),
        (SlintKey::DownArrow, Key::Down),
        (SlintKey::PageUp, Key::PageUp),
        (SlintKey::PageDown, Key::PageDown),
        (SlintKey::Home, Key::Home),
        (SlintKey::End, Key::End),
        (SlintKey::Return, Key::Enter),
        (SlintKey::Escape, Key::Escape),
        (SlintKey::Delete, Key::Delete),
        (SlintKey::Tab, Key::Tab),
    ];
    if let Some(&(_, k)) = named.iter().find(|(sk, _)| char::from(*sk) == c) {
        return k;
    }
    if c == '\r' {
        return Key::Enter;
    }
    // Other control characters and Slint's private-use codes (arrows, F-keys, ...).
    if c.is_control() || ('\u{F700}'..='\u{F8FF}').contains(&c) {
        // Ctrl+letter may arrive as its control code (Ctrl+P = U+0010, Ctrl+Z = U+001A).
        if matches!(c, '\u{10}' | '\u{1a}') {
            return Key::Char(c);
        }
        return Key::Other;
    }
    Key::Char(c)
}

/// The popup action for a key, or `None` to let the focused element handle it.
pub fn map_key(k: Key, m: Mods, ctx: KeyCtx) -> Option<Action> {
    if ctx.chips_focused && k != Key::Escape {
        return None;
    }
    let plain = !m.ctrl && !m.alt && !m.meta;
    // The list is in charge of Home, End and Delete unless the search box can use them.
    let list_keys = !ctx.search_focused || ctx.query_empty;
    match k {
        Key::Up if plain => Some(Action::Move(Move::Up)),
        Key::Down if plain => Some(Action::Move(Move::Down)),
        Key::PageUp if plain => Some(Action::Move(Move::PageUp)),
        Key::PageDown if plain => Some(Action::Move(Move::PageDown)),
        Key::Home if (plain && list_keys) || (m.ctrl && !m.alt) => Some(Action::Move(Move::Home)),
        Key::End if (plain && list_keys) || (m.ctrl && !m.alt) => Some(Action::Move(Move::End)),
        Key::Enter if m.alt || m.meta => None,
        Key::Enter if m.ctrl => Some(Action::Copy(CopyKind::Paste)),
        Key::Enter if m.shift => Some(Action::Copy(CopyKind::Plain)),
        Key::Enter => Some(Action::Copy(CopyKind::Original)),
        Key::Escape => Some(Action::Close),
        Key::Delete if plain && !m.shift && list_keys => Some(Action::Delete),
        Key::Char('p' | 'P' | '\u{10}') if m.ctrl && !m.alt => Some(Action::TogglePin),
        Key::Char('z' | 'Z' | '\u{1a}') if m.ctrl && !m.alt && ctx.undo_available => {
            Some(Action::Undo)
        }
        Key::Char(c @ '1'..='9') if m.alt && !m.ctrl && !m.meta => {
            let n = c.to_digit(10).and_then(|d| usize::try_from(d).ok())?;
            Some(Action::CopyNth(n - 1))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    const ALT: Mods = Mods { alt: true, ..NONE };

    /// Typing in a non-empty search box.
    const TYPING: KeyCtx = KeyCtx {
        query_empty: false,
        search_focused: true,
        undo_available: false,
        chips_focused: false,
    };
    /// Just opened: search box focused and empty.
    const FRESH: KeyCtx = KeyCtx {
        query_empty: true,
        ..TYPING
    };
    /// The list has the focus (after Tab).
    const LIST: KeyCtx = KeyCtx {
        search_focused: false,
        ..TYPING
    };

    fn text(k: SlintKey) -> String {
        char::from(k).to_string()
    }

    #[test]
    fn slint_key_texts_are_recognised() {
        assert_eq!(key_from_text(&text(SlintKey::UpArrow)), Key::Up);
        assert_eq!(key_from_text(&text(SlintKey::DownArrow)), Key::Down);
        assert_eq!(key_from_text(&text(SlintKey::PageUp)), Key::PageUp);
        assert_eq!(key_from_text(&text(SlintKey::PageDown)), Key::PageDown);
        assert_eq!(key_from_text(&text(SlintKey::Home)), Key::Home);
        assert_eq!(key_from_text(&text(SlintKey::End)), Key::End);
        assert_eq!(key_from_text(&text(SlintKey::Return)), Key::Enter);
        assert_eq!(key_from_text("\r"), Key::Enter);
        assert_eq!(key_from_text(&text(SlintKey::Escape)), Key::Escape);
        assert_eq!(key_from_text(&text(SlintKey::Delete)), Key::Delete);
        assert_eq!(key_from_text(&text(SlintKey::Tab)), Key::Tab);
        assert_eq!(key_from_text("p"), Key::Char('p'));
        assert_eq!(key_from_text("ệ"), Key::Char('ệ'));
        assert_eq!(key_from_text(""), Key::Other);
        assert_eq!(key_from_text("ab"), Key::Other);
        assert_eq!(key_from_text(&text(SlintKey::LeftArrow)), Key::Other);
    }

    #[test]
    fn arrows_and_pages_move_from_anywhere() {
        for ctx in [TYPING, FRESH, LIST] {
            assert_eq!(map_key(Key::Up, NONE, ctx), Some(Action::Move(Move::Up)));
            assert_eq!(
                map_key(Key::Down, NONE, ctx),
                Some(Action::Move(Move::Down))
            );
            assert_eq!(
                map_key(Key::PageUp, NONE, ctx),
                Some(Action::Move(Move::PageUp))
            );
            assert_eq!(
                map_key(Key::PageDown, NONE, ctx),
                Some(Action::Move(Move::PageDown))
            );
        }
    }

    #[test]
    fn home_and_end_edit_text_while_typing() {
        assert_eq!(map_key(Key::Home, NONE, TYPING), None);
        assert_eq!(map_key(Key::End, NONE, TYPING), None);
        assert_eq!(
            map_key(Key::Home, NONE, FRESH),
            Some(Action::Move(Move::Home))
        );
        assert_eq!(map_key(Key::End, NONE, LIST), Some(Action::Move(Move::End)));
        // Ctrl+Home/End always reach the list.
        assert_eq!(
            map_key(Key::Home, CTRL, TYPING),
            Some(Action::Move(Move::Home))
        );
        assert_eq!(
            map_key(Key::End, CTRL, TYPING),
            Some(Action::Move(Move::End))
        );
    }

    #[test]
    fn enter_variants_copy() {
        assert_eq!(
            map_key(Key::Enter, NONE, TYPING),
            Some(Action::Copy(CopyKind::Original))
        );
        assert_eq!(
            map_key(Key::Enter, SHIFT, TYPING),
            Some(Action::Copy(CopyKind::Plain))
        );
        assert_eq!(
            map_key(Key::Enter, CTRL, LIST),
            Some(Action::Copy(CopyKind::Paste))
        );
        assert_eq!(map_key(Key::Enter, ALT, LIST), None);
    }

    #[test]
    fn escape_closes() {
        for ctx in [TYPING, FRESH, LIST] {
            assert_eq!(map_key(Key::Escape, NONE, ctx), Some(Action::Close));
        }
    }

    #[test]
    fn delete_removes_only_when_it_cannot_edit_text() {
        assert_eq!(map_key(Key::Delete, NONE, TYPING), None);
        assert_eq!(map_key(Key::Delete, NONE, FRESH), Some(Action::Delete));
        assert_eq!(map_key(Key::Delete, NONE, LIST), Some(Action::Delete));
        assert_eq!(map_key(Key::Delete, CTRL, LIST), None);
    }

    #[test]
    fn ctrl_p_pins_from_anywhere() {
        for c in ['p', 'P', '\u{10}'] {
            assert_eq!(map_key(Key::Char(c), CTRL, TYPING), Some(Action::TogglePin));
        }
        assert_eq!(map_key(Key::Char('p'), NONE, TYPING), None);
        assert_eq!(map_key(Key::Char('p'), ALT, TYPING), None);
    }

    #[test]
    fn alt_digits_copy_the_nth_result() {
        assert_eq!(
            map_key(Key::Char('1'), ALT, TYPING),
            Some(Action::CopyNth(0))
        );
        assert_eq!(map_key(Key::Char('9'), ALT, LIST), Some(Action::CopyNth(8)));
        assert_eq!(map_key(Key::Char('0'), ALT, TYPING), None);
        assert_eq!(map_key(Key::Char('1'), NONE, TYPING), None);
        assert_eq!(
            map_key(
                Key::Char('1'),
                Mods {
                    ctrl: true,
                    alt: true,
                    ..NONE
                },
                TYPING
            ),
            None
        );
    }

    #[test]
    fn ctrl_z_undoes_a_delete_only_when_one_is_pending() {
        let pending = KeyCtx {
            undo_available: true,
            ..TYPING
        };
        assert_eq!(map_key(Key::Char('z'), CTRL, pending), Some(Action::Undo));
        assert_eq!(
            map_key(Key::Char('\u{1a}'), CTRL, pending),
            Some(Action::Undo)
        );
        // Otherwise the search box undoes its own typing.
        assert_eq!(map_key(Key::Char('z'), CTRL, TYPING), None);
    }

    #[test]
    fn a_focused_chip_keeps_its_keys_but_esc_still_closes() {
        let chip = KeyCtx {
            search_focused: false,
            chips_focused: true,
            ..TYPING
        };
        assert_eq!(map_key(Key::Enter, NONE, chip), None);
        assert_eq!(map_key(Key::Char(' '), NONE, chip), None);
        assert_eq!(map_key(Key::Down, NONE, chip), None);
        assert_eq!(map_key(Key::Delete, NONE, chip), None);
        assert_eq!(map_key(Key::Escape, NONE, chip), Some(Action::Close));
    }

    #[test]
    fn chip_focus_does_not_depend_on_report_order() {
        let mut f = ChipFocus::default();
        assert!(!f.focused(false));
        // Tab from chip 1 to chip 2, reports in the expected order.
        f.report(1, true);
        assert!(f.focused(false));
        f.report(1, false);
        f.report(2, true);
        assert!(f.focused(false));
        // Tab from chip 2 to chip 3, the old chip's "lost" arriving last.
        f.report(3, true);
        f.report(2, false);
        assert!(f.focused(false));
        // The focus leaves the chips.
        f.report(3, false);
        assert!(!f.focused(false));
        // The search box having the focus always wins.
        f.report(4, true);
        assert!(!f.focused(true));
        // A focused chip removed by a rebuild loses the focus; others keep it.
        f.retain(|id| id != 4);
        assert!(!f.focused(false));
        f.report(5, true);
        f.retain(|id| id == 5);
        assert!(f.focused(false));
    }

    #[test]
    fn tab_and_typing_are_left_to_slint() {
        for ctx in [TYPING, FRESH, LIST] {
            assert_eq!(map_key(Key::Tab, NONE, ctx), None);
            assert_eq!(map_key(Key::Tab, SHIFT, ctx), None);
            assert_eq!(map_key(Key::Char('a'), NONE, ctx), None);
            assert_eq!(map_key(Key::Char('ệ'), SHIFT, ctx), None);
            assert_eq!(map_key(Key::Other, NONE, ctx), None);
        }
    }
}
