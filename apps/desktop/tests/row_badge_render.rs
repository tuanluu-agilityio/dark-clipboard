//! Task 714: a selected favorite row still draws its star beside the row actions. Headless
//! (GC-19): a software-rendered `MinimalSoftwareWindow`, no OS window. `set_platform` works once
//! per process, so the one `#[test]` below holds every case.

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::ComponentHandle;
use std::rc::Rc;

slint::slint! {
    #[edition = "1.19"]
    import { PopupRow } from "ui/popup_row.slint";
    import { ClipRow } from "ui/popup.slint";

    export component Harness inherits Window {
        width: 440px;
        height: 48px;
        in-out property <ClipRow> data;
        in-out property <bool> selected;
        PopupRow {
            x: 0px;
            y: 0px;
            width: 440px;
            data: root.data;
            selected: root.selected;
        }
    }
}

struct TestPlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
}

const W: usize = 440;
const H: usize = 48;

fn render(w: &Rc<MinimalSoftwareWindow>) -> Vec<slint::platform::software_renderer::Rgb565Pixel> {
    let mut buf = vec![slint::platform::software_renderer::Rgb565Pixel::default(); W * H];
    slint::platform::update_timers_and_animations();
    w.draw_if_needed(|renderer| {
        renderer.render(&mut buf, W);
    });
    buf
}

/// Pixels that differ in the strip left of the row actions (x from 440 - 8 - 92 - 40 to
/// 440 - 8 - 92).
fn strip_diff(
    a: &[slint::platform::software_renderer::Rgb565Pixel],
    b: &[slint::platform::software_renderer::Rgb565Pixel],
) -> usize {
    let mut n = 0;
    for y in 0..H {
        for x in (W - 8 - 92 - 40)..(W - 8 - 92) {
            if a[y * W + x] != b[y * W + x] {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn a_selected_row_draws_its_favorite_star_and_masked_eye() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(slint::PhysicalSize::new(W as u32, H as u32));
    slint::platform::set_platform(Box::new(TestPlatform {
        window: window.clone(),
    }))
    .expect("platform");
    let ui = Harness::new().expect("harness");
    ui.show().expect("show");

    let mut plain = ui.get_data();
    plain.meta = "Text - now".into();
    ui.set_selected(true);
    ui.set_data(plain.clone());
    let base = render(&window);

    let mut fav = plain.clone();
    fav.favorite = true;
    ui.set_data(fav);
    let with_star = render(&window);
    assert!(
        strip_diff(&base, &with_star) > 10,
        "a selected favorite row must draw a star"
    );

    let mut masked = plain;
    masked.masked = true;
    ui.set_data(masked);
    let with_eye = render(&window);
    assert!(
        strip_diff(&base, &with_eye) > 10,
        "a selected masked row must draw an eye-off"
    );
}
