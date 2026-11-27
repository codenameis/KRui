# KRui Build Options & Runtime Flags

All Cargo profiles, feature flags, executable targets, platform host crates, `krui serve` CLI options, and environment variables currently defined in [`Cargo.toml`](../Cargo.toml), [`crates/*/Cargo.toml`](../crates), and [`crates/cli/src/args.rs`](../crates/cli/src/args.rs).

---

## 1. Cargo Profiles ([`Cargo.toml`](../Cargo.toml))

| Profile | Invocation | Optimization / LTO / Debug | Intended Use |
|---|---|---|---|
| `dev` *(default)* | `cargo build` / `cargo run` | External dependencies `opt-level = 2`, line tables only | Fast iterative development, `--features preview`, `krui serve` |
| `release` | `cargo build --release` | `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"` | Desktop/production binaries, `krui-bench` (`component_gallery` uses Windows GUI subsystem) |
| `kiosk` | `cargo build --profile kiosk` | Inherits `release` with `opt-level = "z"`, `lto = "fat"`, `panic = "abort"`, `strip = "symbols"` | Minimal binary footprint for embedded Linux DRM/SPI kiosks |

---

## 2. Cargo Feature Flags

### Root Crate (`krui`)
| Feature | Default | Implies | Behavior |
|---|---|---|---|
| `desktop` | **Yes** | `krui-renderer/gpu`, `dep:krui-winit`, `dep:pollster`, `dep:wgpu`, `dep:winit` | Full desktop GUI host with `wgpu` GPU + `TinySkia` CPU fallback |
| `kiosk` | No | *(marker for headless/DRM builds)* | Pair with `--no-default-features --features kiosk` to exclude `winit`/`wgpu` |
| `preview` | No | `desktop`, `dep:krui-preview`, and `preview` across `core`/`winit`/`patterns`/`macros`/`style` | Dev-only TCP live reload, IR serialization, template interpreter, and registry |

### Workspace Crates
| Crate | Default | Optional Features | Purpose |
|---|---|---|---|
| `krui-renderer` | `["gpu"]` | `--no-default-features` | Disables `wgpu`/`lyon`/`pollster`; keeps pure-Rust `TinySkiaRenderer` only |
| `krui-winit` | `["gpu"]` | `gpu`, `preview` | Desktop `winit` host (`EmbeddedPanelDriver` when `preview` enabled) |
| `krui-android-ffi` | `[]` | `preview` (`cdylib`, `rlib`) | Android JNI (`5` `#[no_mangle]`, `0` `unsafe {}`) + optional TCP hot-reload |
| `krui-compiler` | `[]` | `preview` | Enables `serde` + `postcard` IR bundle serialization |
| `krui-{core,macros,patterns,style}` | `[]` | `preview` | Enables `ScopeRegistry`, `Value` reflection, and `preview` serialization |

---

## 3. Binaries, Examples & Platform Host Crates

| Target / Crate | Command | Notes |
|---|---|---|
| `krui-showcase` | `cargo run -p krui --bin krui-showcase [-- --headless]` | Desktop application shell (`required-features = ["desktop"]`) |
| `krui-bench` | `cargo run --release --bin krui-bench [-- --allow-missing-wgpu]` | Controlled 30-warmup + 300-frame benchmark (`TinySkia` & `Wgpu` `p50`/`p99 <= 8,333us`) |
| `component_gallery` | `cargo run [--release] --example component_gallery [-- --headless \| --screenshots [DIR] \| --snapshot <PATH>]` | 47 starter built-ins + Material 3 / Tailwind v4 pure CSS showcase & Gates 1–5 |
| `krui` CLI | `cargo run -p krui-cli -- serve [OPTIONS]` | Hot-reload TCP server & IDE `--panel` bridge |
| `krui-engine-host` | `cargo build -p krui-engine-host` | **Primary Production Tier**: In-pass UI overlay host sharing an external 3D engine's `wgpu::Device` & `tokio` handle |
| `krui-kms` | `cargo build -p krui-kms` | **Experimental / Adapter Preview**: Trait-backed Linux DRM DumbBuffer + `DIRTYFB` planner + `evdev` router |
| `krui-web` (`crates/web-dom`) | `cargo test -p krui-web` | **Experimental Prototype**: Standalone browser DOM + `<canvas>` backend (`DomBackend`, outside `ViewTree`/`stylo`/`taffy`) |

---

## 4. `krui serve` CLI Options ([`crates/cli/src/args.rs`](../crates/cli/src/args.rs))

```text
krui serve [--example <NAME> | --app <NAME>] [--frame iphone-15|pixel-8|rpi-800x480] [--overlay relayout|fps|hit]
```

| Flag | Default | Description |
|---|---|---|
| `-e, --example <NAME>` | `component_gallery` (via default watch fallback) | Build and run `examples/<NAME>` with `--features preview` |
| `--app, --bin <NAME>` | None | Build and run workspace member `apps/<NAME>` with `--features preview` |
| `-w, --watch <PATH>` | Auto-inferred | Explicit root `.krui` (or `components/*.krui`, `icons/*.krui`, companion `.css`) to watch |
| `-f, --frame <PRESET>` | None | Wrap viewport in bezel preset (`iphone-15`, `pixel-8`, `rpi-800x480`) |
| `-p, --port <PORT>` | `41731` | Local TCP port (`0` for ephemeral OS-assigned port) |
| `--host <ADDR>` | `127.0.0.1` | TCP bind interface |
| `--overlay <KIND>` | None | Enable diagnostic overlay (`relayout`, `fps`, `hit`; repeatable) |
| `--panel` | Off | Launch IDE side-panel TCP bridge (`PANEL_ADDR=127.0.0.1:<port>`) |
| `--android` | Off | Run `adb reverse tcp:<port> tcp:<port>` and skip desktop spawn |
| `--no-spawn` | Off | Run TCP server without spawning `cargo build`/`cargo run` |

---

## 5. Environment Variables

| Variable | Effect |
|---|---|
| `PYTHON3` | Path to Python 3 executable required by `stylo` build scripts on Windows |
| `CARGO_TARGET_DIR` | Override Cargo artifact directory (e.g. `target-review`) |
| `KRUI_SERVE_ADDR` | TCP `host:port` consumed by preview-enabled apps to connect to `krui serve` |
| `KRUI_EMBED_PANEL` | Set to `1` by `krui serve --panel` to stream `"KRFR"` RGBA8 RLE frames headlessly |
| `KRUI_DEVICE_FRAME` | Initial device frame slug (`iphone-15`, `pixel-8`, `rpi-800x480`) |
| `KRUI_BENCH_WARMUP_FRAMES` / `KRUI_BENCH_MEASURED_FRAMES` | Override `krui-bench` warmup (`30`) and measured (`300`) frame counts |
| `KRUI_BENCH_BUDGET_US` | Override `krui-bench` 120 FPS budget (`8333` us) |
| `KRUI_BENCH_ALLOW_MISSING_WGPU` | Set to `1` in GPU-less CI to skip `Wgpu` benchmark instead of failing |
