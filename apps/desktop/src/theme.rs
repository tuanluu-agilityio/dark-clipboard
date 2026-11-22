//! Dark palette (D-037: dark only). Values follow the Darkshot dark theme (read-only reference,
//! numbers only); the code is new. `apply` copies them into the Slint `Theme` global so this
//! file is the single source of truth. Text colours must keep WCAG 4.5:1 on their backgrounds
//! (tested).

use crate::ui::{MainWindow, Theme};
use slint::{Color, ComponentHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `0xRRGGBB`.
    pub const fn hex(v: u32) -> Rgb {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    fn color(self) -> Color {
        Color::from_rgb_u8(self.0, self.1, self.2)
    }
}

pub const BG: Rgb = Rgb::hex(0x1C1E22);
pub const SURFACE: Rgb = Rgb::hex(0x24272C);
pub const LINE: Rgb = Rgb::hex(0x383C43);
pub const TEXT: Rgb = Rgb::hex(0xE7E9EC);
pub const TEXT_MUTED: Rgb = Rgb::hex(0xA3A9B2);
pub const ACCENT: Rgb = Rgb::hex(0x4C8DFF);
pub const DANGER: Rgb = Rgb::hex(0xFF7B72);
pub const SUCCESS: Rgb = Rgb::hex(0x4ADE80);
pub const WARNING: Rgb = Rgb::hex(0xF5C04A);

/// (text, background) pairs in use; each must reach 4.5:1.
#[cfg(test)]
pub const TEXT_PAIRS: &[(Rgb, Rgb)] = &[
    (TEXT, BG),
    (TEXT, SURFACE),
    (TEXT_MUTED, BG),
    (TEXT_MUTED, SURFACE),
    (ACCENT, BG),
    (ACCENT, SURFACE),
    (DANGER, BG),
    (SUCCESS, BG),
    (WARNING, BG),
];

#[cfg(test)]
fn linear(c: u8) -> f64 {
    let s = f64::from(c) / 255.0;
    if s <= 0.039_28 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
fn luminance(c: Rgb) -> f64 {
    0.2126 * linear(c.0) + 0.7152 * linear(c.1) + 0.0722 * linear(c.2)
}

/// WCAG 2.x contrast ratio.
#[cfg(test)]
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Sets the Slint `Theme` global of `window`.
pub fn apply(window: &MainWindow) {
    let t = window.global::<Theme>();
    t.set_bg(BG.color());
    t.set_surface(SURFACE.color());
    t.set_line(LINE.color());
    t.set_text(TEXT.color());
    t.set_text_muted(TEXT_MUTED.color());
    t.set_accent(ACCENT.color());
    t.set_danger(DANGER.color());
    t.set_success(SUCCESS.color());
    t.set_warning(WARNING.color());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_splits_channels() {
        assert_eq!(Rgb::hex(0x1C1E22), Rgb(0x1C, 0x1E, 0x22));
    }

    #[test]
    fn every_text_pair_reaches_wcag_aa() {
        for (fg, bg) in TEXT_PAIRS {
            let r = contrast_ratio(*fg, *bg);
            assert!(r >= 4.5, "{fg:?} on {bg:?} is {r:.2}");
        }
    }

    #[test]
    fn theme_is_dark() {
        assert!(luminance(BG) < 0.05);
    }
}
