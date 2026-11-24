//! The settings window's keys, as pure functions (Task 705, D-079). Slint only forwards the key
//! text and modifiers through one capture-phase `key(...) -> bool` callback; what a key means is
//! decided here, so it is tested without a window. The `Select` list (a `PopupWindow`) owns the
//! keys while it is open and forwards them through `select_code`.

use crate::popup::keys::{key_from_text, Key, Mods};

/// Number of sections in the nav (General, Capture and Privacy, History and Storage, Data and
/// Transfer, About).
pub const SECTION_COUNT: i32 = 5;

/// What a key does in the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsKey {
    /// Ctrl+S: the same as pressing Save.
    Save,
    /// Ctrl+1..5: the section with this 0-based index.
    Section(u8),
    /// Up in the nav: the previous section.
    NavUp,
    /// Down in the nav: the next section.
    NavDown,
    /// Esc: closes one layer (`escape_layer`).
    Escape,
}

/// What else decides a key's meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyCtx {
    /// A nav item has the keyboard focus (Up and Down move between sections only then).
    pub nav_focused: bool,
    /// One of the confirmation dialogs is open.
    pub dialog_open: bool,
    /// The running-app picker is open.
    pub picker_open: bool,
    /// A `Select` list is open: it owns every key (the window sees none of them).
    pub list_open: bool,
}

/// The layer one Esc press closes, in order: a `Select` list, then a dialog or the picker, else
/// the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeLayer {
    List,
    Dialog,
    Picker,
    Window,
}

pub fn escape_layer(ctx: KeyCtx) -> EscapeLayer {
    if ctx.list_open {
        EscapeLayer::List
    } else if ctx.dialog_open {
        EscapeLayer::Dialog
    } else if ctx.picker_open {
        EscapeLayer::Picker
    } else {
        EscapeLayer::Window
    }
}

/// The settings action for a key, or `None` to let the focused element handle it.
pub fn map_key(k: Key, m: Mods, ctx: KeyCtx) -> Option<SettingsKey> {
    let _ = (k, m, ctx);
    None
}

/// Slint's key text and modifiers to a settings action.
pub fn map_text(text: &str, m: Mods, ctx: KeyCtx) -> Option<SettingsKey> {
    map_key(key_from_text(text), m, ctx)
}

/// What a key does to a `Select`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectKey {
    Open,
    Prev,
    Next,
    First,
    Last,
    Choose,
    Close,
}

impl SelectKey {
    /// The code the Slint `Select` understands (0 is "not handled").
    pub fn code(self) -> i32 {
        match self {
            SelectKey::Open => 1,
            SelectKey::Prev => 2,
            SelectKey::Next => 3,
            SelectKey::First => 4,
            SelectKey::Last => 5,
            SelectKey::Choose => 6,
            SelectKey::Close => 7,
        }
    }
}

pub fn select_key(k: Key, m: Mods, open: bool) -> Option<SelectKey> {
    let _ = (k, m, open);
    None
}

/// The code for `Select`: 0 when the key is not for it.
pub fn select_code(text: &str, alt: bool, open: bool) -> i32 {
    let m = Mods {
        alt,
        ..Mods::default()
    };
    select_key(key_from_text(text), m, open).map_or(0, SelectKey::code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::platform::Key as SlintKey;

    const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
    };
    const CTRL: Mods = Mods { ctrl: true, ..NONE };
    const ALT: Mods = Mods { alt: true, ..NONE };
    const CTRL_SHIFT: Mods = Mods {
        ctrl: true,
        shift: true,
        ..NONE
    };
    const IDLE: KeyCtx = KeyCtx {
        nav_focused: false,
        dialog_open: false,
        picker_open: false,
        list_open: false,
    };
    const NAV: KeyCtx = KeyCtx {
        nav_focused: true,
        ..IDLE
    };
    const DIALOG: KeyCtx = KeyCtx {
        dialog_open: true,
        ..IDLE
    };
    const PICKER: KeyCtx = KeyCtx {
        picker_open: true,
        ..IDLE
    };
    const LIST: KeyCtx = KeyCtx {
        list_open: true,
        ..IDLE
    };

    fn text(k: SlintKey) -> String {
        char::from(k).to_string()
    }

    #[test]
    fn ctrl_s_saves_in_every_resting_context() {
        for c in ['s', 'S', '\u{13}'] {
            // A text field has the focus: the chord is consumed, never typed.
            assert_eq!(
                map_key(Key::Char(c), CTRL, IDLE),
                Some(SettingsKey::Save),
                "{c:?}"
            );
            assert_eq!(map_key(Key::Char(c), CTRL, NAV), Some(SettingsKey::Save));
        }
        assert_eq!(map_text("s", CTRL, IDLE), Some(SettingsKey::Save));
    }

    #[test]
    fn plain_s_and_other_chords_are_typing() {
        assert_eq!(map_key(Key::Char('s'), NONE, IDLE), None);
        assert_eq!(map_key(Key::Char('s'), ALT, IDLE), None);
        assert_eq!(map_key(Key::Char('s'), CTRL_SHIFT, IDLE), None);
        assert_eq!(map_key(Key::Char('S'), NONE, IDLE), None);
    }

    #[test]
    fn ctrl_digits_pick_a_section() {
        for (c, n) in [('1', 0u8), ('2', 1), ('3', 2), ('4', 3), ('5', 4)] {
            assert_eq!(
                map_key(Key::Char(c), CTRL, IDLE),
                Some(SettingsKey::Section(n))
            );
            assert_eq!(
                map_key(Key::Char(c), CTRL, NAV),
                Some(SettingsKey::Section(n))
            );
        }
        // There is no sixth section; plain digits are typing.
        assert_eq!(map_key(Key::Char('6'), CTRL, IDLE), None);
        assert_eq!(map_key(Key::Char('0'), CTRL, IDLE), None);
        assert_eq!(map_key(Key::Char('1'), NONE, IDLE), None);
        assert_eq!(map_key(Key::Char('1'), ALT, IDLE), None);
        assert_eq!(map_key(Key::Char('1'), CTRL_SHIFT, IDLE), None);
    }

    #[test]
    fn up_and_down_move_sections_only_while_a_nav_item_is_focused() {
        assert_eq!(map_key(Key::Up, NONE, NAV), Some(SettingsKey::NavUp));
        assert_eq!(map_key(Key::Down, NONE, NAV), Some(SettingsKey::NavDown));
        // In a field or on a control they stay with it.
        assert_eq!(map_key(Key::Up, NONE, IDLE), None);
        assert_eq!(map_key(Key::Down, NONE, IDLE), None);
        // With a modifier they are not the nav keys.
        assert_eq!(map_key(Key::Down, ALT, NAV), None);
        assert_eq!(map_key(Key::Up, CTRL, NAV), None);
    }

    #[test]
    fn slint_key_texts_reach_the_map() {
        assert_eq!(
            map_text(&text(SlintKey::UpArrow), NONE, NAV),
            Some(SettingsKey::NavUp)
        );
        assert_eq!(
            map_text(&text(SlintKey::DownArrow), NONE, NAV),
            Some(SettingsKey::NavDown)
        );
        assert_eq!(
            map_text(&text(SlintKey::Escape), NONE, IDLE),
            Some(SettingsKey::Escape)
        );
        assert_eq!(map_text(&text(SlintKey::DownArrow), NONE, IDLE), None);
    }

    #[test]
    fn a_dialog_or_the_picker_blocks_the_chords_but_not_escape() {
        for ctx in [DIALOG, PICKER] {
            assert_eq!(map_key(Key::Char('s'), CTRL, ctx), None);
            assert_eq!(map_key(Key::Char('2'), CTRL, ctx), None);
            assert_eq!(map_key(Key::Escape, NONE, ctx), Some(SettingsKey::Escape));
        }
        // The dialog or picker also keeps the arrow keys even if a nav item still has the focus.
        let nav_in_dialog = KeyCtx {
            nav_focused: true,
            ..DIALOG
        };
        assert_eq!(map_key(Key::Down, NONE, nav_in_dialog), None);
    }

    #[test]
    fn an_open_list_owns_every_key() {
        for k in [Key::Escape, Key::Up, Key::Down, Key::Char('s')] {
            assert_eq!(map_key(k, CTRL, LIST), None, "{k:?}");
            assert_eq!(map_key(k, NONE, LIST), None, "{k:?}");
        }
        let nav_list = KeyCtx {
            nav_focused: true,
            ..LIST
        };
        assert_eq!(map_key(Key::Down, NONE, nav_list), None);
    }

    #[test]
    fn escape_closes_one_layer_per_press() {
        assert_eq!(map_key(Key::Escape, NONE, IDLE), Some(SettingsKey::Escape));
        assert_eq!(escape_layer(LIST), EscapeLayer::List);
        let all = KeyCtx {
            nav_focused: true,
            dialog_open: true,
            picker_open: true,
            list_open: true,
        };
        assert_eq!(escape_layer(all), EscapeLayer::List);
        let no_list = KeyCtx {
            list_open: false,
            ..all
        };
        assert_eq!(escape_layer(no_list), EscapeLayer::Dialog);
        let picker_only = KeyCtx {
            dialog_open: false,
            ..no_list
        };
        assert_eq!(escape_layer(picker_only), EscapeLayer::Picker);
        assert_eq!(escape_layer(IDLE), EscapeLayer::Window);
        assert_eq!(escape_layer(NAV), EscapeLayer::Window);
    }

    #[test]
    fn select_keys_when_closed() {
        assert_eq!(
            select_key(Key::Char(' '), NONE, false),
            Some(SelectKey::Open)
        );
        assert_eq!(select_key(Key::Down, ALT, false), Some(SelectKey::Open));
        // Plain arrows and Enter leave a closed select alone (Tab and the form keep working).
        assert_eq!(select_key(Key::Down, NONE, false), None);
        assert_eq!(select_key(Key::Up, NONE, false), None);
        assert_eq!(select_key(Key::Enter, NONE, false), None);
        assert_eq!(select_key(Key::Escape, NONE, false), None);
        assert_eq!(select_key(Key::Char(' '), CTRL, false), None);
    }

    #[test]
    fn select_keys_when_open() {
        assert_eq!(select_key(Key::Up, NONE, true), Some(SelectKey::Prev));
        assert_eq!(select_key(Key::Down, NONE, true), Some(SelectKey::Next));
        assert_eq!(select_key(Key::Home, NONE, true), Some(SelectKey::First));
        assert_eq!(select_key(Key::End, NONE, true), Some(SelectKey::Last));
        assert_eq!(select_key(Key::Enter, NONE, true), Some(SelectKey::Choose));
        assert_eq!(
            select_key(Key::Char(' '), NONE, true),
            Some(SelectKey::Choose)
        );
        assert_eq!(select_key(Key::Escape, NONE, true), Some(SelectKey::Close));
        // Alt+Down on an open list is the same as Down.
        assert_eq!(select_key(Key::Down, ALT, true), Some(SelectKey::Next));
        // Typing keys are not handled.
        assert_eq!(select_key(Key::Char('a'), NONE, true), None);
    }

    #[test]
    fn select_codes_match_the_slint_side() {
        assert_eq!(select_code(" ", false, false), 1);
        assert_eq!(select_code(&text(SlintKey::DownArrow), true, false), 1);
        assert_eq!(select_code(&text(SlintKey::UpArrow), false, true), 2);
        assert_eq!(select_code(&text(SlintKey::DownArrow), false, true), 3);
        assert_eq!(select_code(&text(SlintKey::Home), false, true), 4);
        assert_eq!(select_code(&text(SlintKey::End), false, true), 5);
        assert_eq!(select_code(&text(SlintKey::Return), false, true), 6);
        assert_eq!(select_code(&text(SlintKey::Escape), false, true), 7);
        assert_eq!(select_code("x", false, true), 0);
    }
}
