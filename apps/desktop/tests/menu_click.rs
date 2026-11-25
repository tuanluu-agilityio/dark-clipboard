//! Task 713 regression test: a click on a menu entry must run `activated`, and the pointer
//! moving over an entry must run `hovered`. Headless (GC-19): a software-rendered
//! `MinimalSoftwareWindow`, no OS window. `set_platform` works once per process, so every case
//! lives in the one `#[test]` below.

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize};
use std::cell::RefCell;
use std::rc::Rc;

slint::slint! {
    #[edition = "1.19"]
    import { Menu, MenuEntry } from "ui/kit/menu.slint";

    export component Harness inherits Window {
        width: 300px;
        height: 300px;
        in-out property <[MenuEntry]> entries;
        in-out property <int> highlighted: 0;
        callback activated(int);
        callback hovered(int);
        public function open() {
            m.open(10px, 10px);
        }
        m := Menu {
            x: 0px;
            y: 0px;
            entries: root.entries;
            highlighted: root.highlighted;
            activated(i) => { root.activated(i); }
            hovered(i) => { root.hovered(i); }
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

fn entry(label: &str, submenu: bool) -> MenuEntry {
    MenuEntry {
        label: label.into(),
        icon: Default::default(),
        shortcut: "".into(),
        danger: false,
        submenu,
        checked: false,
    }
}

fn settle(w: &Rc<MinimalSoftwareWindow>) {
    slint::platform::update_timers_and_animations();
    w.draw_if_needed(|renderer| {
        let mut buf = vec![slint::platform::software_renderer::Rgb565Pixel::default(); 300 * 300];
        renderer.render(&mut buf, 300);
    });
}

fn click(w: &Rc<MinimalSoftwareWindow>, x: f32, y: f32) {
    let pos = LogicalPosition::new(x, y);
    w.window()
        .dispatch_event(WindowEvent::PointerMoved { position: pos });
    settle(w);
    w.window().dispatch_event(WindowEvent::PointerPressed {
        position: pos,
        button: PointerEventButton::Left,
    });
    settle(w);
    w.window().dispatch_event(WindowEvent::PointerReleased {
        position: pos,
        button: PointerEventButton::Left,
    });
    settle(w);
}

#[test]
fn menu_click_activates_and_hover_reports() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(PhysicalSize::new(300, 300));
    slint::platform::set_platform(Box::new(TestPlatform {
        window: window.clone(),
    }))
    .expect("platform set once");

    let ui = Harness::new().unwrap();
    ui.set_entries(
        Rc::new(slint::VecModel::from(vec![
            entry("A", false),
            entry("B", false),
            entry("Sub", true),
        ]))
        .into(),
    );
    let activated = Rc::new(RefCell::new(Vec::<i32>::new()));
    let hovered = Rc::new(RefCell::new(Vec::<i32>::new()));
    {
        let a = activated.clone();
        let weak = ui.as_weak();
        ui.on_activated(move |i| {
            a.borrow_mut().push(i);
            // Like the host: a submenu click swaps the entries (to non-submenu ones) inside
            // the callback, and the menu must stay open.
            if i == 2 {
                if let Some(ui) = weak.upgrade() {
                    // Rewrite the rows of the same model in place: the repeated row that is
                    // running this handler sees its new entry at once.
                    use slint::Model;
                    let model = ui.get_entries();
                    for (r, l) in ["X", "Y", "Z"].iter().enumerate() {
                        model.set_row_data(r, entry(l, false));
                    }
                }
            }
        });
        let h = hovered.clone();
        ui.on_hovered(move |i| h.borrow_mut().push(i));
    }
    ui.show().unwrap();
    settle(&window);

    // Menu at (10, 10), padding 4, rows 28 px: row i centre y = 10 + 4 + 28 * i + 14.
    ui.invoke_open();
    settle(&window);

    // (b) a plain move (no button) over row 1 reports hovered(1).
    window.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(60.0, 56.0),
    });
    settle(&window);
    assert!(
        hovered.borrow().contains(&1),
        "hovered(1) expected, got {:?}",
        hovered.borrow()
    );

    // (a) a click on row 1 calls activated(1) exactly once.
    click(&window, 60.0, 56.0);
    assert_eq!(*activated.borrow(), vec![1], "activated(1) exactly once");

    // (c) a submenu entry click calls activated and leaves the popup open: a second click on
    // another row still works because the popup is still there.
    activated.borrow_mut().clear();
    ui.invoke_open();
    settle(&window);
    click(&window, 60.0, 84.0);
    assert_eq!(*activated.borrow(), vec![2], "submenu entry activated");
    click(&window, 60.0, 28.0);
    assert_eq!(
        *activated.borrow(),
        vec![2, 0],
        "popup stayed open after a submenu click that swapped the entries"
    );
}
