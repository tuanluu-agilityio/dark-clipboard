//! History window logic that does not need a window (Task 211): multi-select and delete
//! confirmation, the keyboard map and window placement. The filters, the list, the detail pane
//! and the row text are the popup's pure view-models (`crate::popup`), reused as they are. The
//! Slint wiring is `ui/history_host.rs`.

// The history window runs on Windows first; other platforms compile it but do not show it yet.
#![cfg_attr(not(windows), allow(dead_code))]

pub mod keys;
pub mod layout;
pub mod menu;
pub mod place;
pub mod select;
pub mod sidebar;

#[cfg(test)]
mod layout_tests {
    //! Task 703: the new layout removed buttons, not actions. These tables keep every key of the
    //! history window on the action it had (`04` section 3, row 1) and say where each removed
    //! control went. `keys.rs` itself is unchanged.

    use super::keys::{map_key, HistoryAction as A, HistoryCtx};
    use crate::popup::keys::{CopyKind, Key, Mods, Move};

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
    const CTRL_SHIFT: Mods = Mods {
        ctrl: true,
        shift: true,
        ..NONE
    };
    const LIST: HistoryCtx = HistoryCtx {
        query_empty: true,
        search_focused: false,
        chips_focused: false,
        confirming: false,
    };

    #[test]
    fn every_history_key_keeps_its_action() {
        let table: &[(Key, Mods, A)] = &[
            (Key::Up, NONE, A::Move(Move::Up)),
            (Key::Down, NONE, A::Move(Move::Down)),
            (Key::PageUp, NONE, A::Move(Move::PageUp)),
            (Key::PageDown, NONE, A::Move(Move::PageDown)),
            (Key::Home, NONE, A::Move(Move::Home)),
            (Key::End, NONE, A::Move(Move::End)),
            (Key::Up, SHIFT, A::Extend(Move::Up)),
            (Key::Down, SHIFT, A::Extend(Move::Down)),
            (Key::PageDown, SHIFT, A::Extend(Move::PageDown)),
            (Key::Home, SHIFT, A::Extend(Move::Home)),
            (Key::End, SHIFT, A::Extend(Move::End)),
            (Key::Down, CTRL, A::MoveKeep(Move::Down)),
            (Key::Up, CTRL, A::MoveKeep(Move::Up)),
            (Key::Home, CTRL, A::MoveKeep(Move::Home)),
            (Key::End, CTRL, A::MoveKeep(Move::End)),
            (Key::Char(' '), NONE, A::ToggleMark),
            (Key::Enter, NONE, A::Copy(CopyKind::Original)),
            (Key::Enter, SHIFT, A::Copy(CopyKind::Plain)),
            (Key::Delete, NONE, A::Delete),
            (Key::Menu, NONE, A::OpenMenu),
            (Key::Char('p'), CTRL, A::TogglePin),
            (Key::Char('t'), CTRL, A::CopyAs),
            (Key::Char('e'), CTRL, A::Edit),
            (Key::Char('d'), CTRL, A::ToggleFavorite),
            (Key::Char('n'), CTRL, A::EditNote),
            (Key::Char('g'), CTRL, A::EditTags),
            (Key::Char('l'), CTRL, A::AddToCollection),
            (Key::Char('l'), CTRL_SHIFT, A::RemoveFromCollection),
            (Key::Char('n'), CTRL_SHIFT, A::NewCollection),
            (Key::Char('r'), CTRL_SHIFT, A::RenameCollection),
            (Key::Delete, CTRL_SHIFT, A::DeleteCollection),
            (Key::Char('e'), CTRL_SHIFT, A::Export),
            (Key::Char('a'), CTRL, A::SelectAll),
            (Key::Escape, NONE, A::Escape),
            // Task 704: new keys, only added.
            (Key::F10, SHIFT, A::OpenMenu),
            (Key::Char('f'), CTRL, A::FocusSearch),
            (Key::Char('k'), CTRL, A::FocusSearch),
            (Key::Char('b'), CTRL, A::ToggleSidebar),
            (Key::Char(','), CTRL, A::OpenSettings),
            (super::keys::f6(), NONE, A::NextRegion),
            (super::keys::f6(), SHIFT, A::PrevRegion),
        ];
        for (key, mods, want) in table {
            assert_eq!(map_key(*key, *mods, LIST), Some(*want), "{key:?} {mods:?}");
        }
        let confirm = HistoryCtx {
            confirming: true,
            ..LIST
        };
        assert_eq!(map_key(Key::Enter, NONE, confirm), Some(A::Confirm));
        assert_eq!(map_key(Key::Escape, NONE, confirm), Some(A::Cancel));
        assert_eq!(map_key(Key::Down, NONE, confirm), Some(A::Swallow));
    }

    /// How a removed control stays reachable.
    #[derive(Debug, Clone, Copy)]
    enum Way {
        /// A button of the detail toolbar (its `act-*` callback in `history.slint`).
        Toolbar(&'static str),
        /// An entry of the context menu (the same callback).
        Menu(&'static str),
        /// A key and the action it maps to.
        Key(Key, Mods, A),
        /// An inline section of the detail panel, the sidebar or the toolbar (a callback in
        /// `history.slint`).
        Panel(&'static str),
    }

    use Way::{Key as K, Menu, Panel, Toolbar};

    /// (old control, ways it is reachable now). Mirrors the table of the task package.
    fn reach() -> Vec<(&'static str, Vec<Way>)> {
        vec![
            (
                "Copy",
                vec![
                    Toolbar("act-copy"),
                    Menu("act-copy"),
                    K(Key::Enter, NONE, A::Copy(CopyKind::Original)),
                ],
            ),
            (
                "Copy as plain text",
                vec![
                    Toolbar("act-copy-plain"),
                    Menu("act-copy-plain"),
                    K(Key::Enter, SHIFT, A::Copy(CopyKind::Plain)),
                ],
            ),
            (
                "Copy as...",
                vec![
                    Toolbar("act-copy-as"),
                    Menu("act-copy-as"),
                    K(Key::Char('t'), CTRL, A::CopyAs),
                ],
            ),
            (
                "Edit...",
                vec![
                    Toolbar("act-edit"),
                    Menu("act-edit"),
                    K(Key::Char('e'), CTRL, A::Edit),
                ],
            ),
            (
                "Pin / Unpin",
                vec![
                    Toolbar("act-pin"),
                    Menu("act-pin"),
                    K(Key::Char('p'), CTRL, A::TogglePin),
                ],
            ),
            (
                "Delete",
                vec![
                    Toolbar("act-delete"),
                    Menu("act-delete"),
                    K(Key::Delete, NONE, A::Delete),
                ],
            ),
            (
                "Favorite",
                vec![
                    Toolbar("act-favorite"),
                    Menu("act-favorite"),
                    K(Key::Char('d'), CTRL, A::ToggleFavorite),
                ],
            ),
            (
                "Select all",
                vec![
                    Menu("act-select-all"),
                    Panel("act-select-mode"),
                    K(Key::Char('a'), CTRL, A::SelectAll),
                ],
            ),
            (
                "Clear selection",
                vec![Panel("act-select-mode"), K(Key::Escape, NONE, A::Escape)],
            ),
            (
                "Add to collection",
                vec![
                    Panel("act-add-collection"),
                    Menu("act-add-collection"),
                    K(Key::Char('l'), CTRL, A::AddToCollection),
                ],
            ),
            (
                "Remove from collection",
                vec![
                    Menu("act-remove-collection"),
                    Panel("org-remove-collection"),
                    K(Key::Char('l'), CTRL_SHIFT, A::RemoveFromCollection),
                ],
            ),
            (
                "Edit tags",
                vec![
                    Panel("act-tags"),
                    Panel("org-remove-tag"),
                    K(Key::Char('g'), CTRL, A::EditTags),
                ],
            ),
            (
                "Edit note",
                vec![Panel("act-note"), K(Key::Char('n'), CTRL, A::EditNote)],
            ),
            (
                "New collection",
                vec![
                    Panel("act-new-collection"),
                    K(Key::Char('n'), CTRL_SHIFT, A::NewCollection),
                ],
            ),
            (
                "Rename collection",
                vec![
                    Panel("act-rename-collection"),
                    K(Key::Char('r'), CTRL_SHIFT, A::RenameCollection),
                ],
            ),
            (
                "Delete collection",
                vec![
                    Panel("act-delete-collection"),
                    K(Key::Delete, CTRL_SHIFT, A::DeleteCollection),
                ],
            ),
            // Task 704: the row menu has an Export entry (same action as the key).
            (
                "Export",
                vec![Menu("export"), K(Key::Char('e'), CTRL_SHIFT, A::Export)],
            ),
            ("Date filters", vec![Panel("date-menu-activated")]),
        ]
    }

    const SLINT: &str = include_str!("../../ui/history.slint");
    const HOST: &str = include_str!("../ui/history_host.rs");
    const ORG: &str = include_str!("../ui/history_host/org.rs");

    /// `act-copy-plain` as the Rust setter name `on_act_copy_plain`.
    fn handler(callback: &str) -> String {
        format!("on_{}", callback.replace('-', "_"))
    }

    /// The row menu entry for a callback name of the old `PopupWindow` menu.
    fn menu_cmd(callback: &str) -> Option<super::menu::RowCmd> {
        use super::menu::RowCmd as C;
        Some(match callback {
            "act-copy" => C::Copy,
            "act-copy-plain" => C::CopyPlain,
            "act-copy-as" => C::CopyAs,
            "act-edit" => C::Edit,
            "act-pin" => C::TogglePin,
            "act-delete" => C::Delete,
            "act-favorite" => C::ToggleFavorite,
            "act-select-all" => C::SelectAll,
            "act-add-collection" => C::AddToCollection,
            "act-remove-collection" => C::RemoveFromCollection,
            "export" => C::Export,
            _ => return None,
        })
    }

    #[test]
    fn every_removed_control_stays_reachable() {
        for (control, ways) in reach() {
            assert!(!ways.is_empty(), "{control} has no way left");
            for way in ways {
                match way {
                    Way::Key(key, mods, want) => {
                        assert_eq!(map_key(key, mods, LIST), Some(want), "{control}");
                    }
                    // Task 704: the row menu is built in Rust (`menu::history_menu_items`); its
                    // entry runs the same host method as the toolbar button.
                    Way::Menu(cb) => {
                        let cmd = menu_cmd(cb);
                        let ctx = super::menu::MenuCtx::default();
                        assert!(
                            super::menu::history_menu_items(&ctx)
                                .iter()
                                .any(|i| Some(i.cmd) == cmd),
                            "{control}: the row menu has no entry for {cb}"
                        );
                    }
                    Way::Toolbar(cb) | Way::Panel(cb) => {
                        assert!(
                            SLINT.contains(&format!("callback {cb}(")),
                            "{control}: history.slint has no callback {cb}"
                        );
                        let h = handler(cb);
                        assert!(
                            HOST.contains(&h) || ORG.contains(&h),
                            "{control}: nothing handles {cb}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_toolbar_and_the_row_menu_run_the_same_host_methods() {
        // The toolbar calls the callback; the menu's `RowCmd` is dispatched in the host to the
        // method the callback's handler calls.
        for (cb, method) in [
            ("act-copy", "RowCmd::Copy => self.copy(false)"),
            ("act-copy-plain", "RowCmd::CopyPlain => self.copy(true)"),
            ("act-copy-as", "RowCmd::CopyAs => self.open_copy_as()"),
            ("act-edit", "RowCmd::Edit => self.open_edit()"),
            ("act-pin", "RowCmd::TogglePin => self.toggle_pin()"),
            ("act-delete", "RowCmd::Delete => self.request_delete()"),
            ("act-favorite", "RowCmd::ToggleFavorite => self.act_favorite()"),
        ] {
            assert!(
                SLINT.contains(&format!("root.{cb}();")),
                "history.slint: the toolbar no longer calls {cb}"
            );
            assert!(HOST.contains(method), "history_host.rs: no `{method}`");
        }
    }

    #[test]
    fn the_old_footer_and_hint_lines_are_gone() {
        for gone in [
            "Strings.footer-copy-plain",
            "OrgStrings.org-hints",
            "OrgStrings.collection-keys",
            "Strings.clear-selection",
            "ActionButton",
        ] {
            assert!(!SLINT.contains(gone), "history.slint still has {gone}");
        }
    }
}
