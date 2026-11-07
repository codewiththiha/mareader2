//! Rust shape gate backed by the small, dependency-free source lexer.

use std::process::Command;

/// Reject Rust size/function/mod.rs debt not present in the reviewed baseline.
pub fn run(args: &[String]) -> Result<(), String> {
    let status = Command::new("python3")
        .arg("tools/shape_check.py")
        .args(args)
        .current_dir(crate::root())
        .status()
        .map_err(|error| format!("shape check: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("shape check failed".to_owned())
    }
}
