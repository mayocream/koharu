use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::Result;
use koharu_bindgen::Generator;

const HEADER: &str = "wrapper.h";
const PUBLIC_HEADERS: &[&str] = &["include/stable-diffusion.h"];

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={HEADER}");
    for path in PUBLIC_HEADERS {
        println!("cargo:rerun-if-changed={path}");
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    generate_bindings(&manifest_dir, &out_dir)?;
    Ok(())
}

fn generate_bindings(manifest_dir: &Path, out_dir: &Path) -> Result<()> {
    let include_dir = manifest_dir.join("include");
    Generator::from_header(manifest_dir.join(HEADER), "stable-diffusion")
        .with_libraries(library_candidates())
        .with_bindgen(|builder| {
            builder
                .clang_arg(format!("-I{}", include_dir.display()))
                .layout_tests(false)
                .derive_partialeq(true)
                .allowlist_function(
                    "^(sd_.*|str_to_.*|new_sd_ctx|free_sd_ctx|free_sd_audio|free_sd_images|generate_image|generate_video|new_upscaler_ctx|free_upscaler_ctx|upscale|get_upscale_factor|convert|convert_with_components|preprocess_canny|load_imatrix|save_imatrix|enable_imatrix_collection|disable_imatrix_collection)$",
                )
                .allowlist_type(
                    "^(sd_.*|rng_type_t|sample_method_t|scheduler_t|prediction_t|preview_t|lora_apply_mode_t|upscaler_ctx_t|ggml_tensor)$",
                )
                .allowlist_var(
                    "^(STD_DEFAULT_RNG|CUDA_RNG|CPU_RNG|RNG_TYPE_COUNT|.*_SAMPLE_METHOD|SAMPLE_METHOD_COUNT|.*_SCHEDULER|SCHEDULER_COUNT|.*_PRED|PREDICTION_COUNT|SD_.*|PREVIEW_.*|LORA_.*)$",
                )
                .prepend_enum_name(false)
        })
        .write_to_file(out_dir.join("bindings.rs"))
}

fn library_candidates() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &[
            "stable-diffusion",
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
            "ggml-hip",
            "ggml-rocm",
            "ggml-rpc",
        ]
    } else if cfg!(target_os = "macos") {
        &[
            "stable-diffusion",
            "ggml",
            "ggml-base",
            "ggml-cpu",
            "ggml-metal",
            "ggml-blas",
            "ggml-rpc",
        ]
    } else {
        &[
            "stable-diffusion",
            "ggml",
            "ggml-base",
            "ggml-cpu",
            "ggml-cuda",
            "ggml-vulkan",
            "ggml-hip",
            "ggml-rocm",
            "ggml-rpc",
        ]
    }
}
