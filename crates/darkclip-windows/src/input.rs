//! The menu mask key (Task 612): one key-down and key-up of the unassigned virtual key
//! `0xE8`, sent while the user still holds the Alt or Win key of a global shortcut.
//!
//! The shortcut's own key is swallowed by the hotkey, so the app in front sees Alt (or Win)
//! go down and come up with no other key between: a lone tap, which focuses the menu bar
//! (Alt) or opens the Start menu (Win). A key event in between makes it "Alt + another key",
//! so the release does nothing. `0xE8` is documented by Microsoft as unassigned
//! ("Virtual-Key Codes", Winuser.h), has no meaning in apps and produces no character; it is
//! the key AutoHotkey suggests for `#MenuMaskKey`.
//!
//! A closed key set: this module can send only `0xE8` down then up. The decision to send it is
//! the caller's (the translate host); nothing here reads a key state, a window or content, and
//! nothing is logged (GC-06).

// Only the Windows build sends it; the pure key list is tested everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

/// The unassigned virtual key used as the mask.
pub(crate) const MENU_MASK_VK: u16 = 0xE8;

/// The events of one mask, in order: `(virtual key, is key-up)`.
pub(crate) const fn menu_mask_keys() -> [(u16, bool); 2] {
    [(MENU_MASK_VK, false), (MENU_MASK_VK, true)]
}

/// Sends the menu mask key (`0xE8` down, then up) as one `SendInput` batch. Call it only while
/// the shortcut's Alt or Win key is still held: then the later release is no lone tap and no
/// menu opens. On a partial send a key-up is sent so the key never stays down.
#[cfg(windows)]
pub fn mask_menu_activation() -> Result<(), darkclip_platform::PlatformError> {
    crate::win::send_menu_mask()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mask_is_exactly_0xe8_down_then_up() {
        assert_eq!(menu_mask_keys(), [(0xE8, false), (0xE8, true)]);
    }

    // The INPUT array built from this list is checked in `win.rs` (all unsafe stays there).
}
