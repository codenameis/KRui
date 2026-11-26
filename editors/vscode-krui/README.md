# KRui Live Preview (`vscode-krui`)

VS Code extension providing `.krui` syntax highlighting, inline compile diagnostics, and an **interactive IDE side-panel Live Preview** powered directly by the compiled Rust app (`KRUI_EMBED_PANEL=1`) and `krui-renderer` (`Wgpu` GPU / `TinySkia` CPU).

## Features

- **Side-by-Side Real Renderer Preview** (`Ctrl+Shift+V` or the editor title icon on any `.krui` file): launches `krui serve --panel` and streams compressed RGBA8 (`"KRFR"`) frames from your live app right inside a VS Code editor panel beside your code.
- **Zero-Save Hot Reload**: edits in `.krui` and `.css` buffers recompile and update the live panel immediately without saving or losing `<input>` text/scroll state.
- **Korean / Japanese / English IME & Interactive Input**: full mouse, wheel, keyboard shortcut, and CJK IME composition (`compositionupdate` / `compositionend` UTF-8 byte offsets) inside `<canvas>` + `#ime-bridge` (password inputs stay direct non-IME).
- **Color Scheme, Device Bezel & Overlays**: toggle pure-CSS `System / Light / Dark` (`@media (prefers-color-scheme)`), Desktop / iPhone 15 / Pixel 8 / RPi 800x480 frames, and `relayout` / `FPS` / `Hit` overlays live from the toolbar.
- **Inspect & Jump to Source**: toggle `Inspect` (or `Alt+Click` on any element) or click an error banner to jump straight to the exact `.krui` line and column.
