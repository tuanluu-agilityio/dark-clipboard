//! The "Copy as..." panel's pure state and keys (Task 404), shared by the popup and the
//! history window. No clip content is held here: only the clip id, the highlighted transform
//! and a request counter that drops stale previews.

use super::keys::Key;
use darkclip_core::transform::Transform;
use darkclip_core::{ClipId, ClipType};

/// Text transforms and editing apply to text-like clips that are not masked. Images and file
/// lists have no text to work on; a masked clip's text is never read.
pub fn can_transform(kind: ClipType, masked: bool) -> bool {
    !masked && !matches!(kind, ClipType::Image | ClipType::Files)
}

/// What a key does while the panel is open. The panel is modal: every other key is swallowed,
/// so nothing happens in the list behind it (and nothing is typed into the search box).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKey {
    Previous,
    Next,
    Copy,
    Close,
    Swallow,
}

pub fn panel_key(k: Key) -> PanelKey {
    match k {
        Key::Up => PanelKey::Previous,
        Key::Down => PanelKey::Next,
        Key::Enter => PanelKey::Copy,
        Key::Escape => PanelKey::Close,
        _ => PanelKey::Swallow,
    }
}

/// One open panel: which clip, which transform is highlighted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    clip: ClipId,
    selected: usize,
    gen: u64,
}

impl Session {
    pub fn new(clip: ClipId) -> Self {
        Self {
            clip,
            selected: 0,
            gen: 0,
        }
    }

    pub fn clip(&self) -> ClipId {
        self.clip
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn transform(&self) -> Transform {
        Transform::ALL
            .get(self.selected)
            .copied()
            .unwrap_or(Transform::ALL[0])
    }

    /// Highlights transform `i`; false when it is out of range or already highlighted.
    pub fn select(&mut self, i: usize) -> bool {
        if i >= Transform::ALL.len() || i == self.selected {
            return false;
        }
        self.selected = i;
        true
    }

    /// Up and Down stop at the ends. True when the highlight moved.
    pub fn step(&mut self, k: PanelKey) -> bool {
        match k {
            PanelKey::Previous if self.selected > 0 => self.select(self.selected - 1),
            PanelKey::Next => self.select(self.selected + 1),
            _ => false,
        }
    }

    /// A new preview request: answers to older requests are stale.
    pub fn next_gen(&mut self) -> u64 {
        self.gen += 1;
        self.gen
    }

    pub fn is_current(&self, gen: u64) -> bool {
        self.gen == gen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_core::Timestamp;

    fn id() -> ClipId {
        ClipId::new_v7(Timestamp(1_795_320_000_000))
    }

    #[test]
    fn only_unmasked_text_like_clips_can_be_transformed() {
        for k in [
            ClipType::Text,
            ClipType::RichText,
            ClipType::Url,
            ClipType::Color,
            ClipType::Code,
        ] {
            assert!(can_transform(k, false), "{k:?}");
            assert!(!can_transform(k, true), "{k:?} masked");
        }
        assert!(!can_transform(ClipType::Image, false));
        assert!(!can_transform(ClipType::Files, false));
    }

    #[test]
    fn panel_keys_are_modal() {
        assert_eq!(panel_key(Key::Up), PanelKey::Previous);
        assert_eq!(panel_key(Key::Down), PanelKey::Next);
        assert_eq!(panel_key(Key::Enter), PanelKey::Copy);
        assert_eq!(panel_key(Key::Escape), PanelKey::Close);
        for k in [Key::Delete, Key::Tab, Key::Char('a'), Key::Left, Key::Other] {
            assert_eq!(panel_key(k), PanelKey::Swallow, "{k:?}");
        }
    }

    #[test]
    fn the_highlight_stops_at_both_ends() {
        let mut s = Session::new(id());
        assert_eq!(s.selected(), 0);
        assert_eq!(s.transform(), Transform::ALL[0]);
        assert!(!s.step(PanelKey::Previous));
        assert!(s.step(PanelKey::Next));
        assert_eq!(s.transform(), Transform::ALL[1]);
        for _ in 0..50 {
            s.step(PanelKey::Next);
        }
        assert_eq!(s.selected(), Transform::ALL.len() - 1);
        assert!(!s.step(PanelKey::Next));
        assert!(!s.step(PanelKey::Copy));
        assert!(s.step(PanelKey::Previous));
    }

    #[test]
    fn select_rejects_out_of_range_and_repeats() {
        let mut s = Session::new(id());
        assert!(!s.select(0));
        assert!(!s.select(Transform::ALL.len()));
        assert!(s.select(4));
        assert_eq!(s.transform(), Transform::ALL[4]);
    }

    #[test]
    fn only_the_latest_request_is_current() {
        let mut s = Session::new(id());
        let a = s.next_gen();
        assert!(s.is_current(a));
        let b = s.next_gen();
        assert!(!s.is_current(a));
        assert!(s.is_current(b));
    }
}
