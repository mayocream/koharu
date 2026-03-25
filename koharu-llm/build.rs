use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use bindgen::callbacks::ParseCallbacks;
use flate2::read::GzDecoder;
use quote::{format_ident, quote};
use reqwest::blocking::Client;
use syn::{FnArg, ImplItem, Item, Pat, Signature};
use tar::Archive;

const GGML_FUNCTIONS: &[&str] = &[
    "ggml_backend_dev_count",
    "ggml_backend_dev_get",
    "ggml_backend_load_all_from_path",
];

const GGML_BASE_FUNCTIONS: &[&str] = &[
    "gguf_find_key",
    "gguf_free",
    "gguf_get_key",
    "gguf_get_kv_type",
    "gguf_get_n_kv",
    "gguf_get_n_tensors",
    "gguf_get_val_i32",
    "gguf_get_val_str",
    "gguf_get_val_u32",
    "gguf_get_val_u64",
    "gguf_init_from_file",
    "ggml_backend_cpu_buffer_type",
    "ggml_backend_dev_backend_reg",
    "ggml_backend_dev_get_props",
    "ggml_backend_reg_name",
    "ggml_log_set",
    "ggml_time_us",
];

const MTMD_FUNCTIONS: &[&str] = &["mtmd_.*", "mtmd_helper_.*"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LLAMA_CPP_TAG");

    if let Err(err) = run() {
        panic!("{err:#}");
    }
}

fn run() -> Result<()> {
    validate_target()?;

    let llama_cpp_tag = env::var("LLAMA_CPP_TAG").context("missing LLAMA_CPP_TAG")?;
    let out_dir = PathBuf::from(env::var("OUT_DIR").context("missing OUT_DIR")?);
    let source_root = ensure_source_tree(&out_dir, &llama_cpp_tag)?;
    let header_path = write_wrapper_header(&out_dir)?;
    let include_dirs = include_dirs(&source_root);

    generate_types(&out_dir, &header_path, &include_dirs)?;
    generate_loader(
        &out_dir,
        &header_path,
        &include_dirs,
        LoaderSpec {
            module_name: "llama",
            function_patterns: &["llama_.*"],
        },
    )?;
    generate_loader(
        &out_dir,
        &header_path,
        &include_dirs,
        LoaderSpec {
            module_name: "ggml",
            function_patterns: GGML_FUNCTIONS,
        },
    )?;
    generate_loader(
        &out_dir,
        &header_path,
        &include_dirs,
        LoaderSpec {
            module_name: "ggml_base",
            function_patterns: GGML_BASE_FUNCTIONS,
        },
    )?;
    generate_loader(
        &out_dir,
        &header_path,
        &include_dirs,
        LoaderSpec {
            module_name: "mtmd",
            function_patterns: MTMD_FUNCTIONS,
        },
    )?;
    generate_wrappers(
        &out_dir,
        &[
            ("llama", "llama_lib"),
            ("ggml", "ggml_lib"),
            ("ggml_base", "ggml_base_lib"),
            ("mtmd", "mtmd_lib"),
        ],
    )?;

    println!("cargo:rustc-env=KOHARU_LLM_LLAMA_CPP_TAG={llama_cpp_tag}");

    Ok(())
}

fn validate_target() -> Result<()> {
    let target = env::var("TARGET").context("missing TARGET")?;

    match target.as_str() {
        "x86_64-pc-windows-msvc" | "x86_64-unknown-linux-gnu" | "aarch64-apple-darwin" => {}
        _ => bail!(
            "unsupported target `{target}`; only Windows x86_64 MSVC, Linux x86_64, and macOS arm64 are supported"
        ),
    }

    Ok(())
}

fn ensure_source_tree(out_dir: &Path, llama_cpp_tag: &str) -> Result<PathBuf> {
    let cache_dir = out_dir.join("llama.cpp-source");
    let tarball_path = cache_dir.join(format!("llama.cpp-{llama_cpp_tag}.tar.gz"));
    let source_root = cache_dir.join(format!("llama.cpp-{llama_cpp_tag}"));
    let source_url =
        format!("https://github.com/ggml-org/llama.cpp/archive/refs/tags/{llama_cpp_tag}.tar.gz");

    if source_root.join("include/llama.h").exists() {
        return Ok(source_root);
    }

    fs::create_dir_all(&cache_dir).context("failed to create source cache dir")?;

    if !tarball_path.exists() {
        let client = Client::builder()
            .user_agent("koharu-llm-build")
            .build()
            .context("failed to build reqwest client")?;
        let mut response = client
            .get(&source_url)
            .send()
            .context("failed to download llama.cpp source tarball")?
            .error_for_status()
            .context("source tarball request failed")?;
        let mut file =
            fs::File::create(&tarball_path).context("failed to create source tarball file")?;
        io::copy(&mut response, &mut file).context("failed to write source tarball")?;
        file.flush().context("failed to flush source tarball")?;
    }

    let tarball =
        fs::File::open(&tarball_path).context("failed to reopen downloaded source tarball")?;
    let decoder = GzDecoder::new(tarball);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(&cache_dir)
        .context("failed to extract llama.cpp source tarball")?;

    if !source_root.join("include/llama.h").exists() {
        bail!(
            "expected extracted source tree at `{}` but llama.h was not found",
            source_root.display()
        );
    }

    Ok(source_root)
}

fn write_wrapper_header(out_dir: &Path) -> Result<PathBuf> {
    let header_path = out_dir.join("koharu_llm_bindings.h");
    let header = r#"
#include "ggml.h"
#include "gguf.h"
#include "llama.h"
#include "mtmd.h"
#include "mtmd-helper.h"
"#;
    fs::write(&header_path, header).context("failed to write bindings wrapper header")?;
    Ok(header_path)
}

fn include_dirs(source_root: &Path) -> Vec<PathBuf> {
    vec![
        source_root.join("ggml/include"),
        source_root.join("include"),
        source_root.join("tools/mtmd"),
    ]
}

fn base_builder(header_path: &Path, include_dirs: &[PathBuf]) -> bindgen::Builder {
    let callbacks: Box<dyn ParseCallbacks> = Box::new(bindgen::CargoCallbacks::new());
    include_dirs.iter().fold(
        bindgen::Builder::default()
            .header(header_path.display().to_string())
            .layout_tests(false)
            .prepend_enum_name(false)
            .wrap_unsafe_ops(true)
            .parse_callbacks(callbacks),
        |builder, include_dir| builder.clang_arg(format!("-I{}", include_dir.display())),
    )
}

fn generate_types(out_dir: &Path, header_path: &Path, include_dirs: &[PathBuf]) -> Result<()> {
    let bindings = base_builder(header_path, include_dirs)
        .allowlist_type("^(llama|ggml|gguf|mtmd).*")
        .allowlist_var("^(LLAMA|GGML|GGUF|MTMD).*")
        .blocklist_function(".*")
        .generate()
        .context("failed to generate type bindings")?;

    bindings
        .write_to_file(out_dir.join("types.rs"))
        .context("failed to write type bindings")?;

    Ok(())
}

fn generate_loader(
    out_dir: &Path,
    header_path: &Path,
    include_dirs: &[PathBuf],
    spec: LoaderSpec<'_>,
) -> Result<()> {
    let builder = spec.function_patterns.iter().fold(
        base_builder(header_path, include_dirs),
        |builder, pattern| builder.allowlist_function(pattern),
    );

    let bindings = builder
        .allowlist_recursively(false)
        .dynamic_library_name(spec.module_name)
        .dynamic_link_require_all(true)
        .generate()
        .with_context(|| format!("failed to generate {} runtime loader", spec.module_name))?;

    bindings
        .write_to_file(out_dir.join(format!("{}_loader.rs", spec.module_name)))
        .with_context(|| format!("failed to write {} runtime loader", spec.module_name))?;

    Ok(())
}

fn generate_wrappers(out_dir: &Path, modules: &[(&str, &str)]) -> Result<()> {
    let mut wrapper_items = Vec::new();

    for (module_name, accessor) in modules {
        let loader_path = out_dir.join(format!("{module_name}_loader.rs"));
        let loader_source = fs::read_to_string(&loader_path)
            .with_context(|| format!("failed to read {module_name} loader"))?;
        let file = syn::parse_file(&loader_source)
            .with_context(|| format!("failed to parse {module_name} loader"))?;
        let loader_impl = file.items.iter().find_map(|item| match item {
            Item::Impl(item) => Some(item),
            _ => None,
        });
        let Some(loader_impl) = loader_impl else {
            bail!("generated {module_name} loader did not contain an impl block");
        };

        let accessor_ident = format_ident!("{accessor}");

        for item in &loader_impl.items {
            let ImplItem::Fn(method) = item else {
                continue;
            };

            if matches!(
                method.sig.ident.to_string().as_str(),
                "new" | "from_library"
            ) {
                continue;
            }

            wrapper_items.push(generate_wrapper(
                accessor_ident.clone(),
                &method.sig,
                &method.attrs,
            )?);
        }
    }

    let wrappers = quote! {
        #(#wrapper_items)*
    };
    fs::write(out_dir.join("wrappers.rs"), wrappers.to_string())
        .context("failed to write generated wrapper functions")?;

    Ok(())
}

fn generate_wrapper(
    accessor_ident: syn::Ident,
    signature: &Signature,
    attrs: &[syn::Attribute],
) -> Result<proc_macro2::TokenStream> {
    let mut wrapper_sig = signature.clone();
    wrapper_sig.inputs = wrapper_sig
        .inputs
        .into_iter()
        .filter(|arg| !matches!(arg, FnArg::Receiver(_)))
        .collect();

    let arg_names = wrapper_sig
        .inputs
        .iter()
        .map(|arg| match arg {
            FnArg::Receiver(_) => bail!("unexpected method receiver after filtering"),
            FnArg::Typed(arg) => match arg.pat.as_ref() {
                Pat::Ident(ident) => Ok(quote! { #ident }),
                other => Ok(quote! { #other }),
            },
        })
        .collect::<Result<Vec<_>>>()?;

    let method_ident = &signature.ident;
    let wrapper = quote! {
        #(#attrs)*
        #[inline]
        pub #wrapper_sig {
            unsafe { #accessor_ident().#method_ident(#(#arg_names),*) }
        }
    };

    Ok(wrapper)
}

struct LoaderSpec<'a> {
    module_name: &'a str,
    function_patterns: &'a [&'a str],
}
