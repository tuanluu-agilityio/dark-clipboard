//! Compiles the Slint markup into Rust (`slint::include_modules!` in `src/ui/mod.rs`) and, when
//! the target is Windows, embeds the app icon (`res/app.rc`) into the exe.

fn main() {
    println!("cargo:rerun-if-changed=res/app.rc");
    println!("cargo:rerun-if-changed=res/dark-clipboard.ico");
    if slint_build::compile("ui/app.slint").is_err() {
        // The compiler already printed its diagnostics; fail the build.
        std::process::exit(1);
    }
    embed_icon();
}

/// The `embed-resource` crate is a Windows-only build dependency, so this is a no-op elsewhere.
#[cfg(windows)]
fn embed_icon() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // A broken or missing resource fails the build loudly: the exe icon is part of the app.
        if embed_resource::compile("res/app.rc", embed_resource::NONE)
            .manifest_required()
            .is_err()
        {
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
fn embed_icon() {}
