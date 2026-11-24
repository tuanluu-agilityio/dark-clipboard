//! The F1 / Ctrl+/ shortcuts sheet (Task 706). Pure: the two tables of the keys each window
//! lists, the one key text format, the sheet's rows and its open/close rule. Nothing here draws
//! (`ui/transform.slint` has `ShortcutSheet`) and nothing here decides a key's action: the
//! windows' key maps (`popup::keys::map_key`, `history::keys::map_key_in`) do, and
//! `tests/shortcuts.rs` proves that every row of a table gives its action through them and that
//! every action with a key is either listed or excused in `*_NOT_LISTED`, so the sheet cannot
//! drift from the keys.
//!
//! The sheet's text comes from these tables and `Msg`, never from the long hint strings. Nothing
//! is logged and no clip text is involved.

#[cfg(test)]
use crate::history::keys::HistoryAction;
#[cfg(test)]
use crate::popup::keys::{Action, CopyKind, Move};
use crate::popup::keys::{Key, Mods};
use darkclip_core::i18n::{t, Lang, Msg};

/// The window a sheet belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Win {
    Popup,
    History,
}

/// The four groups of the sheet, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Navigate,
    Copy,
    Organize,
    Window,
}

impl Group {
    fn title(self) -> Msg {
        match self {
            Group::Navigate => Msg::SheetGroupNavigate,
            Group::Copy => Msg::SheetGroupCopy,
            Group::Organize => Msg::SheetGroupOrganize,
            Group::Window => Msg::SheetGroupWindow,
        }
    }
}

/// What a row's key does, in the words the two windows share. Several window actions may map to
/// one (Shift+Enter, Ctrl+Enter ...); the key maps stay the authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetAction {
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    Home,
    End,
    /// Shift+arrow: extend the marked range (history).
    Extend,
    /// Ctrl+arrow: move and keep the marks (history).
    #[cfg(test)]
    MoveKeep,
    Mark,
    SelectAll,
    TogglePreview,
    OpenPreview,
    ClosePreview,
    /// Right on a narrow history window: the details drawer.
    OpenDrawer,
    #[cfg(test)]
    CloseDrawer,
    Copy,
    CopyPlain,
    CopyPaste,
    CopyNth,
    CopyAs,
    Edit,
    Export,
    Pin,
    Favorite,
    Note,
    Tags,
    AddToCollection,
    RemoveFromCollection,
    NewCollection,
    RenameCollection,
    DeleteCollection,
    Delete,
    Undo,
    RowMenu,
    FocusSearch,
    Segment,
    MoreFilters,
    History,
    Settings,
    Sidebar,
    NextRegion,
    #[cfg(test)]
    PrevRegion,
    Shortcuts,
    Close,
}

/// One listed key.
#[derive(Debug, Clone, Copy)]
pub struct Shortcut {
    pub group: Group,
    pub key: Key,
    pub mods: Mods,
    /// Read by the drift test (`tests/shortcuts.rs`), not by the sheet.
    #[allow(dead_code)]
    pub action: SheetAction,
    pub label: Msg,
}

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
const CTRL_SHIFT: Mods = Mods {
    ctrl: true,
    shift: true,
    ..NONE
};

const fn row(group: Group, key: Key, mods: Mods, action: SheetAction, label: Msg) -> Shortcut {
    Shortcut {
        group,
        key,
        mods,
        action,
        label,
    }
}

use Group::{Copy as GCopy, Navigate as GNav, Organize as GOrg, Window as GWin};
use SheetAction as S;

/// The popup's keys (`popup::keys::map_key`).
pub const POPUP_SHORTCUTS: &[Shortcut] = &[
    row(GNav, Key::Up, NONE, S::MoveUp, Msg::SheetPrevious),
    row(GNav, Key::Down, NONE, S::MoveDown, Msg::SheetNext),
    row(GNav, Key::PageUp, NONE, S::PageUp, Msg::SheetPageUp),
    row(GNav, Key::PageDown, NONE, S::PageDown, Msg::SheetPageDown),
    row(GNav, Key::Home, NONE, S::Home, Msg::SheetFirst),
    row(GNav, Key::End, NONE, S::End, Msg::SheetLast),
    row(
        GNav,
        Key::Char(' '),
        NONE,
        S::TogglePreview,
        Msg::UiHintPreview,
    ),
    row(
        GNav,
        Key::Right,
        NONE,
        S::OpenPreview,
        Msg::SheetOpenPreview,
    ),
    row(
        GNav,
        Key::Left,
        NONE,
        S::ClosePreview,
        Msg::SheetClosePreview,
    ),
    row(GCopy, Key::Enter, NONE, S::Copy, Msg::UiCopy),
    row(
        GCopy,
        Key::Enter,
        SHIFT,
        S::CopyPlain,
        Msg::UiFooterCopyPlain,
    ),
    row(GCopy, Key::Enter, CTRL, S::CopyPaste, Msg::UiHintPaste),
    row(GCopy, Key::Char('1'), ALT, S::CopyNth, Msg::SheetCopyNth),
    row(GCopy, Key::Char('t'), CTRL, S::CopyAs, Msg::UiFooterCopyAs),
    row(GOrg, Key::Char('p'), CTRL, S::Pin, Msg::SheetPinToggle),
    row(GOrg, Key::Char('d'), CTRL, S::Favorite, Msg::UiTipFavorite),
    row(GOrg, Key::Delete, NONE, S::Delete, Msg::UiDelete),
    row(GOrg, Key::Char('z'), CTRL, S::Undo, Msg::UiUndo),
    row(GOrg, Key::F10, SHIFT, S::RowMenu, Msg::UiHintActions),
    row(
        GWin,
        Key::Char('k'),
        CTRL,
        S::FocusSearch,
        Msg::UiSearchLabel,
    ),
    row(GWin, Key::Char('1'), CTRL, S::Segment, Msg::SheetFilterTab),
    row(
        GWin,
        Key::Char('f'),
        CTRL_SHIFT,
        S::MoreFilters,
        Msg::UiMoreFilters,
    ),
    row(GWin, Key::Char('h'), CTRL, S::History, Msg::UiTipHistory),
    row(GWin, Key::Char(','), CTRL, S::Settings, Msg::UiTipSettings),
    row(GWin, Key::F1, NONE, S::Shortcuts, Msg::SheetShortcuts),
    row(
        GWin,
        Key::Char('/'),
        CTRL,
        S::Shortcuts,
        Msg::SheetShortcuts,
    ),
    row(GWin, Key::Escape, NONE, S::Close, Msg::UiClose),
];

/// The history window's keys (`history::keys::map_key_in`, in the narrow layout so that Right
/// is the drawer's key).
pub const HISTORY_SHORTCUTS: &[Shortcut] = &[
    row(GNav, Key::Up, NONE, S::MoveUp, Msg::SheetPrevious),
    row(GNav, Key::Down, NONE, S::MoveDown, Msg::SheetNext),
    row(GNav, Key::PageUp, NONE, S::PageUp, Msg::SheetPageUp),
    row(GNav, Key::PageDown, NONE, S::PageDown, Msg::SheetPageDown),
    row(GNav, Key::Home, NONE, S::Home, Msg::SheetFirst),
    row(GNav, Key::End, NONE, S::End, Msg::SheetLast),
    row(GNav, Key::Down, SHIFT, S::Extend, Msg::SheetExtend),
    row(GNav, Key::Char(' '), NONE, S::Mark, Msg::UiHintMark),
    row(GNav, Key::Char('a'), CTRL, S::SelectAll, Msg::UiSelectAll),
    row(GCopy, Key::Enter, NONE, S::Copy, Msg::UiCopy),
    row(
        GCopy,
        Key::Enter,
        SHIFT,
        S::CopyPlain,
        Msg::UiFooterCopyPlain,
    ),
    row(GCopy, Key::Char('t'), CTRL, S::CopyAs, Msg::UiFooterCopyAs),
    row(GCopy, Key::Char('e'), CTRL, S::Edit, Msg::UiTipEditCopy),
    row(
        GCopy,
        Key::Char('e'),
        CTRL_SHIFT,
        S::Export,
        Msg::HistMenuExport,
    ),
    row(GOrg, Key::Char('p'), CTRL, S::Pin, Msg::SheetPinToggle),
    row(GOrg, Key::Char('d'), CTRL, S::Favorite, Msg::UiTipFavorite),
    row(GOrg, Key::Char('n'), CTRL, S::Note, Msg::OrgNote),
    row(GOrg, Key::Char('g'), CTRL, S::Tags, Msg::OrgGroupTags),
    row(
        GOrg,
        Key::Char('l'),
        CTRL,
        S::AddToCollection,
        Msg::HistMenuAddToCollection,
    ),
    row(
        GOrg,
        Key::Char('l'),
        CTRL_SHIFT,
        S::RemoveFromCollection,
        Msg::HistMenuRemoveFromCollection,
    ),
    row(
        GOrg,
        Key::Char('n'),
        CTRL_SHIFT,
        S::NewCollection,
        Msg::OrgNewCollection,
    ),
    row(
        GOrg,
        Key::Char('r'),
        CTRL_SHIFT,
        S::RenameCollection,
        Msg::OrgRenameCollection,
    ),
    row(
        GOrg,
        Key::Delete,
        CTRL_SHIFT,
        S::DeleteCollection,
        Msg::OrgDeleteCollection,
    ),
    row(GOrg, Key::Delete, NONE, S::Delete, Msg::UiDelete),
    row(GOrg, Key::Menu, NONE, S::RowMenu, Msg::UiHintActions),
    row(
        GWin,
        Key::Char('f'),
        CTRL,
        S::FocusSearch,
        Msg::UiSearchLabel,
    ),
    row(GWin, Key::Char('b'), CTRL, S::Sidebar, Msg::SheetSidebar),
    row(GWin, Key::Char(F6), NONE, S::NextRegion, Msg::SheetNextArea),
    row(GWin, Key::Right, NONE, S::OpenDrawer, Msg::SheetDrawer),
    row(GWin, Key::Char(','), CTRL, S::Settings, Msg::UiTipSettings),
    row(GWin, Key::F1, NONE, S::Shortcuts, Msg::SheetShortcuts),
    row(
        GWin,
        Key::Char('/'),
        CTRL,
        S::Shortcuts,
        Msg::SheetShortcuts,
    ),
    row(GWin, Key::Escape, NONE, S::Close, Msg::UiClose),
];

/// F6 as the history key map receives it (`history::keys::history_key`).
const F6: char = '\u{F709}';

/// Popup actions that have a key but are not in the table, with the reason. None today.
#[cfg(test)]
#[allow(dead_code)]
pub const POPUP_NOT_LISTED: &[(SheetAction, &str)] = &[];

/// History actions that have a key but are not in the table, with the reason.
#[cfg(test)]
#[allow(dead_code)]
pub const HISTORY_NOT_LISTED: &[(SheetAction, &str)] = &[
    (
        SheetAction::MoveKeep,
        "Ctrl plus an arrow key already listed: it only keeps the marks",
    ),
    (
        SheetAction::PrevRegion,
        "Shift+F6 is F6 backwards; F6 is listed",
    ),
    (
        SheetAction::CloseDrawer,
        "Left closes the drawer that Right opened; Esc closes it too",
    ),
];

/// The sheet word of a popup action (an exhaustive match: a new `Action` must be placed here).
#[cfg(test)]
#[allow(dead_code)]
pub fn popup_sheet_action(a: Action) -> SheetAction {
    match a {
        Action::Move(Move::Up) => S::MoveUp,
        Action::Move(Move::Down) => S::MoveDown,
        Action::Move(Move::PageUp) => S::PageUp,
        Action::Move(Move::PageDown) => S::PageDown,
        Action::Move(Move::Home) => S::Home,
        Action::Move(Move::End) => S::End,
        Action::Copy(CopyKind::Original) => S::Copy,
        Action::Copy(CopyKind::Plain) => S::CopyPlain,
        Action::Copy(CopyKind::Paste) => S::CopyPaste,
        Action::CopyNth(_) => S::CopyNth,
        Action::Delete => S::Delete,
        Action::TogglePin => S::Pin,
        Action::Undo => S::Undo,
        Action::Close => S::Close,
        Action::ToggleDetail => S::TogglePreview,
        Action::OpenDetail => S::OpenPreview,
        Action::CloseDetail => S::ClosePreview,
        Action::OpenHistory => S::History,
        Action::CopyAs => S::CopyAs,
        Action::ToggleFavorite => S::Favorite,
        Action::FocusSearch => S::FocusSearch,
        Action::OpenSettings => S::Settings,
        Action::RowMenu => S::RowMenu,
        Action::Segment(_) => S::Segment,
        Action::MoreFilters => S::MoreFilters,
        Action::Shortcuts => S::Shortcuts,
    }
}

/// The sheet word of a history action; `None` for the keys only the delete confirmation uses.
#[cfg(test)]
#[allow(dead_code)]
pub fn history_sheet_action(a: HistoryAction) -> Option<SheetAction> {
    Some(match a {
        HistoryAction::Move(Move::Up) => S::MoveUp,
        HistoryAction::Move(Move::Down) => S::MoveDown,
        HistoryAction::Move(Move::PageUp) => S::PageUp,
        HistoryAction::Move(Move::PageDown) => S::PageDown,
        HistoryAction::Move(Move::Home) => S::Home,
        HistoryAction::Move(Move::End) => S::End,
        HistoryAction::Extend(_) => S::Extend,
        HistoryAction::MoveKeep(_) => S::MoveKeep,
        HistoryAction::Copy(CopyKind::Plain) => S::CopyPlain,
        HistoryAction::Copy(_) => S::Copy,
        HistoryAction::Delete => S::Delete,
        HistoryAction::TogglePin => S::Pin,
        HistoryAction::CopyAs => S::CopyAs,
        HistoryAction::Edit => S::Edit,
        HistoryAction::ToggleFavorite => S::Favorite,
        HistoryAction::EditNote => S::Note,
        HistoryAction::EditTags => S::Tags,
        HistoryAction::AddToCollection => S::AddToCollection,
        HistoryAction::RemoveFromCollection => S::RemoveFromCollection,
        HistoryAction::NewCollection => S::NewCollection,
        HistoryAction::RenameCollection => S::RenameCollection,
        HistoryAction::DeleteCollection => S::DeleteCollection,
        HistoryAction::Export => S::Export,
        HistoryAction::SelectAll => S::SelectAll,
        HistoryAction::ToggleMark => S::Mark,
        HistoryAction::OpenMenu => S::RowMenu,
        HistoryAction::Escape => S::Close,
        HistoryAction::ToggleSidebar => S::Sidebar,
        HistoryAction::FocusSearch => S::FocusSearch,
        HistoryAction::OpenSettings => S::Settings,
        HistoryAction::NextRegion => S::NextRegion,
        HistoryAction::PrevRegion => S::PrevRegion,
        HistoryAction::OpenDrawer => S::OpenDrawer,
        HistoryAction::CloseDrawer => S::CloseDrawer,
        HistoryAction::Shortcuts => S::Shortcuts,
        HistoryAction::Confirm | HistoryAction::Cancel | HistoryAction::Swallow => return None,
    })
}

/// The text of a key as the tooltip key caps write it: "Ctrl+T", "Shift+Enter", "Del", "Esc".
pub fn key_label(key: Key, mods: Mods) -> String {
    let mut s = String::new();
    for (on, name) in [
        (mods.ctrl, "Ctrl+"),
        (mods.alt, "Alt+"),
        (mods.shift, "Shift+"),
        (mods.meta, "Meta+"),
    ] {
        if on {
            s.push_str(name);
        }
    }
    let name = match key {
        Key::Up => "Up".to_string(),
        Key::Down => "Down".to_string(),
        Key::Left => "Left".to_string(),
        Key::Right => "Right".to_string(),
        Key::PageUp => "PgUp".to_string(),
        Key::PageDown => "PgDn".to_string(),
        Key::Home => "Home".to_string(),
        Key::End => "End".to_string(),
        Key::Enter => "Enter".to_string(),
        Key::Escape => "Esc".to_string(),
        Key::Delete => "Del".to_string(),
        Key::Tab => "Tab".to_string(),
        Key::Menu => "Menu".to_string(),
        Key::F1 => "F1".to_string(),
        Key::F10 => "F10".to_string(),
        Key::Char(' ') => "Space".to_string(),
        Key::Char(F6) => "F6".to_string(),
        Key::Char(c) => c.to_uppercase().collect(),
        Key::Other => "?".to_string(),
    };
    s.push_str(&name);
    s
}

/// One line of the sheet: a group header (no key) or a key with its label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetRow {
    pub header: bool,
    pub key: String,
    pub label: String,
}

/// The sheet's lines for a window: a header, then the keys of that group, for each group.
pub fn rows(win: Win, lang: Lang) -> Vec<SheetRow> {
    let table = match win {
        Win::Popup => POPUP_SHORTCUTS,
        Win::History => HISTORY_SHORTCUTS,
    };
    let mut out: Vec<SheetRow> = Vec::with_capacity(table.len() + 4);
    let mut group: Option<Group> = None;
    for r in table {
        if group != Some(r.group) {
            group = Some(r.group);
            out.push(SheetRow {
                header: true,
                key: String::new(),
                label: t(lang, r.group.title()).to_string(),
            });
        }
        out.push(SheetRow {
            header: false,
            key: key_label(r.key, r.mods),
            label: t(lang, r.label).to_string(),
        });
    }
    out
}

/// Splits the lines into the sheet's two columns at a group boundary, as even as possible.
pub fn columns(all: Vec<SheetRow>) -> (Vec<SheetRow>, Vec<SheetRow>) {
    let n = all.len();
    let mut best = n;
    let mut best_diff = n;
    for (i, r) in all.iter().enumerate() {
        if r.header && i > 0 {
            let diff = i.abs_diff(n - i);
            if diff < best_diff {
                best_diff = diff;
                best = i;
            }
        }
    }
    let mut left = all;
    let right = left.split_off(best);
    (left, right)
}

/// What the open sheet does with a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetKey {
    /// The sheet is closed and the key is not its own: the window's key map decides.
    Pass,
    /// F1 or Ctrl+/ with the sheet closed.
    Open,
    /// F1, Ctrl+/ or Esc with the sheet open.
    Close,
    /// Any other key while the sheet is open: nothing happens behind it.
    Swallow,
}

/// True for F1 and Ctrl+/ (the same modifier rules as `Action::Shortcuts` and
/// `HistoryAction::Shortcuts`).
pub fn is_sheet_toggle(k: Key, m: Mods) -> bool {
    match k {
        Key::F1 => !m.alt && !m.meta,
        Key::Char('/') => m.ctrl && !m.alt && !m.meta,
        _ => false,
    }
}

/// The sheet's rule for a key. The hosts ask this first, so Esc closes the sheet before the
/// normal unwind of the window, and typing never reaches the window while the sheet is open.
pub fn sheet_key(open: bool, k: Key, m: Mods) -> SheetKey {
    match (open, is_sheet_toggle(k, m)) {
        (false, true) => SheetKey::Open,
        (false, false) => SheetKey::Pass,
        (true, true) => SheetKey::Close,
        (true, false) if k == Key::Escape => SheetKey::Close,
        (true, false) => SheetKey::Swallow,
    }
}
