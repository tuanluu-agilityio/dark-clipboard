//! Phase 0 spike: Slint shell for Dark Clipboard on Windows, measured against the Tauri spike.
//! Tray icon (tray-icon), global shortcut (global-hotkey), undecorated always-on-top popup,
//! programmatic show/hide, startup time. Usage: slint-shell-spike [exit_after_seconds]
//! Throwaway code.
#![windows_subsystem = "windows"]

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::time::{Duration, Instant};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};

slint::slint! {
    import { LineEdit } from "std-widgets.slint";
    export component Popup inherits Window {
        always-on-top: true;
        no-frame: true;
        width: 420px;
        height: 520px;
        background: #16181d;
        in-out property <string> query;
        VerticalLayout {
            padding: 8px;
            spacing: 4px;
            LineEdit { text <=> root.query; placeholder-text: "Search clips"; }
            for i in 12: Text { text: "clip " + i; color: #e6e6e6; }
        }
    }
}

fn log(line: &str) {
    use std::io::Write;
    let path = std::env::temp_dir().join("dark-clipboard-slint-spike.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

fn main() {
    let t0 = Instant::now();
    let exit_after = std::env::args().nth(1).and_then(|s| s.parse::<u64>().ok());

    let popup = Popup::new().expect("create popup");

    let menu = Menu::new();
    let quit = MenuItem::new("Quit", true, None);
    let quit_id = quit.id().clone();
    menu.append(&quit).expect("menu");
    let rgba: Vec<u8> = std::iter::repeat([30u8, 34, 42, 255]).take(32 * 32).flatten().collect();
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(Icon::from_rgba(rgba, 32, 32).expect("icon"))
        .with_tooltip("Dark Clipboard spike")
        .build();
    log(&format!("tray: {}", if tray.is_ok() { "ok" } else { "FAILED" }));

    let manager = GlobalHotKeyManager::new().expect("hotkey manager");
    let hk = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::F12);
    match manager.register(hk) {
        Ok(()) => log("global shortcut Ctrl+Shift+F12: registered"),
        Err(e) => log(&format!("global shortcut: FAILED {e}")),
    }

    popup.show().expect("show");
    log(&format!("popup visible after show: {}", popup.window().is_visible()));
    popup.hide().expect("hide");
    log(&format!("popup visible after hide: {}", popup.window().is_visible()));

    // Poll hotkey and menu channels from the UI thread.
    let weak = popup.as_weak();
    let poll = slint::Timer::default();
    poll.start(slint::TimerMode::Repeated, Duration::from_millis(50), move || {
        while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
            if ev.state == HotKeyState::Pressed {
                if let Some(p) = weak.upgrade() {
                    if p.window().is_visible() {
                        let _ = p.hide();
                    } else {
                        let _ = p.show();
                    }
                }
            }
        }
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == quit_id {
                let _ = slint::quit_event_loop();
            }
        }
    });
    log(&format!("setup ready in {} ms", t0.elapsed().as_millis()));
    if let Some(s) = exit_after {
        slint::Timer::single_shot(Duration::from_secs(s), || {
            log("exit requested by timer");
            let _ = slint::quit_event_loop();
        });
    }
    slint::run_event_loop_until_quit().expect("event loop");
    drop(tray);
}
