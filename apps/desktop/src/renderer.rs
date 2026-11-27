//! Which Slint backend and renderer the UI uses (Task 212, D-055).
//!
//! On Windows the app asks for Slint's winit backend with the software renderer: Task 209
//! measured 6.9 MiB private bytes hidden and no idle CPU, against 52 MiB and ~0.2 % of a core
//! for the default GPU (FemtoVG/OpenGL) renderer. A `SLINT_BACKEND` set by the user always wins
//! (the app then selects nothing and Slint reads the variable itself). The choice is made
//! once, before any window exists. `renderer=software` in the start log means the software
//! renderer was requested and the selection call succeeded. If the selection fails the app
//! logs `renderer_fallback`, but on Windows that failure comes from creating the winit event
//! loop (the renderer name is not checked there), and winit 0.30 refuses a second event loop,
//! so Slint may then fail to start as well. A software renderer failure at window creation is
//! handled inside Slint, not here.
//!
//! The winit window accessors used by the popup (`slint::winit_030::WinitWindowAccessor`:
//! focus-loss events and bring-to-front) work with every renderer of the winit backend: they
//! downcast the window adapter to the winit adapter, which is the same for the software and
//! the FemtoVG renderer.

// The UI runs on Windows first; other platforms compile this but do not start the UI yet.
#![cfg_attr(not(windows), allow(dead_code))]

use std::ffi::OsStr;

/// What `select` will try, decided from the environment and the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// `SLINT_BACKEND` is set: select nothing, Slint follows the variable.
    UserChoice,
    /// Ask for the winit backend with the software renderer.
    Software,
    /// Leave Slint's built-in default (other platforms until Phase 4).
    Default,
}

/// Pure: the plan for a `SLINT_BACKEND` value (`None` when unset) on this platform. An empty
/// value counts as unset (it names no backend).
pub fn plan(slint_backend: Option<&OsStr>, windows: bool) -> Plan {
    if slint_backend.is_some_and(|v| !v.is_empty()) {
        Plan::UserChoice
    } else if windows {
        Plan::Software
    } else {
        Plan::Default
    }
}

/// What was chosen, for the start log (`renderer=<name>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chosen {
    /// `software`, `default`, or `env` (the user's `SLINT_BACKEND`).
    pub name: &'static str,
    /// The software renderer was wanted but could not be selected.
    pub fell_back: bool,
}

/// Pure: the result of a plan, given whether selecting the software renderer worked (only
/// looked at for `Plan::Software`).
pub fn outcome(plan: Plan, software_selected: bool) -> Chosen {
    let (name, fell_back) = match plan {
        Plan::UserChoice => ("env", false),
        Plan::Default => ("default", false),
        Plan::Software if software_selected => ("software", false),
        Plan::Software => ("default", true),
    };
    Chosen { name, fell_back }
}

/// Selects the backend for this process. Call once, before the first Slint window or timer.
/// A failed selection is reported (`fell_back`); Slint then tries its default backend when the
/// first window is created, which may fail too (see the module doc).
pub fn select() -> Chosen {
    let env = std::env::var_os("SLINT_BACKEND");
    let p = plan(env.as_deref(), cfg!(windows));
    let selected = p == Plan::Software
        && slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name("software".into())
            // Only the translate overlay's window is changed (inactive, topmost; Task 606).
            .with_winit_window_attributes_hook(crate::ui::overlay_window_attributes)
            .select()
            .is_ok();
    outcome(p, selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_defaults_to_the_software_renderer() {
        assert_eq!(plan(None, true), Plan::Software);
    }

    #[test]
    fn a_user_slint_backend_always_wins() {
        for v in ["winit-femtovg", "winit-software", "qt", "skia", "anything"] {
            assert_eq!(plan(Some(OsStr::new(v)), true), Plan::UserChoice, "{v}");
            assert_eq!(plan(Some(OsStr::new(v)), false), Plan::UserChoice, "{v}");
        }
    }

    #[test]
    fn an_empty_slint_backend_counts_as_unset() {
        assert_eq!(plan(Some(OsStr::new("")), true), Plan::Software);
        assert_eq!(plan(Some(OsStr::new("")), false), Plan::Default);
    }

    #[test]
    fn other_platforms_keep_the_default_until_phase_4() {
        assert_eq!(plan(None, false), Plan::Default);
    }

    #[test]
    fn the_software_renderer_is_logged_when_selected() {
        assert_eq!(
            outcome(Plan::Software, true),
            Chosen {
                name: "software",
                fell_back: false
            }
        );
    }

    #[test]
    fn a_failed_selection_falls_back_to_the_default() {
        assert_eq!(
            outcome(Plan::Software, false),
            Chosen {
                name: "default",
                fell_back: true
            }
        );
    }

    #[test]
    fn user_choice_and_default_never_fall_back() {
        for ok in [true, false] {
            assert_eq!(
                outcome(Plan::UserChoice, ok),
                Chosen {
                    name: "env",
                    fell_back: false
                }
            );
            assert_eq!(
                outcome(Plan::Default, ok),
                Chosen {
                    name: "default",
                    fell_back: false
                }
            );
        }
    }
}
