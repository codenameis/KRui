//! Android `adb reverse` port-forwarding helpers for `krui serve --android`.

use std::process::Command;

/// Configures `adb reverse tcp:<port> tcp:<port>` for a connected Android device/emulator.
pub fn try_setup_adb_reverse(port: u16) -> Result<(), String> {
    try_setup_adb_reverse_with_bin("adb", port)
}

/// Runs `<adb_bin> reverse tcp:<port> tcp:<port>` and returns a descriptive error if it fails.
pub fn try_setup_adb_reverse_with_bin(adb_bin: &str, port: u16) -> Result<(), String> {
    let spec = format!("tcp:{port}");
    let out = Command::new(adb_bin)
        .args(["reverse", &spec, &spec])
        .output()
        .map_err(|e| format!("failed to run `{adb_bin} reverse {spec} {spec}`: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    Err(format!(
        "`{adb_bin} reverse {spec} {spec}` exited with {}: {}",
        out.status, detail
    ))
}
