//! Dark palette (D-037: dark only). The colour values follow the Darkshot dark theme (numbers
//! only, read from its repo); the code is new. `apply` copies them into the Slint `Theme`
//! global, so this file is the single source of truth. Text colours must keep WCAG 4.5:1 on
//! their backgrounds (tested).
// The palette is only used by the Windows app for now (the UI runs on Windows first).
#![cfg_attr(not(windows), allow(dead_code))]

use crate::ui::Theme;
use slint::{Color, ComponentHandle};

pub const BG: Color = Color::from_rgb_u8(0x1C, 0x1E, 0x22);
pub const SURFACE: Color = Color::from_rgb_u8(0x24, 0x27, 0x2C);
pub const LINE: Color = Color::from_rgb_u8(0x38, 0x3C, 0x43);
pub const TEXT: Color = Color::from_rgb_u8(0xE7, 0xE9, 0xEC);
pub const TEXT_MUTED: Color = Color::from_rgb_u8(0xA3, 0xA9, 0xB2);
pub const ACCENT: Color = Color::from_rgb_u8(0x4C, 0x8D, 0xFF);
pub const DANGER: Color = Color::from_rgb_u8(0xFF, 0x7B, 0x72);
pub const SUCCESS: Color = Color::from_rgb_u8(0x4A, 0xDE, 0x80);
pub const WARNING: Color = Color::from_rgb_u8(0xF5, 0xC0, 0x4A);

/// A monospace face that ships with the OS (code clips).
#[cfg(windows)]
pub const MONO_FONT: &str = "Consolas";
#[cfg(target_os = "macos")]
pub const MONO_FONT: &str = "Menlo";
#[cfg(not(any(windows, target_os = "macos")))]
pub const MONO_FONT: &str = "DejaVu Sans Mono";

/// Sets the Slint `Theme` global of `window`.
pub fn apply<'a, C: ComponentHandle>(window: &'a C)
where
    Theme<'a>: slint::Global<'a, C>,
{
    let t = window.global::<Theme>();
    t.set_bg(BG);
    t.set_surface(SURFACE);
    t.set_line(LINE);
    t.set_text(TEXT);
    t.set_text_muted(TEXT_MUTED);
    t.set_accent(ACCENT);
    t.set_danger(DANGER);
    t.set_success(SUCCESS);
    t.set_warning(WARNING);
    t.set_mono_font(MONO_FONT.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Linearised value of one sRGB channel, from the WCAG 2.x definition.
    fn channel_linear(v: u8) -> f64 {
        let c = f64::from(v) / 255.0;
        if c > 0.03928 {
            ((c + 0.055) / 1.055).powf(2.4)
        } else {
            c / 12.92
        }
    }

    /// Relative luminance: weighted sum of the linearised channels.
    fn relative_luminance(c: Color) -> f64 {
        [(c.red(), 0.2126), (c.green(), 0.7152), (c.blue(), 0.0722)]
            .iter()
            .map(|&(v, w)| w * channel_linear(v))
            .sum()
    }

    /// Contrast ratio: (lighter + 0.05) / (darker + 0.05).
    fn contrast(a: Color, b: Color) -> f64 {
        let (x, y) = (relative_luminance(a), relative_luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn contrast_function_matches_the_wcag_extremes() {
        let black = Color::from_rgb_u8(0, 0, 0);
        let white = Color::from_rgb_u8(255, 255, 255);
        assert!((contrast(black, white) - 21.0).abs() < 1e-9);
        assert!((contrast(white, white) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn every_text_pair_reaches_wcag_aa() {
        let pairs = [
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
        for (fg, bg) in pairs {
            let r = contrast(fg, bg);
            assert!(r >= 4.5, "{fg:?} on {bg:?} is {r:.2}");
        }
    }

    #[test]
    fn theme_is_dark() {
        assert!(relative_luminance(BG) < 0.05);
    }
}
