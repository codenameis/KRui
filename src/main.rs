//! KRui Showcase Binary (`krui-showcase.exe`)
//!
//! Launches the pure-CSS Component Gallery showcase (`examples/component_gallery`) in desktop,
//! `--headless`, `--snapshot <path>`, or `--screenshots <dir>` verification mode.

#![forbid(unsafe_code)]
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

#[path = "../examples/component_gallery/controller/mod.rs"]
mod controller;
#[path = "../examples/component_gallery/desktop_tray.rs"]
mod desktop_tray;
#[path = "../examples/component_gallery/geometry_gates.rs"]
mod geometry_gates;
#[path = "../examples/component_gallery/interaction_gates.rs"]
mod interaction_gates;
#[path = "../examples/component_gallery/runner.rs"]
mod runner;
#[path = "../examples/component_gallery/screenshots.rs"]
mod screenshots;
#[path = "../examples/component_gallery/viewport3d/mod.rs"]
mod viewport3d;

pub(crate) use controller::GalleryController;
pub(crate) use runner::{build_gallery_session, WINDOW_HEIGHT, WINDOW_WIDTH};

fn main() {
    runner::run_cli();
}
