//! CLI arguments and target path resolution for `krui serve`.

use krui_preview::{DeviceFramePreset, PreviewOverlayKind, DEFAULT_PREVIEW_PORT};
use std::path::{Path, PathBuf};

/// Error or help-exit produced by [`parse_cli_args`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliParseError {
    /// `--help` or `-h` was requested; should be printed to stdout and exit with code `0`.
    HelpRequested(String),
    /// Invalid subcommand, flag, or value; should be printed to stderr and exit with non-zero code.
    InvalidArgs(String),
}

impl std::fmt::Display for CliParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HelpRequested(msg) | Self::InvalidArgs(msg) => f.write_str(msg),
        }
    }
}

/// Parsed command-line options for `krui serve`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServeOptions {
    /// Optional `--example <NAME>` to spawn (`None` runs the default `krui-showcase` binary).
    pub example: Option<String>,
    /// Optional `--app <PKG>` / `--bin <PKG>` package override (`None` infers from `watch_path`).
    pub app: Option<String>,
    /// Optional `--frame iphone-15|pixel-8|rpi-800x480` device frame preset.
    pub frame: Option<DeviceFramePreset>,
    /// Optional initial `.krui` file path to watch (`None` auto-discovers from the target example/app).
    pub watch_path: Option<PathBuf>,
    /// Local TCP port to bind (`0` for OS-assigned ephemeral port, default `41731`).
    pub port: u16,
    /// Optional initial debug overlays (`--overlay relayout|fps|hit`).
    pub overlays: Vec<PreviewOverlayKind>,
    /// Do not spawn a child `cargo run` process (useful for headless tests or external runners).
    pub no_spawn: bool,
    /// Host interface address to bind the TCP live-preview server on (default `"127.0.0.1"`).
    pub host: String,
    /// Configures `adb reverse tcp:<port> tcp:<port>` for Android devices and skips desktop spawn.
    pub android: bool,
    /// Launches embedded IDE side-panel TCP frame bridge (`PANEL_ADDR=...`).
    pub panel: bool,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            example: None,
            app: None,
            frame: None,
            watch_path: None,
            port: DEFAULT_PREVIEW_PORT,
            overlays: Vec::new(),
            no_spawn: false,
            host: "127.0.0.1".to_string(),
            android: false,
            panel: false,
        }
    }
}

/// Parses `krui serve ...` command-line arguments into [`ServeOptions`].
pub fn parse_cli_args<I, S>(args: I) -> Result<ServeOptions, CliParseError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let raw: Vec<String> = args.into_iter().map(Into::into).collect();
    let mut idx = 0;
    if raw.get(idx).map(String::as_str) == Some("serve") {
        idx += 1;
    } else if let Some(cmd) = raw.get(idx) {
        if cmd == "--help" || cmd == "-h" {
            return Err(CliParseError::HelpRequested(usage_text()));
        }
        if !cmd.starts_with("--") {
            return Err(CliParseError::InvalidArgs(format!(
                "Unknown subcommand `{cmd}`.\n\n{}",
                usage_text()
            )));
        }
    }

    let mut opts = ServeOptions::default();
    while idx < raw.len() {
        apply_cli_flag(&raw, &mut idx, &mut opts)?;
        idx += 1;
    }
    Ok(opts)
}

fn apply_cli_flag(
    raw: &[String],
    idx: &mut usize,
    opts: &mut ServeOptions,
) -> Result<(), CliParseError> {
    let arg = &raw[*idx];
    match arg.as_str() {
        "--help" | "-h" => return Err(CliParseError::HelpRequested(usage_text())),
        "--example" | "-e" => {
            opts.example = Some(next_arg_value(
                raw,
                idx,
                "Missing example name after `--example`",
            )?);
        }
        "--frame" | "-f" => {
            let frame_str = next_arg_value(
                raw,
                idx,
                "Missing preset after `--frame` (expected iphone-15|pixel-8|rpi-800x480)",
            )?;
            opts.frame = Some(parse_frame_preset(&frame_str)?);
        }
        "--watch" | "-w" => {
            let p = next_arg_value(raw, idx, "Missing path after `--watch`")?;
            opts.watch_path = Some(PathBuf::from(p));
        }
        "--port" | "-p" => {
            let p_str = next_arg_value(raw, idx, "Missing port number after `--port`")?;
            opts.port = parse_u16_port(&p_str)?;
        }
        "--host" => {
            let host = next_arg_value(raw, idx, "Missing address after `--host`")?;
            opts.host = parse_non_empty_host(&host)?;
        }
        "--android" => opts.android = true,
        "--app" | "--bin" => {
            let pkg = next_arg_value(raw, idx, "Missing app package name after `--app`")?;
            opts.app = Some(pkg.clone());
            if opts.watch_path.is_none() {
                opts.watch_path = Some(default_krui_path_for_app_package(&pkg));
            }
        }
        "--overlay" => apply_overlay_flag(raw, idx, opts)?,
        "--no-spawn" => opts.no_spawn = true,
        "--panel" => opts.panel = true,
        other => apply_inline_cli_flag(other, opts)?,
    }
    Ok(())
}

fn apply_overlay_flag(
    raw: &[String],
    idx: &mut usize,
    opts: &mut ServeOptions,
) -> Result<(), CliParseError> {
    let ov_str = next_arg_value(
        raw,
        idx,
        "Missing overlay kind after `--overlay` (expected relayout|fps|hit)",
    )?;
    let kind = PreviewOverlayKind::parse(&ov_str).ok_or_else(|| {
        CliParseError::InvalidArgs(format!(
            "Unknown `--overlay {ov_str}` (expected `relayout`, `fps`, or `hit`)"
        ))
    })?;
    opts.overlays.push(kind);
    Ok(())
}

fn parse_u16_port(raw: &str) -> Result<u16, CliParseError> {
    raw.parse::<u16>()
        .map_err(|_| CliParseError::InvalidArgs(format!("Invalid port `{raw}`")))
}

fn parse_non_empty_host(raw: &str) -> Result<String, CliParseError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CliParseError::InvalidArgs(
            "Empty host address after `--host`".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

fn next_arg_value(raw: &[String], idx: &mut usize, err_msg: &str) -> Result<String, CliParseError> {
    *idx += 1;
    raw.get(*idx)
        .cloned()
        .ok_or_else(|| CliParseError::InvalidArgs(err_msg.to_string()))
}

fn apply_inline_cli_flag(other: &str, opts: &mut ServeOptions) -> Result<(), CliParseError> {
    if let Some(val) = other.strip_prefix("--example=") {
        opts.example = Some(val.to_string());
    } else if let Some(val) = other
        .strip_prefix("--app=")
        .or_else(|| other.strip_prefix("--bin="))
    {
        let pkg = parse_non_empty_host(val)?;
        opts.app = Some(pkg.clone());
        if opts.watch_path.is_none() {
            opts.watch_path = Some(default_krui_path_for_app_package(&pkg));
        }
    } else if let Some(val) = other.strip_prefix("--frame=") {
        opts.frame = Some(parse_frame_preset(val)?);
    } else if let Some(val) = other.strip_prefix("--host=") {
        opts.host = parse_non_empty_host(val)?;
    } else {
        return Err(CliParseError::InvalidArgs(format!(
            "Unrecognized argument `{other}`.\n\n{}",
            usage_text()
        )));
    }
    Ok(())
}

fn parse_frame_preset(val: &str) -> Result<DeviceFramePreset, CliParseError> {
    DeviceFramePreset::parse(val).ok_or_else(|| {
        CliParseError::InvalidArgs(format!(
            "Unknown `--frame {val}` (expected `iphone-15`, `pixel-8`, or `rpi-800x480`)"
        ))
    })
}

/// Returns the CLI help text for `krui serve`.
pub fn usage_text() -> String {
    [
        "KRui Live Preview Server (`krui serve`)",
        "",
        "USAGE:",
        "    krui serve [--example <NAME> | --app <NAME>] [--frame iphone-15|pixel-8|rpi-800x480] [--overlay relayout|fps|hit]",
        "",
        "OPTIONS:",
        "    -e, --example <NAME>                 Run `examples/<NAME>` with `--features preview`",
        "        --app <NAME>                     Run workspace app `apps/<NAME>` with `--features preview`",
        "    -f, --frame <PRESET>                 Wrap viewport in device frame (`iphone-15`, `pixel-8`, `rpi-800x480`)",
        "    -w, --watch <PATH>                   Explicit `.krui` file path to watch",
        "    -p, --port <PORT>                    Local TCP port (default: 41731)",
        "        --overlay <relayout|fps|hit>     Enable a live diagnostic overlay",
        "        --no-spawn                       Start TCP server without spawning `cargo run`",
        "        --panel                          Run embedded IDE side-panel TCP frame bridge",
        "        --host <ADDR>                    Bind TCP preview server interface (default: 127.0.0.1)",
        "        --android                        Run `adb reverse tcp:<port>` and skip desktop spawn",
    ]
    .join("\n")
}

/// Resolves the default `.krui` file to watch for an example name or the component gallery showcase.
pub fn default_krui_path_for_target(example: Option<&str>) -> PathBuf {
    match example {
        None | Some("component_gallery") => {
            PathBuf::from("examples/component_gallery/views/component_gallery.krui")
        }
        Some(name) => first_krui_in_dir(
            &PathBuf::from(format!("examples/{name}/views")),
            &format!("examples/{name}/views/app.krui"),
        ),
    }
}

pub(crate) fn default_krui_path_for_app_package(pkg: &str) -> PathBuf {
    first_krui_in_dir(
        &PathBuf::from(format!("apps/{pkg}/views")),
        &format!("apps/{pkg}/views/app.krui"),
    )
}

fn first_krui_in_dir(views_dir: &Path, fallback: &str) -> PathBuf {
    if let Ok(entries) = std::fs::read_dir(views_dir) {
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("krui"))
            .collect();
        found.sort();
        if let Some(first) = found.into_iter().next() {
            return first;
        }
    }
    PathBuf::from(fallback)
}

fn fallback_krui_for_path(example: Option<&str>, raw: &Path) -> PathBuf {
    if example.is_none() {
        if let Some(pkg) = infer_app_package_from_path(raw) {
            return default_krui_path_for_app_package(&pkg);
        }
    }
    default_krui_path_for_target(example)
}

/// Infers `(example_name, root_krui_path)` from [`ServeOptions`], resolving `--watch` paths
/// inside `examples/<name>/...`, `apps/<pkg>/...`, `components/*.krui`, companion `.css`, and `showcase_nav.krui`.
pub fn resolve_effective_target(opts: &ServeOptions) -> (Option<String>, PathBuf) {
    let example = opts
        .example
        .clone()
        .or_else(|| opts.watch_path.as_deref().and_then(infer_example_from_path));
    let Some(raw) = &opts.watch_path else {
        return (
            example.clone(),
            default_krui_path_for_target(example.as_deref()),
        );
    };
    let normalized = raw.to_string_lossy().replace('\\', "/");
    let is_component = raw
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        == Some("components");
    let is_showcase_nav = normalized.ends_with("src/views/showcase_nav.krui");
    if is_component || is_showcase_nav {
        return (
            example.clone(),
            fallback_krui_for_path(example.as_deref(), raw),
        );
    }
    if raw.extension().and_then(|e| e.to_str()) == Some("css") {
        let sibling = raw.with_extension("krui");
        if sibling.is_file() {
            return (example, sibling);
        }
        return (
            example.clone(),
            fallback_krui_for_path(example.as_deref(), raw),
        );
    }
    (example, raw.clone())
}

fn infer_segment_after_folder(path: &Path, folder: &str) -> Option<String> {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let mut parts = normalized.split('/').filter(|s| !s.is_empty());
    while let Some(seg) = parts.next() {
        if seg == folder {
            if let Some(name) = parts.next() {
                if !name.is_empty() && !name.contains('.') {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

fn infer_example_from_path(path: &Path) -> Option<String> {
    infer_segment_after_folder(path, "examples")
}

pub(crate) fn infer_app_package_from_path(path: &Path) -> Option<String> {
    infer_segment_after_folder(path, "apps")
}

pub(crate) fn infer_app_package_from_opts(opts: &ServeOptions) -> Option<String> {
    if opts.example.is_some() {
        return None;
    }
    if let Some(pkg) = &opts.app {
        return Some(pkg.clone());
    }
    opts.watch_path
        .as_deref()
        .and_then(infer_app_package_from_path)
}
