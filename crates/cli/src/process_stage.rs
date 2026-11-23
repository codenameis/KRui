//! Process building and generation-staged binary launching for lock-free Windows restarts.

use crate::{ServeOptions, ServeSession};
use krui_compiler::CompileError;
use krui_preview::{
    DeviceFramePreset, PreviewCommand, KRUI_DEVICE_FRAME_ENV, KRUI_EMBED_PANEL_ENV,
    KRUI_SERVE_ADDR_ENV,
};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_STAGE_GENERATION: AtomicU64 = AtomicU64::new(1);

pub(crate) struct ChildKillGuard(pub(crate) Option<Child>);

impl Drop for ChildKillGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub(crate) fn poll_exited_preview_child(child_slot: &mut Option<Child>) -> Option<String> {
    let child = child_slot.as_mut()?;
    match child.try_wait() {
        Ok(Some(status)) => {
            *child_slot = None;
            Some(format!("Preview process exited unexpectedly with {status}"))
        }
        Ok(None) => None,
        Err(err) => {
            *child_slot = None;
            Some(format!("Preview process wait failed: {err}"))
        }
    }
}

/// Builds the preview target (`cargo build --features preview ...`) and returns `Ok(())` on success
/// or `Err(String)` if compilation fails (so the existing child process can stay alive).
pub fn build_preview_app_target(opts: &ServeOptions) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.arg("build").arg("--features").arg("preview");
    if let Some(ex) = &opts.example {
        cmd.arg("--example").arg(ex);
    } else if let Some(pkg) = crate::args::infer_app_package_from_opts(opts) {
        cmd.arg("-p").arg(pkg);
    } else {
        cmd.arg("--bin").arg("krui-showcase");
    }
    let status = cmd
        .status()
        .map_err(|e| format!("Failed to invoke `cargo build`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "`cargo build --features preview` failed with {status}"
        ))
    }
}

/// Resolves the Cargo build `target` directory (`CARGO_TARGET_DIR` if set, otherwise `./target`).
pub fn resolve_cargo_target_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
        let p = PathBuf::from(dir);
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    PathBuf::from("target")
}

/// Returns the expected `<target_dir>/debug/[examples/]<name>[.exe]` path produced by `cargo build`.
pub fn built_preview_binary_path(target_dir: &Path, example: Option<&str>) -> PathBuf {
    let name = example.unwrap_or("krui-showcase");
    let exe_name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    if example.is_some() {
        target_dir.join("debug").join("examples").join(exe_name)
    } else {
        target_dir.join("debug").join(exe_name)
    }
}

/// Returns the expected built preview binary path for [`ServeOptions`] (`examples/<ex>`, `apps/<pkg>`, or `krui-showcase`).
pub fn preview_binary_path_for_opts(target_dir: &Path, opts: &ServeOptions) -> PathBuf {
    if opts.example.is_some() {
        return built_preview_binary_path(target_dir, opts.example.as_deref());
    }
    if let Some(pkg) = crate::args::infer_app_package_from_opts(opts) {
        let exe_name = format!("{pkg}{}", std::env::consts::EXE_SUFFIX);
        return target_dir.join("debug").join(exe_name);
    }
    built_preview_binary_path(target_dir, None)
}

/// Copies the built preview executable into `<target_dir>/krui-serve-staging/<name>-<gen>[.exe]`
/// so `cargo build` never collides with a Windows file lock on the running preview process,
/// best-effort removing older staged binaries for `<name>`.
pub fn stage_built_preview_binary(
    target_dir: &Path,
    example: Option<&str>,
    generation: u64,
) -> std::io::Result<PathBuf> {
    let src_bin = built_preview_binary_path(target_dir, example);
    let name = example.unwrap_or("krui-showcase");
    stage_binary_from_src(target_dir, &src_bin, name, generation)
}

fn stage_binary_from_src(
    target_dir: &Path,
    src_bin: &Path,
    name: &str,
    generation: u64,
) -> std::io::Result<PathBuf> {
    let staging_dir = target_dir.join("krui-serve-staging");
    std::fs::create_dir_all(&staging_dir)?;
    let staged_name = format!("{name}-{generation}{}", std::env::consts::EXE_SUFFIX);
    let dst_bin = staging_dir.join(staged_name);
    cleanup_old_staged_binaries(&staging_dir, name, &dst_bin);
    std::fs::copy(src_bin, &dst_bin)?;
    Ok(dst_bin)
}

fn cleanup_old_staged_binaries(staging_dir: &Path, name: &str, keep_path: &Path) {
    let prefix = format!("{name}-");
    let Ok(entries) = std::fs::read_dir(staging_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p == keep_path {
            continue;
        }
        if p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|s| s.starts_with(&prefix))
        {
            let _ = std::fs::remove_file(p);
        }
    }
}

fn spawn_staged_preview_binary(
    staged_bin: &Path,
    opts: &ServeOptions,
    serve_addr: std::net::SocketAddr,
) -> std::io::Result<Child> {
    let mut cmd = Command::new(staged_bin);
    cmd.env(KRUI_SERVE_ADDR_ENV, serve_addr.to_string());
    if opts.panel {
        cmd.env(KRUI_EMBED_PANEL_ENV, "1");
    }
    if let Some(frame) = opts.frame {
        let code = match frame {
            DeviceFramePreset::Iphone15 => "iphone-15",
            DeviceFramePreset::Pixel8 => "pixel-8",
            DeviceFramePreset::Rpi800x480 => "rpi-800x480",
        };
        cmd.env(KRUI_DEVICE_FRAME_ENV, code);
    }
    cmd.spawn()
}

/// Stages the already-built preview executable under a fresh generation number and spawns it.
pub fn spawn_staged_after_build(
    opts: &ServeOptions,
    serve_addr: std::net::SocketAddr,
) -> std::io::Result<Child> {
    let gen_id = NEXT_STAGE_GENERATION.fetch_add(1, Ordering::Relaxed);
    let target_dir = resolve_cargo_target_dir();
    let staged = if opts.example.is_none() {
        if let Some(pkg) = crate::args::infer_app_package_from_opts(opts) {
            let src = preview_binary_path_for_opts(&target_dir, opts);
            stage_binary_from_src(&target_dir, &src, &pkg, gen_id)?
        } else {
            stage_built_preview_binary(&target_dir, None, gen_id)?
        }
    } else {
        stage_built_preview_binary(&target_dir, opts.example.as_deref(), gen_id)?
    };
    spawn_staged_preview_binary(&staged, opts, serve_addr)
}

/// Builds the preview target, copies it to a generation-numbered staged binary, and spawns it
/// connected to `serve_addr`.
pub fn spawn_preview_app_process(
    opts: &ServeOptions,
    serve_addr: std::net::SocketAddr,
) -> std::io::Result<Child> {
    build_preview_app_target(opts).map_err(std::io::Error::other)?;
    spawn_staged_after_build(opts, serve_addr)
}

/// Attempts to rebuild the preview binary while the previous staged `<name>-<gen-1>[.exe]` stays
/// unlocked and running, replacing `child` with `<name>-<gen>[.exe]` **only** after `cargo build` succeeds.
pub fn rebuild_and_restart_child(
    opts: &ServeOptions,
    session: &mut ServeSession,
    child: &mut Option<Child>,
) -> Result<(), String> {
    match build_preview_app_target(opts) {
        Ok(()) => {
            if let Some(mut old_child) = child.take() {
                let _ = old_child.kill();
                let _ = old_child.wait();
            }
            if !opts.no_spawn && !opts.android {
                let new_child = spawn_staged_after_build(opts, session.local_addr())
                    .map_err(|e| format!("Failed to respawn staged preview process: {e}"))?;
                *child = Some(new_child);
            }
            Ok(())
        }
        Err(build_err) => {
            let ce = CompileError::new(1, 1, build_err.clone());
            session.send_command(&PreviewCommand::CompileError(ce));
            Err(build_err)
        }
    }
}
