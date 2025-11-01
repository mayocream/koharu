fn main() {
    #[cfg(feature = "cuda")]
    setup_cuda();
}

#[allow(dead_code)]
fn setup_cuda() {
    let workspace_dir = std::env::var("CARGO_WORKSPACE_DIR").unwrap();
    let script_path = std::path::Path::new(&workspace_dir).join("scripts/cuda.py");

    // Determine target directory (debug or release)
    let profile = std::env::var("PROFILE").unwrap();
    let target_dir = std::path::Path::new(&workspace_dir)
        .join("target")
        .join(&profile);

    println!("cargo:rerun-if-changed={}", script_path.display());

    if !std::process::Command::new("python")
        .arg(script_path)
        .arg("-o")
        .arg(&target_dir)
        .status()
        .expect("Failed to run CUDA setup script")
        .success()
    {
        panic!("CUDA setup script failed");
    }

    println!("cargo:rustc-link-search=native={}", target_dir.display());
}
