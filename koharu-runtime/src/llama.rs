use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::Runtime;
use crate::archive::{self, ExtractPolicy};
use crate::install::InstallState;
use crate::loader::{add_runtime_search_path, preload_library};

const LLAMA_CPP_TAG: &str = env!("LLAMA_CPP_TAG");
const RELEASE_BASE_URL: &str = "https://github.com/ggml-org/llama.cpp/releases/download";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum LlamaDistribution {
    WindowsCuda13X64,
    WindowsVulkanX64,
    LinuxVulkanX64,
    MacosArm64,
}

impl LlamaDistribution {
    #[allow(clippy::needless_return)]
    fn detect(_runtime: &Runtime) -> Result<Self> {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        return Ok(Self::windows_x64(_runtime));

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

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    fn windows_x64(runtime: &Runtime) -> Self {
        if crate::zluda::package_enabled(runtime) {
            Self::WindowsVulkanX64
        } else if crate::cuda::llama_cuda_enabled(runtime) {
            Self::WindowsCuda13X64
        } else {
            Self::WindowsVulkanX64
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::WindowsCuda13X64 => "windows-cuda13-x64",
            Self::WindowsVulkanX64 => "windows-vulkan-x64",
            Self::LinuxVulkanX64 => "linux-vulkan-x64",
            Self::MacosArm64 => "macos-arm64",
        }
    }

    fn assets(self) -> Vec<String> {
        let tag = LLAMA_CPP_TAG;
        match self {
            Self::WindowsCuda13X64 => vec![
                format!("llama-{tag}-bin-win-cuda-13.1-x64.zip"),
                "cudart-llama-bin-win-cuda-13.1-x64.zip".to_string(),
            ],
            Self::WindowsVulkanX64 => vec![format!("llama-{tag}-bin-win-vulkan-x64.zip")],
            Self::LinuxVulkanX64 => vec![format!("llama-{tag}-bin-ubuntu-vulkan-x64.tar.gz")],
            Self::MacosArm64 => vec![format!("llama-{tag}-bin-macos-arm64.tar.gz")],
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

    fn install_dir(self, runtime: &Runtime) -> PathBuf {
        runtime
            .root()
            .join("runtime")
            .join("llama.cpp")
            .join(LLAMA_CPP_TAG)
            .join(self.id())
    }

    fn source_id(self) -> String {
        format!("llama-{LLAMA_CPP_TAG}-{}", self.id())
    }
}

pub(crate) fn package_enabled(runtime: &Runtime) -> bool {
    LlamaDistribution::detect(runtime).is_ok()
}

pub(crate) fn package_present(runtime: &Runtime) -> Result<bool> {
    let distribution = LlamaDistribution::detect(runtime)?;
    let install_dir = distribution.install_dir(runtime);
    let source_id = distribution.source_id();
    let install = InstallState::new(&install_dir, &source_id);
    if !install.is_current() {
        return Ok(false);
    }

    Ok(distribution
        .libraries()
        .iter()
        .all(|library| install_dir.join(library).exists()))
}

pub(crate) async fn package_prepare(runtime: &Runtime) -> Result<()> {
    ensure_ready(runtime).await
}

pub(crate) async fn ensure_ready(runtime: &Runtime) -> Result<()> {
    let distribution = LlamaDistribution::detect(runtime)?;
    let install_dir = distribution.install_dir(runtime);
    let source_id = distribution.source_id();
    let install = InstallState::new(&install_dir, &source_id);

    if !install.is_current() {
        install.reset()?;

        for asset in &distribution.assets() {
            let url = format!("{RELEASE_BASE_URL}/{LLAMA_CPP_TAG}/{asset}");
            let archive = runtime
                .downloads()
                .cached_download(&url, asset)
                .await
                .with_context(|| format!("failed to download `{url}`"))?;
            let kind = archive::detect_kind(asset)?;
            archive::extract(
                &archive,
                &install_dir,
                kind,
                ExtractPolicy::RuntimeLibraries,
            )?;
        }

        for library in distribution.libraries() {
            if !install_dir.join(library).exists() {
                bail!(
                    "required library `{library}` missing from `{}`",
                    install_dir.display()
                );
            }
        }

        install.commit()?;
    }

    add_runtime_search_path(&install_dir)?;
    for library in distribution.libraries() {
        preload_library(&install_dir.join(library))?;
    }

    Ok(())
}

pub(crate) fn runtime_dir(runtime: &Runtime) -> Result<PathBuf> {
    Ok(LlamaDistribution::detect(runtime)?.install_dir(runtime))
}

crate::declare_native_package!(
    id: "runtime:llama",
    bootstrap: true,
    order: 20,
    enabled: crate::llama::package_enabled,
    present: crate::llama::package_present,
    prepare: crate::llama::package_prepare,
);

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
        let runtime = Runtime::new("/tmp/koharu-runtime", crate::ComputePolicy::PreferGpu).unwrap();
        let distribution = LlamaDistribution::detect(&runtime).unwrap();
        assert!(!distribution.id().is_empty());
        assert!(!distribution.assets().is_empty());
        assert!(!distribution.libraries().is_empty());
    }

    #[test]
    fn install_dir_includes_tag_and_id() {
        let runtime = Runtime::new("/tmp/koharu-runtime", crate::ComputePolicy::CpuOnly).unwrap();
        let dir = LlamaDistribution::WindowsVulkanX64.install_dir(&runtime);
        assert!(
            dir.ends_with(
                std::path::Path::new("llama.cpp")
                    .join(LLAMA_CPP_TAG)
                    .join("windows-vulkan-x64")
            )
        );
    }

    #[test]
    fn preload_order_matches_libraries() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();
        let runtime = LlamaDistribution::WindowsCuda13X64;

        for library in runtime.libraries() {
            touch(&root.join(library));
        }

        let paths: Vec<PathBuf> = runtime
            .libraries()
            .iter()
            .map(|library| root.join(library))
            .collect();
        assert!(paths.iter().all(|path| path.exists()));
        assert_eq!(paths.len(), runtime.libraries().len());
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    #[test]
    fn windows_runtime_prefers_vulkan_when_zluda_is_enabled() {
        let runtime = Runtime::new("/tmp/koharu-runtime", crate::ComputePolicy::PreferGpu).unwrap();
        if crate::zluda::package_enabled(&runtime) {
            assert_eq!(
                LlamaDistribution::detect(&runtime).unwrap(),
                LlamaDistribution::WindowsVulkanX64
            );
        }
    }
}
