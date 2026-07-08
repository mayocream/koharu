use anyhow::Result;
use koharu_runtime::package::{libtorch::Libtorch, Package};
use std::{env, fs, path::PathBuf};

#[tokio::main]
async fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=libtch/CMakeLists.txt");
    println!("cargo:rerun-if-changed=libtch/torch_api.cpp");
    println!("cargo:rerun-if-changed=libtch/torch_api.h");
    println!("cargo:rerun-if-changed=libtch/torch_api_generated.cpp");
    println!("cargo:rerun-if-changed=libtch/torch_api_generated.h");

    let target_dir = output_dir()?;
    fs::create_dir_all(&target_dir)?;

    let target_shim = target_dir.join(library_name());
    if target_shim.exists() {
        return Ok(());
    }

    let libtorch = if cfg!(target_os = "macos") {
        Libtorch::Cpu
    } else {
        Libtorch::Cuda126
    };
    let libtorch_dir = libtorch.resolve().await?.join("libtorch");
    let cmake_dir = cmake::Config::new("libtch")
        .define("CMAKE_PREFIX_PATH", libtorch_dir)
        .build();

    fs::copy(cmake_dir.join(library_name()), target_shim)?;

    Ok(())
}

fn output_dir() -> Result<PathBuf> {
    let workspace_dir = PathBuf::from(env::var("CARGO_WORKSPACE_DIR")?);
    let profile = env::var("PROFILE")?;
    Ok(workspace_dir.join("target").join(profile))
}

fn library_name() -> &'static str {
    if cfg!(windows) {
        "koharu_torch_shim.dll"
    } else if cfg!(target_os = "macos") {
        "libkoharu_torch_shim.dylib"
    } else {
        "libkoharu_torch_shim.so"
    }
}
