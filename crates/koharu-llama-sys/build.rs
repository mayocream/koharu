use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use koharu_bindgen::Generator;

const HEADER: &str = "wrapper.h";
const PUBLIC_HEADERS: &[&str] = &[
    "include/llama.h",
    "include/gguf.h",
    "include/ggml.h",
    "include/ggml-alloc.h",
    "include/ggml-backend.h",
    "include/ggml-cpu.h",
    "include/ggml-opt.h",
    "include/mtmd.h",
    "include/mtmd-helper.h",
];
const BINDGEN_EXTRA_HEADERS: &[&str] = &[HEADER, "wrapper_common.h", "wrapper_utils.h"];
const SHIM_SOURCES: &[&str] = &[
    "shim/CMakeLists.txt",
    "shim/common.cpp",
    "common/common.h",
    "common/common_support.cpp",
    "common/json-schema-to-grammar.h",
    "common/json-schema-to-grammar.cpp",
];

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    for path in BINDGEN_EXTRA_HEADERS
        .iter()
        .chain(PUBLIC_HEADERS)
        .chain(SHIM_SOURCES)
    {
        println!("cargo:rerun-if-changed={path}");
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    generate_bindings(&manifest_dir, &out_dir)?;
    build_shim(&manifest_dir)
}

fn generate_bindings(manifest_dir: &Path, out_dir: &Path) -> Result<()> {
    let include_dir = manifest_dir.join("include");
    Generator::from_header(manifest_dir.join(HEADER), "koharu_llama_shim")
        .with_libraries(library_candidates())
        .with_bindgen(|builder| {
            builder
                .clang_arg(format!("-I{}", manifest_dir.display()))
                .clang_arg(format!("-I{}", include_dir.display()))
                .layout_tests(false)
                .derive_partialeq(true)
                .allowlist_function("^(ggml|gguf|llama|llama_rs|mtmd)_.*")
                .allowlist_type("^(ggml|gguf|llama|llama_rs|mtmd)_.*")
                .allowlist_var("^(GGML|GGUF|LLAMA|LLAMA_RS|MTMD)_.*")
                .prepend_enum_name(false)
        })
        .write_to_file(out_dir.join("bindings.rs"))
}

fn build_shim(manifest_dir: &Path) -> Result<()> {
    let target_dir = output_dir()?;
    fs::create_dir_all(&target_dir)?;

    let cmake_dir = cmake::Config::new("shim")
        .define(
            "KOHARU_LLAMA_COMMON_SHIM_SOURCE",
            manifest_dir.join("shim/common.cpp"),
        )
        .define(
            "KOHARU_LLAMA_JSON_SCHEMA_SOURCE",
            manifest_dir.join("common/json-schema-to-grammar.cpp"),
        )
        .define(
            "KOHARU_LLAMA_COMMON_SUPPORT_SOURCE",
            manifest_dir.join("common/common_support.cpp"),
        )
        .define("KOHARU_LLAMA_ROOT_DIR", manifest_dir)
        .define("KOHARU_LLAMA_INCLUDE_DIR", manifest_dir.join("include"))
        .define("KOHARU_LLAMA_COMMON_DIR", manifest_dir.join("common"))
        .define("KOHARU_LLAMA_VENDOR_DIR", manifest_dir.join("vendor"))
        .build();

    let shim_name = shim_file_name();
    let built_shim = cmake_dir.join(shim_name);
    ensure!(
        built_shim.exists(),
        "failed to locate built {} at {}",
        shim_name,
        built_shim.display()
    );

    let target_shim = target_dir.join(shim_name);
    fs::copy(&built_shim, &target_shim).with_context(|| {
        format!(
            "failed to copy {} to {}",
            built_shim.display(),
            target_shim.display()
        )
    })?;

    Ok(())
}

fn output_dir() -> Result<PathBuf> {
    Ok(PathBuf::from(env::var("CARGO_WORKSPACE_DIR")?)
        .join("target")
        .join(env::var("PROFILE")?))
}

fn shim_file_name() -> &'static str {
    if cfg!(windows) {
        "koharu_llama_shim.dll"
    } else if cfg!(target_os = "macos") {
        "libkoharu_llama_shim.dylib"
    } else {
        "libkoharu_llama_shim.so"
    }
}

fn library_candidates() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &[
            "koharu_llama_shim",
            "llama",
            "ggml",
            "ggml-base",
            "ggml-cpu",
            "ggml-cpu-x64",
            "ggml-cpu-sse42",
            "ggml-cpu-sandybridge",
            "ggml-cpu-ivybridge",
            "ggml-cpu-haswell",
            "ggml-cpu-piledriver",
            "ggml-cpu-alderlake",
            "ggml-cpu-cannonlake",
            "ggml-cpu-cascadelake",
            "ggml-cpu-cooperlake",
            "ggml-cpu-icelake",
            "ggml-cpu-skylakex",
            "ggml-cpu-sapphirerapids",
            "ggml-cpu-zen4",
            "ggml-cuda",
            "ggml-vulkan",
            "ggml-rpc",
            "llama-common",
            "mtmd",
        ]
    } else if cfg!(target_os = "macos") {
        &[
            "koharu_llama_shim",
            "llama",
            "ggml",
            "ggml-base",
            "ggml-cpu",
            "ggml-metal",
            "ggml-blas",
            "ggml-rpc",
            "llama-common",
            "mtmd",
        ]
    } else {
        &[
            "koharu_llama_shim",
            "llama",
            "ggml",
            "ggml-base",
            "ggml-cpu",
            "ggml-cuda",
            "ggml-vulkan",
            "ggml-rpc",
            "llama-common",
            "mtmd",
        ]
    }
}
