use std::{env, path::PathBuf};

use anyhow::Result;
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
const DYNAMIC_LIBRARIES: &[&str] = &[
    "llama",
    "ggml",
    "ggml-base",
    "ggml-cpu",
    "ggml-cpu-x64",
    "mtmd",
];
const FUNCTION_ALLOWLIST: &str = "^(ggml|gguf|llama|mtmd)_.*";
const TYPE_ALLOWLIST: &str = "^(ggml|gguf|llama|mtmd)_.*";
const VARIABLE_ALLOWLIST: &str = "^(GGML|GGUF|LLAMA|MTMD)_.*";

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={HEADER}");
    for path in PUBLIC_HEADERS {
        println!("cargo:rerun-if-changed={path}");
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let include_dir = manifest_dir.join("include");
    Generator::from_header(manifest_dir.join(HEADER), "koharu-llama")
        .with_libraries(DYNAMIC_LIBRARIES)
        .with_bindgen(|builder| {
            builder
                .clang_arg(format!("-I{}", manifest_dir.display()))
                .clang_arg(format!("-I{}", include_dir.display()))
                .layout_tests(false)
                .derive_partialeq(true)
                .allowlist_function(FUNCTION_ALLOWLIST)
                .allowlist_type(TYPE_ALLOWLIST)
                .allowlist_var(VARIABLE_ALLOWLIST)
                .prepend_enum_name(false)
        })
        .write_to_file(PathBuf::from(env::var("OUT_DIR")?).join("bindings.rs"))
}
