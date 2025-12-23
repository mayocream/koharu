use std::env;
use std::process::Command;

fn main() {
    emit_build_version();
    build_ui_if_needed();
    tauri_build::build();
}

fn emit_build_version() {
    // Re-run the build script when the git state changes or when the override
    // environment variable is updated.
    println!("cargo:rerun-if-env-changed=KOHARU_BUILD_VERSION");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs");
    println!("cargo:rerun-if-changed=../ui/out");
    println!("cargo:rerun-if-changed=../ui/out/index.html");

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

fn build_ui_if_needed() {
    // Only attempt to build the UI for release builds, unless explicitly skipped.
    let profile = env::var("PROFILE").unwrap_or_default();
    if profile != "release" {
        return;
    }
    if env::var("KOHARU_SKIP_UI_BUILD").is_ok() {
        println!("cargo:warning=Skipping UI build because KOHARU_SKIP_UI_BUILD is set");
        return;
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let ui_out = std::path::Path::new(&manifest_dir).join("../ui/out/index.html");

    // Trigger rerun when the built UI changes; this is a coarse signal.
    println!("cargo:rerun-if-changed={}", ui_out.display());

    if ui_out.exists() {
        return;
    }

    let status = Command::new("bun")
        .args(["--cwd", "../ui", "build"])
        .status()
        .expect("failed to spawn bun build for UI");

    if !status.success() {
        panic!("bun build for UI failed with status {status:?}");
    }
}
