//! The history window's row menu (Task 704). Pure: which entries the menu has and when they are
//! enabled (`history_menu_items`), and the keys of the open menu (`RowMenu`). The menu is drawn
//! by the kit `Menu` (`ui/kit/menu.slint`), which only forwards keys here; the host owns the
//! state and runs the command. Entries are fixed i18n labels: a menu never shows the preview, the
//! source or any other content of a clip (a masked clip included, GC-03).

use crate::popup::menu::{letter_jump, menu_key, MenuAction, MenuKey, MenuKind, OpenMenu};
use darkclip_core::i18n::Msg;

/// What an entry does. Each maps to the callback the detail toolbar or the keyboard already has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowCmd {
    Copy,
    CopyPlain,
    CopyAs,
    Edit,
    TogglePin,
    ToggleFavorite,
    /// Opens the second level (one entry per collection).
    AddToCollection,
    RemoveFromCollection,
    Tags,
    Note,
    Export,
    SelectAll,
    Delete,
}

/// What decides the entries' state (the same flags the window already mirrors).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MenuCtx {
    /// Exactly one clip is targeted.
    pub can_copy: bool,
    /// One text clip that is not masked.
    pub can_transform: bool,
    /// At least one clip is targeted.
    pub has_selection: bool,
    /// The pin action would unpin.
    pub will_unpin: bool,
    /// The favorite action would remove the favorite.
    pub will_unfavorite: bool,
    pub can_favorite: bool,
    /// At least one targeted clip is not masked.
    pub can_add: bool,
    pub can_remove: bool,
    /// One clip that is not masked (its note).
    pub can_edit: bool,
    /// A collection is shown, or at least one clip is targeted.
    pub can_export: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuItem {
    pub cmd: RowCmd,
    pub label: Msg,
    /// The key cap beside the label, where the menu is wide enough for it in both languages.
    pub key: Option<Msg>,
    pub enabled: bool,
    pub submenu: bool,
    pub danger: bool,
}

/// The row menu's entries, in order. Disabled entries stay in the list (`04` section 1.8).
pub fn history_menu_items(c: &MenuCtx) -> Vec<MenuItem> {
    let item = |cmd, label, key, enabled| MenuItem {
        cmd,
        label,
        key,
        enabled,
        submenu: false,
        danger: false,
    };
    let pin = if c.will_unpin { Msg::UiUnpin } else { Msg::UiPin };
    let fav = if c.will_unfavorite {
        Msg::UiMenuUnfavorite
    } else {
        Msg::UiMenuFavorite
    };
    vec![
        item(RowCmd::Copy, Msg::UiCopy, Some(Msg::UiKeyEnter), c.can_copy),
        item(RowCmd::CopyPlain, Msg::HistMenuCopyPlain, None, c.can_copy),
        item(RowCmd::CopyAs, Msg::TfCopyAs, None, c.can_transform),
        item(
            RowCmd::Edit,
            Msg::UiTipEditCopy,
            Some(Msg::UiKeyCtrlE),
            c.can_transform,
        ),
        item(
            RowCmd::TogglePin,
            pin,
            Some(Msg::UiKeyCtrlP),
            c.has_selection,
        ),
        item(
            RowCmd::ToggleFavorite,
            fav,
            Some(Msg::UiKeyCtrlD),
            c.can_favorite,
        ),
        MenuItem {
            submenu: true,
            ..item(
                RowCmd::AddToCollection,
                Msg::HistMenuAddToCollection,
                None,
                c.can_add,
            )
        },
        item(
            RowCmd::RemoveFromCollection,
            Msg::HistMenuRemoveFromCollection,
            None,
            c.can_remove,
        ),
        item(
            RowCmd::Tags,
            Msg::OrgTagsEdit,
            Some(Msg::HistKeyCtrlG),
            c.can_add,
        ),
        item(
            RowCmd::Note,
            Msg::OrgEditNote,
            Some(Msg::HistKeyCtrlN),
            c.can_edit,
        ),
        item(
            RowCmd::Export,
            Msg::HistMenuExport,
            Some(Msg::HistKeyCtrlShiftE),
            c.can_export,
        ),
        item(
            RowCmd::SelectAll,
            Msg::UiSelectAll,
            Some(Msg::HistKeyCtrlA),
            true,
        ),
        MenuItem {
            danger: true,
            ..item(
                RowCmd::Delete,
                Msg::UiDelete,
                Some(Msg::UiKeyDel),
                c.has_selection,
            )
        },
    ]
}

/// What a key or a click did to the open menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowMenuEvent {
    /// Nothing for the host to do beyond redrawing (the highlight moved, a level opened).
    Handled,
    /// The key is not the menu's.
    Unhandled,
    Close,
    Run(RowCmd),
    /// A collection of the second level (its index) was chosen: add the targets to it.
    AddTo(usize),
}

/// The open row menu: its entries, how many collections the second level lists and where the
/// highlight is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowMenu {
    pub open: OpenMenu,
    items: Vec<MenuItem>,
    collections: usize,
    /// The first-level entry the second level was opened from.
    parent: usize,
}

impl RowMenu {
    pub fn new(items: Vec<MenuItem>, collections: usize) -> Self {
        RowMenu {
            open: OpenMenu::new(MenuKind::Row, items.len()),
            items,
            collections,
            parent: 0,
        }
    }

    pub fn items(&self) -> &[MenuItem] {
        &self.items
    }

    /// 0 for the entries, 1 for the collections.
    pub fn level(&self) -> u8 {
        self.open.level
    }

    pub fn highlighted(&self) -> usize {
        self.open.highlighted
    }

    pub fn set_highlight(&mut self, i: usize) {
        if i < self.open.count {
            self.open.highlighted = i;
        }
    }

    /// One key. `labels` are the texts of the entries of the current level (for the letter jump).
    pub fn key(&mut self, key: MenuKey, labels: &[&str]) -> RowMenuEvent {
        let m = self.open;
        let action = match key {
            MenuKey::Letter(c) => match letter_jump(labels, m.highlighted, c) {
                Some(i) => MenuAction::Move(i),
                None => return RowMenuEvent::Unhandled,
            },
            k => menu_key(k, m.highlighted, m.count),
        };
        match action {
            MenuAction::Move(i) => {
                self.set_highlight(i);
                RowMenuEvent::Handled
            }
            MenuAction::Activate(i) => self.activate(i),
            // Esc leaves the second level first, then the menu.
            MenuAction::Close if self.open.level > 0 => self.back(),
            MenuAction::Close => RowMenuEvent::Close,
            MenuAction::Back => self.back(),
            MenuAction::Submenu(i) => {
                let opens = self.open.level == 0
                    && self.collections > 0
                    && self.items.get(i).is_some_and(|it| it.submenu && it.enabled);
                if opens {
                    self.enter_collections(i);
                }
                RowMenuEvent::Handled
            }
            MenuAction::None => RowMenuEvent::Unhandled,
        }
    }

    /// A click, or Enter, on entry `i` of the current level.
    pub fn activate(&mut self, i: usize) -> RowMenuEvent {
        if self.open.level > 0 {
            return if i < self.collections {
                RowMenuEvent::AddTo(i)
            } else {
                RowMenuEvent::Handled
            };
        }
        let Some(item) = self.items.get(i).copied() else {
            return RowMenuEvent::Handled;
        };
        if !item.enabled {
            return RowMenuEvent::Handled;
        }
        if item.submenu && self.collections > 0 {
            self.enter_collections(i);
            return RowMenuEvent::Handled;
        }
        // A submenu entry with no collection to list falls back to the picker dialog.
        RowMenuEvent::Run(item.cmd)
    }

    fn enter_collections(&mut self, from: usize) {
        self.parent = from;
        self.open.enter_level(1, self.collections);
    }

    fn back(&mut self) -> RowMenuEvent {
        if self.open.back() {
            self.open.highlighted = self.parent;
        }
        RowMenuEvent::Handled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_core::i18n::{t, Lang};

    fn all_on() -> MenuCtx {
        MenuCtx {
            can_copy: true,
            can_transform: true,
            has_selection: true,
            can_favorite: true,
            can_add: true,
            can_remove: true,
            can_edit: true,
            can_export: true,
            ..MenuCtx::default()
        }
    }

    fn cmds(c: &MenuCtx) -> Vec<RowCmd> {
        history_menu_items(c).iter().map(|i| i.cmd).collect()
    }

    fn enabled(c: &MenuCtx, cmd: RowCmd) -> bool {
        history_menu_items(c)
            .iter()
            .find(|i| i.cmd == cmd)
            .is_some_and(|i| i.enabled)
    }

    #[test]
    fn the_entries_are_todays_actions_then_the_organisation_ones_and_export() {
        use RowCmd::*;
        assert_eq!(
            cmds(&all_on()),
            [
                Copy,
                CopyPlain,
                CopyAs,
                Edit,
                TogglePin,
                ToggleFavorite,
                AddToCollection,
                RemoveFromCollection,
                Tags,
                Note,
                Export,
                SelectAll,
                Delete
            ]
        );
        // The list does not change with the state: disabled entries stay visible.
        assert_eq!(cmds(&MenuCtx::default()), cmds(&all_on()));
    }

    #[test]
    fn nothing_selected_disables_everything_but_select_all() {
        let c = MenuCtx::default();
        for item in history_menu_items(&c) {
            assert_eq!(item.enabled, item.cmd == RowCmd::SelectAll, "{:?}", item.cmd);
        }
    }

    #[test]
    fn a_masked_clip_cannot_be_transformed_edited_or_added() {
        // 703 rules: copy, pin, delete and export still work; the rest is off.
        let masked = MenuCtx {
            can_copy: true,
            has_selection: true,
            can_favorite: true,
            can_remove: true,
            can_export: true,
            ..MenuCtx::default()
        };
        for off in [
            RowCmd::CopyAs,
            RowCmd::Edit,
            RowCmd::AddToCollection,
            RowCmd::Tags,
            RowCmd::Note,
        ] {
            assert!(!enabled(&masked, off), "{off:?}");
        }
        for on in [
            RowCmd::Copy,
            RowCmd::CopyPlain,
            RowCmd::TogglePin,
            RowCmd::Delete,
            RowCmd::Export,
            RowCmd::RemoveFromCollection,
        ] {
            assert!(enabled(&masked, on), "{on:?}");
        }
    }

    #[test]
    fn several_clips_cannot_be_copied_but_can_be_pinned_and_deleted() {
        let many = MenuCtx {
            can_copy: false,
            can_transform: false,
            ..all_on()
        };
        assert!(!enabled(&many, RowCmd::Copy));
        assert!(!enabled(&many, RowCmd::CopyPlain));
        assert!(!enabled(&many, RowCmd::CopyAs));
        assert!(enabled(&many, RowCmd::TogglePin));
        assert!(enabled(&many, RowCmd::Delete));
    }

    #[test]
    fn export_is_present_and_follows_can_export() {
        assert!(enabled(&all_on(), RowCmd::Export));
        let none = MenuCtx {
            can_export: false,
            ..all_on()
        };
        assert!(!enabled(&none, RowCmd::Export));
    }

    #[test]
    fn labels_follow_the_state_and_only_add_to_collection_opens_a_submenu() {
        let pinned = MenuCtx {
            will_unpin: true,
            will_unfavorite: true,
            ..all_on()
        };
        let label = |c: &MenuCtx, cmd| {
            history_menu_items(c)
                .iter()
                .find(|i| i.cmd == cmd)
                .map(|i| i.label)
        };
        assert_eq!(label(&all_on(), RowCmd::TogglePin), Some(Msg::UiPin));
        assert_eq!(label(&pinned, RowCmd::TogglePin), Some(Msg::UiUnpin));
        assert_eq!(
            label(&all_on(), RowCmd::ToggleFavorite),
            Some(Msg::UiMenuFavorite)
        );
        assert_eq!(
            label(&pinned, RowCmd::ToggleFavorite),
            Some(Msg::UiMenuUnfavorite)
        );
        for i in history_menu_items(&all_on()) {
            assert_eq!(i.submenu, i.cmd == RowCmd::AddToCollection, "{:?}", i.cmd);
            assert_eq!(i.danger, i.cmd == RowCmd::Delete, "{:?}", i.cmd);
        }
    }

    #[test]
    fn every_label_and_key_cap_exists_in_both_languages() {
        let items = history_menu_items(&all_on());
        assert!(!items.is_empty());
        for item in items {
            for lang in [Lang::En, Lang::Vi] {
                assert!(!t(lang, item.label).is_empty());
                if let Some(k) = item.key {
                    assert!(!t(lang, k).is_empty());
                }
            }
        }
    }

    fn open(ctx: &MenuCtx, collections: usize) -> (RowMenu, Vec<String>) {
        let items = history_menu_items(ctx);
        let labels = items
            .iter()
            .map(|i| t(Lang::En, i.label).to_string())
            .collect();
        (RowMenu::new(items, collections), labels)
    }

    fn key(m: &mut RowMenu, labels: &[String], k: MenuKey) -> RowMenuEvent {
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        m.key(k, &refs)
    }

    fn add_index(m: &RowMenu) -> usize {
        m.items()
            .iter()
            .position(|i| i.cmd == RowCmd::AddToCollection)
            .unwrap_or(0)
    }

    #[test]
    fn arrows_and_home_end_move_the_highlight_and_wrap() {
        let (mut m, labels) = open(&all_on(), 2);
        assert_eq!(m.highlighted(), 0);
        assert_eq!(key(&mut m, &labels, MenuKey::Down), RowMenuEvent::Handled);
        assert_eq!(m.highlighted(), 1);
        key(&mut m, &labels, MenuKey::Up);
        key(&mut m, &labels, MenuKey::Up);
        assert_eq!(m.highlighted(), m.items().len() - 1, "wraps to the end");
        key(&mut m, &labels, MenuKey::Home);
        assert_eq!(m.highlighted(), 0);
        key(&mut m, &labels, MenuKey::End);
        assert_eq!(m.highlighted(), m.items().len() - 1);
    }

    #[test]
    fn enter_runs_the_highlighted_entry_and_escape_closes() {
        let (mut m, labels) = open(&all_on(), 2);
        assert_eq!(
            key(&mut m, &labels, MenuKey::Enter),
            RowMenuEvent::Run(RowCmd::Copy)
        );
        assert_eq!(key(&mut m, &labels, MenuKey::Escape), RowMenuEvent::Close);
        assert_eq!(
            key(&mut m, &labels, MenuKey::Other),
            RowMenuEvent::Unhandled
        );
    }

    #[test]
    fn a_letter_jumps_to_the_next_entry_with_that_first_letter() {
        let (mut m, labels) = open(&all_on(), 2);
        // Copy, Copy plain text, Copy as...: 'c' goes to the next one.
        assert_eq!(
            key(&mut m, &labels, MenuKey::Letter('c')),
            RowMenuEvent::Handled
        );
        assert_eq!(m.highlighted(), 1);
        key(&mut m, &labels, MenuKey::Letter('D'));
        assert_eq!(
            m.items()[m.highlighted()].cmd,
            RowCmd::Delete,
            "case does not matter"
        );
        assert_eq!(
            key(&mut m, &labels, MenuKey::Letter('9')),
            RowMenuEvent::Unhandled,
            "no entry starts with 9"
        );
    }

    #[test]
    fn disabled_entries_are_not_activated_by_enter() {
        let (mut m, labels) = open(&MenuCtx::default(), 0);
        // Highlight "Copy" (disabled): Enter does nothing, the menu stays open.
        assert_eq!(key(&mut m, &labels, MenuKey::Enter), RowMenuEvent::Handled);
        // "Select all" is the one enabled entry.
        let i = m
            .items()
            .iter()
            .position(|i| i.cmd == RowCmd::SelectAll)
            .unwrap_or(0);
        m.set_highlight(i);
        assert_eq!(
            key(&mut m, &labels, MenuKey::Enter),
            RowMenuEvent::Run(RowCmd::SelectAll)
        );
        // A click on a disabled entry does nothing either.
        assert_eq!(m.activate(0), RowMenuEvent::Handled);
    }

    #[test]
    fn right_arrow_opens_the_collections_and_left_goes_back() {
        let (mut m, labels) = open(&all_on(), 3);
        let add = add_index(&m);
        m.set_highlight(add);
        assert_eq!(key(&mut m, &labels, MenuKey::Right), RowMenuEvent::Handled);
        assert_eq!((m.level(), m.open.count, m.highlighted()), (1, 3, 0));
        // Down inside the second level, then Enter adds to that collection.
        let cols = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        key(&mut m, &cols, MenuKey::Down);
        assert_eq!(key(&mut m, &cols, MenuKey::Enter), RowMenuEvent::AddTo(1));
        // Left goes back to the first level with the parent entry highlighted.
        assert_eq!(key(&mut m, &cols, MenuKey::Left), RowMenuEvent::Handled);
        assert_eq!(m.level(), 0);
        assert_eq!(m.highlighted(), add);
        // Left at the first level is the menu's too (nothing happens).
        assert_eq!(key(&mut m, &labels, MenuKey::Left), RowMenuEvent::Handled);
    }

    #[test]
    fn right_arrow_on_an_ordinary_entry_does_nothing_and_enter_on_add_opens_the_level() {
        let (mut m, labels) = open(&all_on(), 2);
        assert_eq!(key(&mut m, &labels, MenuKey::Right), RowMenuEvent::Handled);
        assert_eq!(m.level(), 0);
        let add = add_index(&m);
        m.set_highlight(add);
        assert_eq!(key(&mut m, &labels, MenuKey::Enter), RowMenuEvent::Handled);
        assert_eq!(m.level(), 1);
    }

    #[test]
    fn with_no_collection_add_runs_the_picker_dialog() {
        let (mut m, labels) = open(&all_on(), 0);
        let add = add_index(&m);
        m.set_highlight(add);
        assert_eq!(
            key(&mut m, &labels, MenuKey::Enter),
            RowMenuEvent::Run(RowCmd::AddToCollection)
        );
        assert_eq!(m.level(), 0);
    }

    #[test]
    fn escape_in_the_second_level_goes_back_before_it_closes() {
        let (mut m, labels) = open(&all_on(), 2);
        let add = add_index(&m);
        m.set_highlight(add);
        key(&mut m, &labels, MenuKey::Right);
        let cols = vec!["A".to_string(), "B".to_string()];
        assert_eq!(key(&mut m, &cols, MenuKey::Escape), RowMenuEvent::Handled);
        assert_eq!(m.level(), 0);
        assert_eq!(key(&mut m, &labels, MenuKey::Escape), RowMenuEvent::Close);
    }
}
