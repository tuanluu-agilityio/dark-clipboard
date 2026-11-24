//! Dark palette (D-037: dark only), the tokens of `docs/specs/ui-redesign/02-design-system.md`
//! section 2 (Task 700; before that the values followed the Darkshot dark theme). `apply`
//! copies them into the Slint `Theme` global, so this file is the single source of truth. Text colours must keep WCAG 4.5:1 on
//! their backgrounds (tested).
// The palette is only used by the Windows app for now (the UI runs on Windows first).
#![cfg_attr(not(windows), allow(dead_code))]

use crate::ui::Theme;
use slint::{Color, ComponentHandle};

pub const CANVAS: Color = Color::from_rgb_u8(0x11, 0x13, 0x18);
pub const SURFACE: Color = Color::from_rgb_u8(0x1B, 0x20, 0x28);
pub const RAISED: Color = Color::from_rgb_u8(0x23, 0x29, 0x35);
pub const SELECTED: Color = Color::from_rgb_u8(0x26, 0x2C, 0x45);
pub const BORDER: Color = Color::from_rgb_u8(0x2D, 0x34, 0x40);
pub const BORDER_STRONG: Color = Color::from_rgb_u8(0x4A, 0x52, 0x63);
pub const TEXT: Color = Color::from_rgb_u8(0xF3, 0xF5, 0xF9);
pub const TEXT_2: Color = Color::from_rgb_u8(0xA1, 0xAA, 0xB8);
pub const TEXT_3: Color = Color::from_rgb_u8(0x8F, 0x98, 0xA8);
pub const ACCENT: Color = Color::from_rgb_u8(0x78, 0x87, 0xFF);
pub const ON_ACCENT: Color = Color::from_rgb_u8(0x11, 0x13, 0x18);
pub const FOCUS: Color = Color::from_rgb_u8(0x9A, 0xA6, 0xFF);
pub const SUCCESS: Color = Color::from_rgb_u8(0x5F, 0xD4, 0xAA);
pub const WARNING: Color = Color::from_rgb_u8(0xF2, 0xC1, 0x4E);
pub const DANGER: Color = Color::from_rgb_u8(0xFF, 0x7A, 0x85);

// Old names, kept as aliases of the new tokens (the Slint `Theme` global has the same aliases).
#[allow(dead_code)]
pub const BG: Color = CANVAS;
#[allow(dead_code)]
pub const LINE: Color = BORDER;
#[allow(dead_code)]
pub const TEXT_MUTED: Color = TEXT_2;

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
    t.set_canvas(CANVAS);
    t.set_surface(SURFACE);
    t.set_raised(RAISED);
    t.set_selected(SELECTED);
    t.set_border(BORDER);
    t.set_border_strong(BORDER_STRONG);
    t.set_text(TEXT);
    t.set_text_2(TEXT_2);
    t.set_text_3(TEXT_3);
    t.set_accent(ACCENT);
    t.set_on_accent(ON_ACCENT);
    t.set_focus(FOCUS);
    t.set_success(SUCCESS);
    t.set_warning(WARNING);
    t.set_danger(DANGER);
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

    fn hex(c: Color) -> String {
        format!("#{:02X}{:02X}{:02X}", c.red(), c.green(), c.blue())
    }

    #[test]
    fn tokens_have_the_values_of_the_design_system() {
        // `docs/specs/ui-redesign/02-design-system.md` section 2.
        let expected = [
            (CANVAS, "#111318"),
            (SURFACE, "#1B2028"),
            (RAISED, "#232935"),
            (SELECTED, "#262C45"),
            (BORDER, "#2D3440"),
            (BORDER_STRONG, "#4A5263"),
            (TEXT, "#F3F5F9"),
            (TEXT_2, "#A1AAB8"),
            (TEXT_3, "#8F98A8"),
            (ACCENT, "#7887FF"),
            (ON_ACCENT, "#111318"),
            (FOCUS, "#9AA6FF"),
            (SUCCESS, "#5FD4AA"),
            (WARNING, "#F2C14E"),
            (DANGER, "#FF7A85"),
        ];
        for (c, h) in expected {
            assert_eq!(hex(c), h);
        }
    }

    #[test]
    fn old_names_are_aliases_of_the_new_tokens() {
        assert_eq!(BG, CANVAS);
        assert_eq!(LINE, BORDER);
        assert_eq!(TEXT_MUTED, TEXT_2);
    }

    /// `05-acceptance.md` A1: every text token on every surface it is used on reaches 4.5:1.
    #[test]
    fn every_text_pair_reaches_wcag_aa() {
        let on_canvas_surface_selected = [TEXT, TEXT_2, TEXT_3, DANGER, SUCCESS, WARNING, FOCUS];
        for fg in on_canvas_surface_selected {
            for bg in [CANVAS, SURFACE, SELECTED] {
                let r = contrast(fg, bg);
                assert!(r >= 4.5, "{} on {} is {r:.2}", hex(fg), hex(bg));
            }
        }
        // Accent text: on canvas and surface only (on selected it is icon-only, below).
        for bg in [CANVAS, SURFACE] {
            let r = contrast(ACCENT, bg);
            assert!(r >= 4.5, "accent on {} is {r:.2}", hex(bg));
        }
        // Raised (hover rows, menus, key caps): everything except `text-3`.
        for fg in [TEXT, TEXT_2, ACCENT, DANGER, SUCCESS, WARNING, FOCUS] {
            let r = contrast(fg, RAISED);
            assert!(r >= 4.5, "{} on raised is {r:.2}", hex(fg));
        }
        let r = contrast(ON_ACCENT, ACCENT);
        assert!(r >= 4.5, "on-accent on accent is {r:.2}");
    }

    #[test]
    fn accent_on_selected_reaches_3_to_1_for_icons() {
        let r = contrast(ACCENT, SELECTED);
        assert!(r >= 3.0, "accent on selected is {r:.2}");
    }

    #[test]
    fn focus_ring_is_visible_on_every_surface() {
        for bg in [CANVAS, SURFACE, RAISED, SELECTED] {
            let r = contrast(FOCUS, bg);
            assert!(r >= 3.0, "focus on {} is {r:.2}", hex(bg));
        }
    }

    #[test]
    fn borders_stay_decorative_and_strong_borders_are_visible() {
        // `border` is decorative only (1.3 to 1.5:1); `border-strong` marks component edges.
        assert!(contrast(BORDER_STRONG, CANVAS) >= 2.3);
        assert!(contrast(BORDER_STRONG, SURFACE) > contrast(BORDER, SURFACE));
    }

    #[test]
    fn theme_is_dark() {
        assert!(relative_luminance(CANVAS) < 0.05);
    }
}
