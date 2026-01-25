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
    println!("cargo:rerun-if-changed=../ui/out");
    println!("cargo:rerun-if-changed=../ui/out/index.html");
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
