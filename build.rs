//! Embeds the KRui application icon into Windows executables (`assets/krui_icon.ico`).

#![forbid(unsafe_code)]

fn main() {
    println!("cargo:rerun-if-changed=assets/krui_icon.ico");
    #[cfg(all(windows, feature = "desktop"))]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::path::Path::new("assets/krui_icon.ico").is_file()
    {
        if let Err(err) = compile_windows_icon() {
            panic!("failed to embed Windows icon resource `assets/krui_icon.ico`: {err}");
        }
    }
}

#[cfg(all(windows, feature = "desktop"))]
fn compile_windows_icon() -> std::io::Result<()> {
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/krui_icon.ico");
    res.compile_for("krui-showcase")
}
