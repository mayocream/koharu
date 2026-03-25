use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use futures::StreamExt;
use tokio::io::AsyncWriteExt;
use zip::read::ZipArchive;

const LLAMA_CPP_TAG: &str = env!("KOHARU_LLM_LLAMA_CPP_TAG");
const RELEASE_BASE_URL: &str = "https://github.com/ggml-org/llama.cpp/releases/download";
const STAMP_FILE: &str = ".koharu-llm-runtime-stamp";
const DOWNLOADS_DIR: &str = ".downloads";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveKind {
    Zip,
    TarGz,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Cuda,
    Vulkan,
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoreLibraryKind {
    GgmlBase,
    Ggml,
    Llama,
    Mtmd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoadStep {
    pub(crate) path: PathBuf,
    pub(crate) core: Option<CoreLibraryKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReleaseAsset {
    name: &'static str,
    kind: ArchiveKind,
}

#[derive(Debug, Clone, Copy)]
struct Manifest {
    id: &'static str,
    support_libraries: &'static [&'static str],
    support_prefixes: &'static [&'static str],
    core_ggml_base: &'static str,
    core_ggml: &'static str,
    core_llama: &'static str,
    core_mtmd: &'static str,
    plugin_prefixes: &'static [&'static str],
    library_extension: &'static str,
    assets: &'static [ReleaseAsset],
}

const WINDOWS_CUDA_ASSETS: &[ReleaseAsset] = &[
    ReleaseAsset {
        name: "llama-b8233-bin-win-cuda-13.1-x64.zip",
        kind: ArchiveKind::Zip,
    },
    ReleaseAsset {
        name: "cudart-llama-bin-win-cuda-13.1-x64.zip",
        kind: ArchiveKind::Zip,
    },
];

const WINDOWS_VULKAN_ASSETS: &[ReleaseAsset] = &[ReleaseAsset {
    name: "llama-b8233-bin-win-vulkan-x64.zip",
    kind: ArchiveKind::Zip,
}];

const LINUX_VULKAN_ASSETS: &[ReleaseAsset] = &[ReleaseAsset {
    name: "llama-b8233-bin-ubuntu-vulkan-x64.tar.gz",
    kind: ArchiveKind::TarGz,
}];

const MACOS_ARM64_ASSETS: &[ReleaseAsset] = &[ReleaseAsset {
    name: "llama-b8233-bin-macos-arm64.tar.gz",
    kind: ArchiveKind::TarGz,
}];

const WINDOWS_CUDA_MANIFEST: Manifest = Manifest {
    id: "windows-cuda13-x64",
    support_libraries: &["cudart64_13.dll", "cublasLt64_13.dll", "cublas64_13.dll"],
    support_prefixes: &["libomp"],
    core_ggml_base: "ggml-base.dll",
    core_ggml: "ggml.dll",
    core_llama: "llama.dll",
    core_mtmd: "mtmd.dll",
    plugin_prefixes: &["ggml-"],
    library_extension: ".dll",
    assets: WINDOWS_CUDA_ASSETS,
};

const WINDOWS_VULKAN_MANIFEST: Manifest = Manifest {
    id: "windows-vulkan-x64",
    support_libraries: &[],
    support_prefixes: &["libomp"],
    core_ggml_base: "ggml-base.dll",
    core_ggml: "ggml.dll",
    core_llama: "llama.dll",
    core_mtmd: "mtmd.dll",
    plugin_prefixes: &["ggml-"],
    library_extension: ".dll",
    assets: WINDOWS_VULKAN_ASSETS,
};

const LINUX_VULKAN_MANIFEST: Manifest = Manifest {
    id: "linux-vulkan-x64",
    support_libraries: &[],
    support_prefixes: &[],
    core_ggml_base: "libggml-base.so",
    core_ggml: "libggml.so",
    core_llama: "libllama.so",
    core_mtmd: "libmtmd.so",
    plugin_prefixes: &["libggml-"],
    library_extension: ".so",
    assets: LINUX_VULKAN_ASSETS,
};

const MACOS_ARM64_MANIFEST: Manifest = Manifest {
    id: "macos-arm64",
    support_libraries: &[],
    support_prefixes: &[],
    core_ggml_base: "libggml-base.dylib",
    core_ggml: "libggml.dylib",
    core_llama: "libllama.dylib",
    core_mtmd: "libmtmd.dylib",
    plugin_prefixes: &["libggml-"],
    library_extension: ".dylib",
    assets: MACOS_ARM64_ASSETS,
};

pub async fn ensure_dylibs(path: impl AsRef<Path>) -> Result<()> {
    let root = path.as_ref().to_path_buf();
    let manifest = compiled_manifest();

    tokio::fs::create_dir_all(&root)
        .await
        .with_context(|| format!("failed to create runtime directory `{}`", root.display()))?;

    if stamp_matches(&root, manifest)? && core_libraries_present(&root, manifest) {
        return Ok(());
    }

    let downloads_dir = root.join(DOWNLOADS_DIR);
    tokio::fs::create_dir_all(&downloads_dir)
        .await
        .with_context(|| {
            format!(
                "failed to create download cache `{}`",
                downloads_dir.display()
            )
        })?;

    for asset in manifest.assets {
        let archive_path = download_asset(asset, &downloads_dir).await?;
        let extract_root = root.clone();
        let asset_name = asset.name;
        tokio::task::spawn_blocking(move || extract_archive(asset, &archive_path, &extract_root))
            .await
            .with_context(|| format!("failed to join extraction task for `{asset_name}`"))??;
    }

    write_stamp(&root, manifest)?;
    Ok(())
}

pub fn initialize(path: impl AsRef<Path>) -> Result<()> {
    crate::sys::initialize(path.as_ref())
}

pub(crate) fn load_plan(dir: &Path) -> Result<Vec<LoadStep>> {
    let manifest = compiled_manifest();
    let mut steps = Vec::new();

    for library_name in manifest.support_libraries {
        let library_path = dir.join(library_name);
        if library_path.exists() {
            steps.push(LoadStep {
                path: library_path,
                core: None,
            });
        }
    }

    steps.extend(scan_prefixed_libraries(
        dir,
        manifest,
        manifest.support_prefixes,
        core_library_names(manifest),
        manifest.support_libraries,
    )?);

    steps.push(required_core_step(
        dir,
        manifest.core_ggml_base,
        CoreLibraryKind::GgmlBase,
    )?);
    steps.push(required_core_step(
        dir,
        manifest.core_ggml,
        CoreLibraryKind::Ggml,
    )?);

    steps.extend(scan_prefixed_libraries(
        dir,
        manifest,
        manifest.plugin_prefixes,
        core_library_names(manifest),
        manifest.support_libraries,
    )?);

    steps.push(required_core_step(
        dir,
        manifest.core_llama,
        CoreLibraryKind::Llama,
    )?);
    steps.push(required_core_step(
        dir,
        manifest.core_mtmd,
        CoreLibraryKind::Mtmd,
    )?);

    Ok(steps)
}

fn compiled_manifest() -> &'static Manifest {
    let backend = compiled_backend();
    select_manifest(env::consts::OS, env::consts::ARCH, target_env(), backend)
        .expect("build.rs validates supported target/backend combinations")
}

fn compiled_backend() -> Backend {
    #[cfg(feature = "cuda")]
    {
        Backend::Cuda
    }

    #[cfg(all(not(feature = "cuda"), feature = "vulkan"))]
    {
        Backend::Vulkan
    }

    #[cfg(all(not(feature = "cuda"), not(feature = "vulkan")))]
    {
        Backend::Default
    }
}

fn target_env() -> &'static str {
    #[cfg(target_env = "msvc")]
    {
        "msvc"
    }

    #[cfg(not(target_env = "msvc"))]
    {
        ""
    }
}

fn select_manifest(
    target_os: &str,
    target_arch: &str,
    target_env: &str,
    backend: Backend,
) -> Result<&'static Manifest> {
    match (target_os, target_arch, target_env, backend) {
        ("windows", "x86_64", "msvc", Backend::Cuda) => Ok(&WINDOWS_CUDA_MANIFEST),
        ("windows", "x86_64", "msvc", Backend::Vulkan) => Ok(&WINDOWS_VULKAN_MANIFEST),
        ("linux", "x86_64", _, Backend::Vulkan) => Ok(&LINUX_VULKAN_MANIFEST),
        ("macos", "aarch64", _, Backend::Default) => Ok(&MACOS_ARM64_MANIFEST),
        _ => bail!(
            "unsupported koharu-llm target/backend combination: target_os={target_os}, target_arch={target_arch}, target_env={target_env}, backend={backend:?}"
        ),
    }
}

fn core_libraries_present(dir: &Path, manifest: &Manifest) -> bool {
    core_library_names(manifest)
        .iter()
        .all(|name| dir.join(name).exists())
}

fn core_library_names(manifest: &Manifest) -> [&'static str; 4] {
    [
        manifest.core_ggml_base,
        manifest.core_ggml,
        manifest.core_llama,
        manifest.core_mtmd,
    ]
}

fn required_core_step(
    dir: &Path,
    library_name: &'static str,
    core: CoreLibraryKind,
) -> Result<LoadStep> {
    let path = dir.join(library_name);
    if !path.exists() {
        bail!(
            "required runtime library `{}` is missing from `{}`; run `koharu_llm::runtime::ensure_dylibs` first",
            library_name,
            dir.display()
        );
    }

    Ok(LoadStep {
        path,
        core: Some(core),
    })
}

fn scan_prefixed_libraries(
    dir: &Path,
    manifest: &Manifest,
    prefixes: &[&str],
    core_names: [&str; 4],
    explicit_support_names: &[&str],
) -> Result<Vec<LoadStep>> {
    let mut paths = Vec::new();

    for entry in fs::read_dir(dir)
        .with_context(|| format!("failed to read runtime directory `{}`", dir.display()))?
    {
        let entry = entry.context("failed to read runtime directory entry")?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        if !file_name.ends_with(manifest.library_extension) {
            continue;
        }
        if core_names.contains(&file_name) || explicit_support_names.contains(&file_name) {
            continue;
        }
        if prefixes.iter().any(|prefix| file_name.starts_with(prefix)) {
            paths.push(LoadStep { path, core: None });
        }
    }

    paths.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(paths)
}

fn release_asset_url(asset_name: &str) -> String {
    format!("{RELEASE_BASE_URL}/{LLAMA_CPP_TAG}/{asset_name}")
}

async fn download_asset(asset: &ReleaseAsset, downloads_dir: &Path) -> Result<PathBuf> {
    let archive_path = downloads_dir.join(asset.name);
    if archive_path.exists() {
        return Ok(archive_path);
    }

    let partial_path = downloads_dir.join(format!("{}.partial", asset.name));
    let url = release_asset_url(asset.name);
    let response = reqwest::Client::builder()
        .user_agent("koharu-llm-runtime")
        .build()
        .context("failed to build reqwest client")?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to download `{url}`"))?
        .error_for_status()
        .with_context(|| format!("download failed for `{url}`"))?;

    let mut file = tokio::fs::File::create(&partial_path)
        .await
        .with_context(|| format!("failed to create `{}`", partial_path.display()))?;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("failed while downloading `{url}`"))?;
        file.write_all(&chunk)
            .await
            .with_context(|| format!("failed to write `{}`", partial_path.display()))?;
    }

    file.flush()
        .await
        .with_context(|| format!("failed to flush `{}`", partial_path.display()))?;
    drop(file);

    tokio::fs::rename(&partial_path, &archive_path)
        .await
        .with_context(|| format!("failed to finalize `{}`", archive_path.display()))?;

    Ok(archive_path)
}

fn extract_archive(asset: &ReleaseAsset, archive_path: &Path, output_dir: &Path) -> Result<()> {
    match asset.kind {
        ArchiveKind::Zip => extract_zip_archive(archive_path, output_dir),
        ArchiveKind::TarGz => extract_tar_archive(archive_path, output_dir),
    }
}

fn extract_zip_archive(archive_path: &Path, output_dir: &Path) -> Result<()> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open `{}`", archive_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read zip archive `{}`", archive_path.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).with_context(|| {
            format!(
                "failed to read zip entry {index} from `{}`",
                archive_path.display()
            )
        })?;
        if entry.is_dir() {
            continue;
        }

        let Some(file_name) = Path::new(entry.name())
            .file_name()
            .and_then(OsStr::to_str)
            .map(ToOwned::to_owned)
        else {
            continue;
        };
        if !looks_like_runtime_library(&file_name) {
            continue;
        }

        let out_path = output_dir.join(&file_name);
        let mut out_file = fs::File::create(&out_path)
            .with_context(|| format!("failed to create `{}`", out_path.display()))?;
        io::copy(&mut entry, &mut out_file)
            .with_context(|| format!("failed to extract `{}`", out_path.display()))?;
    }

    Ok(())
}

fn extract_tar_archive(archive_path: &Path, output_dir: &Path) -> Result<()> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open `{}`", archive_path.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut aliases = Vec::new();

    for entry in archive.entries().with_context(|| {
        format!(
            "failed to read tar entries from `{}`",
            archive_path.display()
        )
    })? {
        let mut entry = entry.context("failed to read tar entry")?;
        let path = entry.path().context("failed to read tar entry path")?;
        let Some(file_name) = path
            .file_name()
            .and_then(OsStr::to_str)
            .map(ToOwned::to_owned)
        else {
            continue;
        };
        if !looks_like_runtime_library(&file_name) {
            continue;
        }

        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() {
            let Some(target_name) = entry
                .link_name()
                .context("failed to read tar symlink target")?
                .and_then(|target| target.file_name().map(ToOwned::to_owned))
                .and_then(|name| name.to_str().map(ToOwned::to_owned))
            else {
                continue;
            };
            aliases.push((output_dir.join(file_name), output_dir.join(target_name)));
            continue;
        }

        if !entry_type.is_file() {
            continue;
        }

        let out_path = output_dir.join(&file_name);
        let mut out_file = fs::File::create(&out_path)
            .with_context(|| format!("failed to create `{}`", out_path.display()))?;
        io::copy(&mut entry, &mut out_file)
            .with_context(|| format!("failed to extract `{}`", out_path.display()))?;
    }

    materialize_aliases(&aliases)?;
    Ok(())
}

fn materialize_aliases(aliases: &[(PathBuf, PathBuf)]) -> Result<()> {
    let mut pending = aliases.to_vec();

    while !pending.is_empty() {
        let mut progressed = false;
        let mut next = Vec::new();

        for (alias_path, target_path) in pending {
            if alias_path.exists() {
                progressed = true;
                continue;
            }
            if !target_path.exists() {
                next.push((alias_path, target_path));
                continue;
            }

            match fs::hard_link(&target_path, &alias_path) {
                Ok(()) => {}
                Err(_) => {
                    fs::copy(&target_path, &alias_path).with_context(|| {
                        format!(
                            "failed to materialize alias `{}` from `{}`",
                            alias_path.display(),
                            target_path.display()
                        )
                    })?;
                }
            }
            progressed = true;
        }

        if !progressed {
            let unresolved = next
                .into_iter()
                .map(|(alias, target)| format!("{} -> {}", alias.display(), target.display()))
                .collect::<Vec<_>>()
                .join(", ");
            bail!("failed to materialize runtime library aliases: {unresolved}");
        }

        pending = next;
    }

    Ok(())
}

fn looks_like_runtime_library(file_name: &str) -> bool {
    file_name.ends_with(".dll")
        || file_name.ends_with(".so")
        || file_name.contains(".so.")
        || file_name.ends_with(".dylib")
        || file_name.contains(".dylib.")
}

fn stamp_matches(root: &Path, manifest: &Manifest) -> Result<bool> {
    let stamp_path = root.join(STAMP_FILE);
    if !stamp_path.exists() {
        return Ok(false);
    }

    let contents = fs::read_to_string(&stamp_path)
        .with_context(|| format!("failed to read `{}`", stamp_path.display()))?;
    Ok(contents == manifest_stamp(manifest))
}

fn write_stamp(root: &Path, manifest: &Manifest) -> Result<()> {
    let stamp_path = root.join(STAMP_FILE);
    fs::write(&stamp_path, manifest_stamp(manifest))
        .with_context(|| format!("failed to write `{}`", stamp_path.display()))?;
    Ok(())
}

fn manifest_stamp(manifest: &Manifest) -> String {
    let mut stamp = format!("{LLAMA_CPP_TAG}\n{}\n", manifest.id);
    for asset in manifest.assets {
        stamp.push_str(asset.name);
        stamp.push('\n');
    }
    stamp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        fs::write(path, b"ok").unwrap();
    }

    #[test]
    fn selects_supported_manifests() {
        assert_eq!(
            select_manifest("windows", "x86_64", "msvc", Backend::Cuda)
                .unwrap()
                .id,
            WINDOWS_CUDA_MANIFEST.id
        );
        assert_eq!(
            select_manifest("windows", "x86_64", "msvc", Backend::Vulkan)
                .unwrap()
                .id,
            WINDOWS_VULKAN_MANIFEST.id
        );
        assert_eq!(
            select_manifest("linux", "x86_64", "", Backend::Vulkan)
                .unwrap()
                .id,
            LINUX_VULKAN_MANIFEST.id
        );
        assert_eq!(
            select_manifest("macos", "aarch64", "", Backend::Default)
                .unwrap()
                .id,
            MACOS_ARM64_MANIFEST.id
        );
    }

    #[test]
    fn rejects_unsupported_manifest() {
        assert!(select_manifest("linux", "x86_64", "", Backend::Cuda).is_err());
        assert!(select_manifest("windows", "aarch64", "msvc", Backend::Cuda).is_err());
    }

    #[test]
    fn manifest_stamp_is_stable() {
        let stamp = manifest_stamp(&WINDOWS_CUDA_MANIFEST);
        assert!(stamp.contains(LLAMA_CPP_TAG));
        assert!(stamp.contains("windows-cuda13-x64"));
        assert!(stamp.contains("llama-b8233-bin-win-cuda-13.1-x64.zip"));
    }

    #[test]
    fn materializes_aliases_by_copy() {
        let tempdir = tempfile::tempdir().unwrap();
        let target = tempdir.path().join("libllama.so.0.0.8233");
        let alias = tempdir.path().join("libllama.so");

        touch(&target);
        materialize_aliases(&[(alias.clone(), target.clone())]).unwrap();

        assert!(alias.exists());
        assert_eq!(fs::read(alias).unwrap(), fs::read(target).unwrap());
    }

    #[test]
    fn load_plan_orders_core_and_plugins() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        let manifest = &WINDOWS_CUDA_MANIFEST;

        for core in core_library_names(manifest) {
            touch(&root.join(core));
        }
        for support in manifest.support_libraries {
            touch(&root.join(support));
        }
        touch(&root.join("libomp140.x86_64.dll"));
        touch(&root.join("ggml-cuda.dll"));
        touch(&root.join("ggml-rpc.dll"));

        let steps = load_plan(root).unwrap();
        let names = steps
            .iter()
            .map(|step| {
                step.path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();

        assert_eq!(names[0], "cudart64_13.dll");
        assert_eq!(names[1], "cublasLt64_13.dll");
        assert_eq!(names[2], "cublas64_13.dll");
        assert_eq!(names[3], "libomp140.x86_64.dll");
        assert_eq!(names[4], "ggml-base.dll");
        assert_eq!(names[5], "ggml.dll");
        assert!(names[6..names.len() - 2].contains(&"ggml-cuda.dll".to_string()));
        assert_eq!(names[names.len() - 2], "llama.dll");
        assert_eq!(names[names.len() - 1], "mtmd.dll");
    }

    #[tokio::test]
    #[ignore]
    async fn downloads_runtime_assets() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        ensure_dylibs(tempdir.path()).await?;
        assert!(core_libraries_present(tempdir.path(), compiled_manifest()));
        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn initializes_runtime_assets() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        ensure_dylibs(tempdir.path()).await?;
        initialize(tempdir.path())?;
        Ok(())
    }
}
