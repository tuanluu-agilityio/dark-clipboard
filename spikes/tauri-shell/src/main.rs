//! Phase 0 spike: can Tauri 2 host Dark Clipboard's desktop shell on Windows?
//! Checks tray icon, global shortcut registration, a hidden always-on-top undecorated popup,
//! programmatic show/hide, and startup time. Size and memory are measured by the caller.
//! Usage: tauri-shell-spike [exit_after_seconds]
//! Throwaway code.
#![windows_subsystem = "windows"]

use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

fn log(line: &str) {
    use std::io::Write;
    let path = std::env::temp_dir().join("dark-clipboard-tauri-spike.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

fn toggle(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("popup") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}

fn main() {
    let t0 = Instant::now();
    let exit_after = std::env::args().nth(1).and_then(|s| s.parse::<u64>().ok());
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle(app);
                    }
                })
                .build(),
        )
        .setup(move |app| {
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(|app, e| {
                    if e.id() == "quit" {
                        app.exit(0)
                    }
                })
                .build(app)?;
            log("tray: ok");
            let sc = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::F12);
            match app.global_shortcut().register(sc) {
                Ok(()) => log("global shortcut Ctrl+Shift+F12: registered"),
                Err(e) => log(&format!("global shortcut: FAILED {e}")),
            }
            let w = app.get_webview_window("popup").unwrap();
            log(&format!("always_on_top: {:?}", w.is_always_on_top()));
            w.show()?;
            log(&format!("popup visible after show: {:?}", w.is_visible()));
            w.hide()?;
            log(&format!("popup visible after hide: {:?}", w.is_visible()));
            log(&format!("setup ready in {} ms", t0.elapsed().as_millis()));
            if let Some(s) = exit_after {
                let h = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(s));
                    log("exit requested by timer");
                    h.exit(0);
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("build tauri app")
        .run(|_app, event| {
            // Tray app: closing the popup must not quit the process.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
