use std::env;
use std::process::Command;

fn main() {
    emit_build_version();
    tauri_build::build();
}

fn emit_build_version() {
    // Re-run the build script when the git state changes or when the override
    // environment variable is updated.
    println!("cargo:rerun-if-env-changed=KOHARU_BUILD_VERSION");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs");

    // Allow manual overrides via env var, otherwise try git, and finally fall
    // back to the Cargo package version.
    let version = env::var("KOHARU_BUILD_VERSION")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(git_describe)
        .unwrap_or_else(|| env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".into()));

    println!("cargo:rustc-env=KOHARU_BUILD_VERSION={version}");
}

fn git_describe() -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--dirty", "--always"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
