// ============================================================================
// KRui Example 08: 순수 CSS 컴포넌트 갤러리 (Material 3 ↔ Tailwind v4)
// (`examples/component_gallery/main.rs`)
// ============================================================================
// 실행 명령:
//   - 데스크탑 창 실행: `cargo run --example component_gallery`
//   - 헤드리스 120프레임 검증: `cargo run --example component_gallery -- --headless`

#![forbid(unsafe_code)]

mod controller;
mod geometry_gates;
mod runner;
mod screenshots;

pub(crate) use controller::GalleryController;
pub(crate) use runner::{build_gallery_session, WINDOW_HEIGHT, WINDOW_WIDTH};

fn main() {
    runner::run_cli();
}
