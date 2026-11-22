//! Compiles the Slint markup into Rust (`slint::include_modules!` in `src/ui/mod.rs`).

fn main() {
    if slint_build::compile("ui/app.slint").is_err() {
        // The compiler already printed its diagnostics; fail the build.
        std::process::exit(1);
    }
}
