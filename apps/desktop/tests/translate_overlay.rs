//! Task 606: the translate overlay window, headless (GC-19): a software-rendered
//! `MinimalSoftwareWindow`, no OS window, no real input. A pointer click on the close button
//! runs `cancel`, and each state renders its own icon, border and spinner (the spinner only
//! while sending). `set_platform` works once per process, so every case lives in the one
//! `#[test]` below.
//!
//! What this does not cover (owner rows): that the real OS window never takes the focus; that
//! is the Win32 style work in `src/ui/translate_host.rs` and `src/winsys.rs`, whose style bits
//! are read back by `winsys::tests::overlay_styles_read_back_on_our_own_hidden_window`.

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize};
use std::cell::Cell;
use std::rc::Rc;

slint::slint! {
    #[edition = "1.19"]
    export { TranslateOverlay } from "ui/translate_overlay.slint";
    export { IconName } from "ui/kit/icon.slint";
}

struct TestPlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
}

const W: usize = 340;
const H: usize = 120;

fn render(w: &Rc<MinimalSoftwareWindow>) -> Vec<Rgb565Pixel> {
    let mut buf = vec![Rgb565Pixel::default(); W * H];
    slint::platform::update_timers_and_animations();
    w.request_redraw();
    w.draw_if_needed(|renderer| {
        renderer.render(&mut buf, W);
    });
    buf
}

fn rgb(hex: u32) -> Rgb565Pixel {
    let r = ((hex >> 16) & 0xFF) as u8;
    let g = ((hex >> 8) & 0xFF) as u8;
    let b = (hex & 0xFF) as u8;
    Rgb565Pixel::from_rgb(r, g, b)
}

/// The theme defaults of `ui/theme.slint` (the test does not run `theme::apply`).
const ACCENT: u32 = 0x7887FF;
const SUCCESS: u32 = 0x5FD4AA;
const WARNING: u32 = 0xF2C14E;
const DANGER: u32 = 0xFF7A85;

/// The overlay's fill (`Theme.raised`), what anti-aliased icon edges blend with.
const RAISED: u32 = 0x232935;

fn channels(p: Rgb565Pixel) -> [f32; 3] {
    let v = p.0;
    [
        f32::from((v >> 11) & 0x1F) / 31.0,
        f32::from((v >> 5) & 0x3F) / 63.0,
        f32::from(v & 0x1F) / 31.0,
    ]
}

/// True when `p` is `color` blended at least half over the fill, the same amount in every
/// channel (an anti-aliased stroke of that colour, not another colour).
fn tinted(p: Rgb565Pixel, color: u32) -> bool {
    let (c, bg, p) = (channels(rgb(color)), channels(rgb(RAISED)), channels(p));
    let mut alphas = Vec::new();
    for i in 0..3 {
        let span = c[i] - bg[i];
        if span.abs() < 0.15 {
            if (p[i] - c[i]).abs() > 0.12 {
                return false;
            }
            continue;
        }
        alphas.push((p[i] - bg[i]) / span);
    }
    let lo = alphas.iter().copied().fold(f32::MAX, f32::min);
    let hi = alphas.iter().copied().fold(f32::MIN, f32::max);
    !alphas.is_empty() && lo >= 0.5 && hi <= 1.1 && hi - lo <= 0.2
}

/// Pixels of `color` in the icon column (x 8..28, y 6..30).
fn in_icon(buf: &[Rgb565Pixel], color: u32) -> usize {
    let mut n = 0;
    for y in 6..30 {
        for x in 8..28 {
            if tinted(buf[y * W + x], color) {
                n += 1;
            }
        }
    }
    n
}

/// Pixels of `color` on the left border (x 0, the first rows of the window).
fn on_border(buf: &[Rgb565Pixel], color: u32, h: usize) -> usize {
    let c = rgb(color);
    (2..h.saturating_sub(2)).filter(|y| buf[y * W] == c).count()
}

fn click(w: &Rc<MinimalSoftwareWindow>, x: f32, y: f32) {
    let pos = LogicalPosition::new(x, y);
    w.window()
        .dispatch_event(WindowEvent::PointerMoved { position: pos });
    w.window().dispatch_event(WindowEvent::PointerPressed {
        position: pos,
        button: PointerEventButton::Left,
    });
    w.window().dispatch_event(WindowEvent::PointerReleased {
        position: pos,
        button: PointerEventButton::Left,
    });
    slint::platform::update_timers_and_animations();
}

#[test]
fn overlay_close_click_cancels_and_states_render_their_icons() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(PhysicalSize::new(W as u32, H as u32));
    slint::platform::set_platform(Box::new(TestPlatform {
        window: window.clone(),
    }))
    .expect("platform set once");

    let ui = TranslateOverlay::new().unwrap();
    ui.set_message("Sending 12 characters to Anthropic…".into());
    ui.set_detail("Esc to cancel".into());
    ui.set_cancel_label("Cancel".into());
    ui.set_state(0);
    ui.set_active(true);
    let cancels = Rc::new(Cell::new(0));
    {
        let c = cancels.clone();
        ui.on_cancel(move || c.set(c.get() + 1));
    }
    ui.show().unwrap();
    let h = (ui.get_content_height().ceil() as usize).min(H);
    assert!(h >= 28, "content height {h}");

    // (a) A pointer click on the close button runs `cancel` once; the first hit from the right
    // edge along the button row is the button (the layout has no other control).
    let mut hit = None;
    'scan: for y in (4..(h.min(40) as i32)).step_by(2) {
        for x in (200..(W as i32)).rev().step_by(2) {
            click(&window, x as f32, y as f32);
            if cancels.get() > 0 {
                hit = Some((x, y));
                break 'scan;
            }
        }
    }
    assert!(hit.is_some(), "no click reached the close button");
    assert_eq!(cancels.get(), 1);
    let (x, y) = hit.unwrap();
    // The button sits at the right edge, in the top row.
    assert!(x >= (W as i32) - 40, "close button at x {x}");
    assert!(y < 40, "close button at y {y}");
    // A click on the text does nothing.
    click(&window, 120.0, 14.0);
    assert_eq!(cancels.get(), 1);

    // (b) Each state's icon, tone and spinner.
    // Sending: the spinner's accent head, no state icon colour.
    let buf = render(&window);
    assert!(in_icon(&buf, ACCENT) > 0, "sending: no spinner");
    assert_eq!(in_icon(&buf, SUCCESS), 0);
    // The spinner stands still with reduced motion, and is still drawn.
    ui.set_reduce_motion(true);
    assert!(in_icon(&render(&window), ACCENT) > 0);
    ui.set_reduce_motion(false);

    // Done: the check in the success colour, no spinner, neutral border.
    ui.set_state(1);
    ui.set_state_icon(IconName::Check);
    ui.set_tone(0);
    ui.set_message("Translated".into());
    let buf = render(&window);
    assert!(in_icon(&buf, SUCCESS) > 0, "done: no success icon");
    assert_eq!(in_icon(&buf, ACCENT), 0, "done: spinner still drawn");
    assert_eq!(on_border(&buf, DANGER, h), 0);
    assert_eq!(on_border(&buf, WARNING, h), 0);

    // Cancelled: the cross, neutral, no spinner.
    ui.set_state(2);
    ui.set_state_icon(IconName::X);
    let buf = render(&window);
    assert_eq!(in_icon(&buf, ACCENT), 0);
    assert_eq!(in_icon(&buf, SUCCESS), 0);

    // Left on the clipboard and blocked: the warning tone on the border and the icon.
    for state in [3, 4] {
        ui.set_state(state);
        ui.set_state_icon(IconName::Info);
        ui.set_tone(1);
        let buf = render(&window);
        assert!(
            on_border(&buf, WARNING, h) > 0,
            "state {state}: no warning border"
        );
        assert!(in_icon(&buf, WARNING) > 0, "state {state}: no warning icon");
        assert_eq!(in_icon(&buf, ACCENT), 0, "state {state}: spinner drawn");
    }

    // Blocked in an excluded app and failed: the danger tone, the lock or info icon.
    for (state, icon) in [(4, IconName::Lock), (5, IconName::Info)] {
        ui.set_state(state);
        ui.set_state_icon(icon);
        ui.set_tone(2);
        let buf = render(&window);
        assert!(
            on_border(&buf, DANGER, h) > 0,
            "state {state}: no danger border"
        );
        assert!(in_icon(&buf, DANGER) > 0, "state {state}: no danger icon");
        assert_eq!(in_icon(&buf, ACCENT), 0, "state {state}: spinner drawn");
    }

    // Back to sending: the spinner again.
    ui.set_state(0);
    ui.set_tone(0);
    assert!(in_icon(&render(&window), ACCENT) > 0);
}
