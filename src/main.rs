//! KRui Showcase Binary (`krui-showcase.exe`)
//!
//! Launches the pure-CSS Component Gallery showcase (`examples/component_gallery`) in desktop,
//! `--headless`, `--snapshot <path>`, or `--screenshots <dir>` verification mode.

#![forbid(unsafe_code)]

#[path = "../examples/component_gallery/controller/mod.rs"]
mod controller;
#[path = "../examples/component_gallery/geometry_gates.rs"]
mod geometry_gates;
#[path = "../examples/component_gallery/runner.rs"]
mod runner;
#[path = "../examples/component_gallery/screenshots.rs"]
mod screenshots;

pub(crate) use controller::GalleryController;
pub(crate) use runner::{build_gallery_session, WINDOW_HEIGHT, WINDOW_WIDTH};

fn main() {
    runner::run_cli();
}
