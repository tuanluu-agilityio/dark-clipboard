//! The popup's menus (Task 701, D-079). Pure: key handling for the in-window `Menu`, the row
//! menu's actions and the state of an open menu. The menu is drawn by `ui/kit/menu.slint`, which
//! only forwards keys here; the host owns the state and drives the highlighted item (F-31).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popup::keys::CopyKind;
    use crate::popup::paste::{after_copy, AfterCopy, Via};
    use darkclip_core::i18n::{t, Lang, Msg};

    #[test]
    fn menu_moves_down_and_up_and_wraps() {
        assert_eq!(menu_key(MenuKey::Down, 0, 4), MenuAction::Move(1));
        assert_eq!(menu_key(MenuKey::Down, 3, 4), MenuAction::Move(0));
        assert_eq!(menu_key(MenuKey::Up, 2, 4), MenuAction::Move(1));
        assert_eq!(menu_key(MenuKey::Up, 0, 4), MenuAction::Move(3));
    }

    #[test]
    fn menu_home_and_end_jump_to_the_first_and_last_item() {
        assert_eq!(menu_key(MenuKey::Home, 2, 3), MenuAction::Move(0));
        assert_eq!(menu_key(MenuKey::End, 0, 3), MenuAction::Move(2));
    }

    #[test]
    fn menu_enter_activates_the_highlighted_item_and_escape_closes() {
        assert_eq!(menu_key(MenuKey::Enter, 1, 2), MenuAction::Activate(1));
        assert_eq!(menu_key(MenuKey::Enter, 5, 2), MenuAction::None);
        assert_eq!(menu_key(MenuKey::Escape, 0, 2), MenuAction::Close);
        assert_eq!(menu_key(MenuKey::Other, 0, 2), MenuAction::None);
    }

    #[test]
    fn an_empty_menu_only_closes() {
        assert_eq!(menu_key(MenuKey::Down, 0, 0), MenuAction::None);
        assert_eq!(menu_key(MenuKey::Enter, 0, 0), MenuAction::None);
        assert_eq!(menu_key(MenuKey::Escape, 0, 0), MenuAction::Close);
        assert_eq!(menu_key(MenuKey::Right, 0, 0), MenuAction::None);
    }

    #[test]
    fn left_goes_back_and_right_enters_a_submenu() {
        assert_eq!(menu_key(MenuKey::Left, 1, 3), MenuAction::Back);
        assert_eq!(menu_key(MenuKey::Right, 1, 3), MenuAction::Submenu(1));
        assert_eq!(menu_key(MenuKey::Right, 7, 3), MenuAction::None);
    }

    #[test]
    fn a_letter_jumps_to_the_next_item_that_starts_with_it() {
        let labels = ["Copy", "Copy as plain text", "Pin", "Delete"];
        assert_eq!(letter_jump(&labels, 0, 'c'), Some(1));
        assert_eq!(letter_jump(&labels, 1, 'C'), Some(0), "wraps around");
        assert_eq!(letter_jump(&labels, 0, 'd'), Some(3));
        assert_eq!(letter_jump(&labels, 3, 'd'), Some(3), "the only match stays");
        assert_eq!(letter_jump(&labels, 0, 'x'), None);
        assert_eq!(letter_jump(&[], 0, 'c'), None);
        // Accented first letters count as themselves.
        assert_eq!(letter_jump(&["Đã ghim", "Xóa"], 1, 'đ'), Some(0));
    }

    #[test]
    fn key_text_becomes_a_menu_key() {
        use slint::platform::Key as K;
        let text = |k: K| char::from(k).to_string();
        assert_eq!(menu_key_from_text(&text(K::UpArrow)), MenuKey::Up);
        assert_eq!(menu_key_from_text(&text(K::DownArrow)), MenuKey::Down);
        assert_eq!(menu_key_from_text(&text(K::LeftArrow)), MenuKey::Left);
        assert_eq!(menu_key_from_text(&text(K::RightArrow)), MenuKey::Right);
        assert_eq!(menu_key_from_text(&text(K::Home)), MenuKey::Home);
        assert_eq!(menu_key_from_text(&text(K::End)), MenuKey::End);
        assert_eq!(menu_key_from_text(&text(K::Return)), MenuKey::Enter);
        assert_eq!(menu_key_from_text(" "), MenuKey::Enter);
        assert_eq!(menu_key_from_text(&text(K::Escape)), MenuKey::Escape);
        assert_eq!(menu_key_from_text("p"), MenuKey::Letter('p'));
        assert_eq!(menu_key_from_text(&text(K::F1)), MenuKey::Other);
        assert_eq!(menu_key_from_text(""), MenuKey::Other);
    }

    #[test]
    fn the_row_menu_has_no_paste_item_and_hides_copy_as_when_it_cannot_run() {
        use RowMenuAction::*;
        assert_eq!(
            row_menu_items(false, false, true),
            [Copy, CopyPlain, CopyAs, TogglePin, ToggleFavorite, Delete]
        );
        assert_eq!(
            row_menu_items(true, true, false),
            [Copy, CopyPlain, TogglePin, ToggleFavorite, Delete]
        );
        // Exhaustive: adding a variant (a Paste, say) breaks this match on purpose.
        for a in row_menu_items(false, false, true) {
            match a {
                Copy | CopyPlain | CopyAs | TogglePin | ToggleFavorite | Delete => {}
            }
        }
    }

    #[test]
    fn row_menu_labels_follow_the_state_in_both_languages() {
        use RowMenuAction::*;
        for lang in [Lang::En, Lang::Vi] {
            assert_eq!(row_menu_label(Copy, false, false), Msg::UiCopy);
            assert_eq!(row_menu_label(TogglePin, false, false), Msg::UiPin);
            assert_eq!(row_menu_label(TogglePin, true, false), Msg::UiUnpin);
            assert_eq!(row_menu_label(ToggleFavorite, false, false), Msg::UiMenuFavorite);
            assert_eq!(row_menu_label(ToggleFavorite, false, true), Msg::UiMenuUnfavorite);
            assert_eq!(row_menu_label(Delete, false, false), Msg::UiDelete);
            for a in [Copy, CopyPlain, CopyAs, TogglePin, ToggleFavorite, Delete] {
                assert!(!t(lang, row_menu_label(a, false, false)).is_empty());
            }
        }
        assert_eq!(row_menu_key(Copy), Some(Msg::UiKeyEnter));
        assert_eq!(row_menu_key(Delete), Some(Msg::UiKeyDel));
    }

    #[test]
    fn copy_items_only_restore_even_when_enter_pastes() {
        use RowMenuAction::*;
        assert_eq!(copy_of(Copy), Some(CopyKind::Original));
        assert_eq!(copy_of(CopyPlain), Some(CopyKind::Plain));
        assert_eq!(copy_of(CopyAs), None);
        assert_eq!(copy_of(Delete), None);
        // The host copies with `Via::Other` (a menu is not the Enter key).
        assert_eq!(MENU_VIA, Via::Other);
        for enter_pastes in [false, true] {
            for a in [Copy, CopyPlain] {
                let kind = copy_of(a).unwrap_or(CopyKind::Original);
                assert_eq!(
                    after_copy(kind, MENU_VIA, enter_pastes, true),
                    AfterCopy::Restore,
                    "{a:?} with Enter pastes = {enter_pastes}"
                );
            }
        }
    }

    #[test]
    fn an_open_menu_wraps_and_resets_with_its_level() {
        let mut m = OpenMenu::new(MenuKind::More, 6);
        assert_eq!((m.level, m.highlighted, m.count), (0, 0, 6));
        m.highlighted = 4;
        m.enter_level(1, 3);
        assert_eq!((m.level, m.highlighted, m.count), (1, 0, 3));
        assert!(m.back());
        assert_eq!((m.level, m.highlighted, m.count), (0, 0, 6));
        assert!(!m.back(), "level 0 has nothing to go back to");
    }

    #[test]
    fn debug_of_menu_state_has_no_texts() {
        let m = OpenMenu::new(MenuKind::Row, 3);
        let s = format!("{m:?}");
        assert!(s.contains("Row"));
    }
}
