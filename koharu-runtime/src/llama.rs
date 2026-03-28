use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::archive;
use crate::loader::{add_runtime_search_path, preload_library};

const LLAMA_CPP_TAG: &str = env!("LLAMA_CPP_TAG");
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum LlamaRuntime {
    WindowsCuda13X64,
    WindowsVulkanX64,
    LinuxVulkanX64,
    MacosArm64,
}

impl LlamaRuntime {
    #[allow(clippy::needless_return)]
    fn detect() -> Result<Self> {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        if unsafe { libloading::Library::new("nvcuda.dll") }.is_ok() {
            return Ok(Self::WindowsCuda13X64);
        } else {
            return Ok(Self::WindowsVulkanX64);
        }

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        return Ok(Self::LinuxVulkanX64);

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        return Ok(Self::MacosArm64);

        #[cfg(not(any(
            all(target_os = "windows", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64")
        )))]
        bail!(
            "unsupported platform: os={}, arch={}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    }

    fn id(self) -> &'static str {
        match self {
            Self::WindowsCuda13X64 => "windows-cuda13-x64",
            Self::WindowsVulkanX64 => "windows-vulkan-x64",
            Self::LinuxVulkanX64 => "linux-vulkan-x64",
            Self::MacosArm64 => "macos-arm64",
        }
    }

    fn assets(self) -> &'static [&'static str] {
        match self {
            Self::WindowsCuda13X64 => &[
                "llama-b8233-bin-win-cuda-13.1-x64.zip",
                "cudart-llama-bin-win-cuda-13.1-x64.zip",
            ],
            Self::WindowsVulkanX64 => &["llama-b8233-bin-win-vulkan-x64.zip"],
            Self::LinuxVulkanX64 => &["llama-b8233-bin-ubuntu-vulkan-x64.tar.gz"],
            Self::MacosArm64 => &["llama-b8233-bin-macos-arm64.tar.gz"],
        }
    }

    fn libraries(self) -> &'static [&'static str] {
        match self {
            Self::WindowsCuda13X64 => &[
                "cudart64_13.dll",
                "cublasLt64_13.dll",
                "cublas64_13.dll",
                "libomp140.x86_64.dll",
                "ggml-base.dll",
                "ggml.dll",
                "ggml-cpu-alderlake.dll",
                "ggml-cpu-cannonlake.dll",
                "ggml-cpu-cascadelake.dll",
                "ggml-cpu-cooperlake.dll",
                "ggml-cpu-haswell.dll",
                "ggml-cpu-icelake.dll",
                "ggml-cpu-ivybridge.dll",
                "ggml-cpu-piledriver.dll",
                "ggml-cpu-sandybridge.dll",
                "ggml-cpu-sapphirerapids.dll",
                "ggml-cpu-skylakex.dll",
                "ggml-cpu-sse42.dll",
                "ggml-cpu-x64.dll",
                "ggml-cpu-zen4.dll",
                "ggml-cuda.dll",
                "ggml-rpc.dll",
                "llama.dll",
                "mtmd.dll",
            ],
            Self::WindowsVulkanX64 => &[
                "libomp140.x86_64.dll",
                "ggml-base.dll",
                "ggml.dll",
                "ggml-cpu-alderlake.dll",
                "ggml-cpu-cannonlake.dll",
                "ggml-cpu-cascadelake.dll",
                "ggml-cpu-cooperlake.dll",
                "ggml-cpu-haswell.dll",
                "ggml-cpu-icelake.dll",
                "ggml-cpu-ivybridge.dll",
                "ggml-cpu-piledriver.dll",
                "ggml-cpu-sandybridge.dll",
                "ggml-cpu-sapphirerapids.dll",
                "ggml-cpu-skylakex.dll",
                "ggml-cpu-sse42.dll",
                "ggml-cpu-x64.dll",
                "ggml-cpu-zen4.dll",
                "ggml-rpc.dll",
                "ggml-vulkan.dll",
                "llama.dll",
                "mtmd.dll",
            ],
            Self::LinuxVulkanX64 => &[
                "libggml-base.so",
                "libggml.so",
                "libggml-cpu-alderlake.so",
                "libggml-cpu-cannonlake.so",
                "libggml-cpu-cascadelake.so",
                "libggml-cpu-cooperlake.so",
                "libggml-cpu-haswell.so",
                "libggml-cpu-icelake.so",
                "libggml-cpu-ivybridge.so",
                "libggml-cpu-piledriver.so",
                "libggml-cpu-sandybridge.so",
                "libggml-cpu-sapphirerapids.so",
                "libggml-cpu-skylakex.so",
                "libggml-cpu-sse42.so",
                "libggml-cpu-x64.so",
                "libggml-cpu-zen4.so",
                "libggml-rpc.so",
                "libggml-vulkan.so",
                "libllama.so",
                "libmtmd.so",
            ],
            Self::MacosArm64 => &[
                "libggml-base.dylib",
                "libggml.dylib",
                "libggml-blas.dylib",
                "libggml-cpu.dylib",
                "libggml-metal.dylib",
                "libggml-rpc.dylib",
                "libllama.dylib",
                "libmtmd.dylib",
            ],
        }
    }

    fn install_dir(self, root: &Path) -> PathBuf {
        root.join("llama.cpp").join(LLAMA_CPP_TAG).join(self.id())
    }

    fn source_id(self) -> String {
        format!("llama-{LLAMA_CPP_TAG}-{}", self.id())
    }

    fn is_zip_asset(asset: &str) -> bool {
        asset.ends_with(".zip")
    }

    async fn install(self, install_dir: &Path, downloads_dir: &Path) -> Result<()> {
        for asset in self.assets() {
            let url = format!(
                "{}/{LLAMA_CPP_TAG}/{asset}",
                koharu_http::config::github_release_base_url()
            );
            let archive = archive::download_cached(&url, asset, downloads_dir).await?;
            if Self::is_zip_asset(asset) {
                archive::extract_zip(&archive, install_dir)?;
            } else {
                archive::extract_tar_gz(&archive, install_dir)?;
            }
        }

        for lib in self.libraries() {
            if !install_dir.join(lib).exists() {
                bail!(
                    "required library `{lib}` missing from `{}`",
                    install_dir.display()
                );
            }
        }

        Ok(())
    }
}

pub(crate) async fn ensure_ready(root: &Path, downloads_dir: &Path) -> Result<()> {
    let runtime = LlamaRuntime::detect()?;
    let install_dir = runtime.install_dir(root);
    let source_id = runtime.source_id();

    if !crate::is_up_to_date(&install_dir, &source_id) {
        crate::reset_dir(&install_dir)?;
        runtime.install(&install_dir, downloads_dir).await?;
        crate::mark_installed(&install_dir, &source_id)?;
    }

    add_runtime_search_path(&install_dir)?;
    for lib in runtime.libraries() {
        preload_library(&install_dir.join(lib))?;
    }

    Ok(())
}

pub(crate) fn runtime_dir(root: &Path) -> Result<PathBuf> {
    Ok(LlamaRuntime::detect()?.install_dir(root))
}

pub(crate) fn runtime_install_dir_for_current_platform(root: &Path) -> Option<PathBuf> {
    LlamaRuntime::detect()
        .ok()
        .map(|runtime| runtime.install_dir(root))
}

pub(crate) fn required_libraries_for_current_platform() -> &'static [&'static str] {
    LlamaRuntime::detect()
        .map(|runtime| runtime.libraries())
        .unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    fn touch(path: &Path) {
        fs::write(path, b"ok").unwrap();
    }

    #[test]
    fn detect_returns_a_variant_for_current_platform() {
        let runtime = LlamaRuntime::detect().unwrap();
        assert!(!runtime.id().is_empty());
        assert!(!runtime.assets().is_empty());
        assert!(!runtime.libraries().is_empty());
    }

    #[test]
    fn install_dir_includes_tag_and_id() {
        let root = Path::new("/tmp/rt");
        let dir = LlamaRuntime::WindowsVulkanX64.install_dir(root);
        assert!(
            dir.ends_with(
                Path::new("llama.cpp")
                    .join(LLAMA_CPP_TAG)
                    .join("windows-vulkan-x64")
            )
        );
    }

    #[test]
    fn preload_order_matches_libraries() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        let rt = LlamaRuntime::WindowsCuda13X64;

        for lib in rt.libraries() {
            touch(&root.join(lib));
        }

        let paths: Vec<PathBuf> = rt.libraries().iter().map(|l| root.join(l)).collect();
        assert!(paths.iter().all(|p| p.exists()));
        assert_eq!(paths.len(), rt.libraries().len());
    }
}
