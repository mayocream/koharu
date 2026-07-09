use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use koharu_bindgen::Generator;
use koharu_runtime::package::{libtorch::Libtorch, Package};

const SHIM_LIBRARY_NAME: &str = "koharu_torch_shim";
const OPAQUE_TYPES: &str = "^(tensor|scalar|optimizer|module|ivalue)$";
const TORCH_API_HEADER: &str = "libtch/torch_api.h";
const TORCH_API_GENERATED_HEADER: &str = "libtch/torch_api_generated.h";
const RERUN_IF_CHANGED: &[&str] = &[
    "build.rs",
    "libtch/CMakeLists.txt",
    "libtch/torch_api.cpp",
    TORCH_API_HEADER,
    "libtch/torch_api_generated.cpp",
    TORCH_API_GENERATED_HEADER,
];

#[tokio::main]
async fn main() -> Result<()> {
    for path in RERUN_IF_CHANGED {
        println!("cargo:rerun-if-changed={path}");
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    generate_bindings(&out_dir)?;
    build_shim().await
}

fn generate_bindings(out_dir: &Path) -> Result<()> {
    generator(TORCH_API_HEADER)
        .with_bindgen(|builder| {
            builder
                .allowlist_function("^(at.*|ato.*|ats.*|atc.*|atm.*|ati.*|get_and_reset_last_err)$")
                .blocklist_function("^at_autocast_(is_enabled|set_enabled)$")
        })
        .write_to_file(out_dir.join("torch_api.rs"))?;

    let generated_header = out_dir.join("torch_api_generated_bindgen.h");
    let generated_source = fs::read_to_string(TORCH_API_GENERATED_HEADER)
        .with_context(|| format!("failed to read {TORCH_API_GENERATED_HEADER}"))?;
    fs::write(
        &generated_header,
        bindgen_generated_header_compat(generated_source),
    )
    .with_context(|| format!("failed to write {}", generated_header.display()))?;

    generator(&generated_header)
        .with_bindgen(|builder| builder.clang_arg("-Ilibtch").allowlist_function("^atg_.*"))
        .write_to_file(out_dir.join("torch_api_generated.rs"))?;

    Ok(())
}

fn generator(header: impl AsRef<Path>) -> Generator {
    Generator::from_header(header, SHIM_LIBRARY_NAME)
        .with_bindgen(|builder| builder.layout_tests(false).blocklist_type(OPAQUE_TYPES))
}

fn bindgen_generated_header_compat(mut source: String) -> String {
    // The generated Rust wrappers pass byte strings as u8 slices and tensor
    // pointer arrays by shared reference. Keep that compatibility isolated to
    // the bindgen view instead of changing the compiled C++ signatures.
    source = source.replace("char **", "__KOHARU_CHAR_PTR_PTR__");
    source = source.replace("char*", "__KOHARU_CHAR_PTR__");
    source = source.replace("char *", "__KOHARU_CHAR_PTR__");
    source = source.replace("int64_t *", "const int64_t *");
    source = source.replace("double *", "const double *");
    source = source.replace("tensor *", "const tensor *");
    source = source.replace("__KOHARU_CHAR_PTR_PTR__", "const uint8_t *const *");
    source.replace("__KOHARU_CHAR_PTR__", "const uint8_t *")
}

async fn build_shim() -> Result<()> {
    let target_dir = output_dir()?;
    fs::create_dir_all(&target_dir)?;

    let target_shim = target_dir.join(shim_file_name());
    if target_shim.exists() {
        return Ok(());
    }

    let libtorch = if cfg!(target_os = "macos") {
        Libtorch::Cpu
    } else {
        Libtorch::Cuda130
    };
    let libtorch_dir = libtorch.resolve().await?.join("libtorch");
    let cmake_dir = cmake::Config::new("libtch")
        .define("CMAKE_PREFIX_PATH", libtorch_dir)
        .build();

    fs::copy(cmake_dir.join(shim_file_name()), target_shim)?;

    Ok(())
}

fn output_dir() -> Result<PathBuf> {
    Ok(PathBuf::from(env::var("CARGO_WORKSPACE_DIR")?)
        .join("target")
        .join(env::var("PROFILE")?))
}

fn shim_file_name() -> &'static str {
    if cfg!(windows) {
        "koharu_torch_shim.dll"
    } else if cfg!(target_os = "macos") {
        "libkoharu_torch_shim.dylib"
    } else {
        "libkoharu_torch_shim.so"
    }
}
