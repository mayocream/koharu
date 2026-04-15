#[cfg(target_os = "windows")]
const RELEASE_BASE_URL: &str = "https://github.com/vosen/ZLUDA/releases/download";
#[cfg(any(target_os = "windows", test))]
const RELEASE_TAG: &str = "v6-preview.64";
#[cfg(any(target_os = "windows", test))]
const ZLUDA_ASSET_NAME: &str = "zluda-windows-8251f1e.zip";
#[cfg(any(target_os = "windows", test))]
// Bump this when extraction behavior changes but the upstream asset name stays the same.
const ZLUDA_EXTRACT_REVISION: u32 = 3;
#[cfg(any(target_os = "windows", test))]
const ZLUDA_DLLS: &[&str] = &[
    "nvcudart_hybrid64.dll",
    "nvcuda.dll",
    "cublasLt64_13.dll",
    "cublas64_13.dll",
    "cufft64_12.dll",
];

#[cfg(target_os = "windows")]
mod platform {
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result, anyhow};

    use crate::Runtime;
    use crate::archive::{self, ArchiveKind, ExtractPolicy};
    use crate::install::InstallState;
    use crate::loader::{add_runtime_search_path, preload_library};

    use super::{RELEASE_BASE_URL, RELEASE_TAG, ZLUDA_ASSET_NAME, ZLUDA_DLLS, source_id};

    const HIP_ROOT_CANDIDATES: &[&str] = &[
        r"C:\hip_sdk",
        r"C:\Program Files\AMD\ROCm",
        r"C:\Program Files\AMD\ROCm\7.1",
        r"C:\Program Files\AMD\ROCm\7.0",
        r"C:\Program Files\AMD\ROCm\6.4",
        r"C:\Program Files\AMD\ROCm\6.3",
        r"C:\Program Files\AMD\ROCm\6.2",
        r"C:\Program Files\AMD\ROCm\6.1",
        r"C:\Program Files\AMD\ROCm\6.0",
    ];
    const HIP_RUNTIME_DLLS: &[&str] = &["amdhip64_7.dll", "amdhip64_6.dll"];
    const HIP_SDK_DOWNLOAD_URL: &str =
        "https://www.amd.com/en/developer/resources/rocm-hub/hip-sdk.html";

    pub(crate) fn package_enabled(runtime: &Runtime) -> bool {
        runtime.wants_gpu() && hip_root_dir().is_some() && !crate::cuda::package_enabled(runtime)
    }

    pub(crate) fn package_present(runtime: &Runtime) -> Result<bool> {
        let install_dir = install_dir(runtime);
        let source_id = source_id();
        let install = InstallState::new(&install_dir, &source_id);
        Ok(install.is_current() && ZLUDA_DLLS.iter().all(|dll| install_dir.join(dll).exists()))
    }

    pub(crate) async fn package_prepare(runtime: &Runtime) -> Result<()> {
        if let Err(err) = ensure_ready(runtime).await {
            tracing::warn!(
                "ZLUDA runtime is unavailable: {err:#}; falling back to CPU for unsupported Candle models."
            );
        }
        Ok(())
    }

    async fn ensure_ready(runtime: &Runtime) -> Result<()> {
        let install_dir = install_dir(runtime);
        install_if_needed(runtime, &install_dir).await?;

        let hip_root = hip_root_dir().ok_or_else(hip_sdk_missing_error)?;
        let hip_bin = hip_root.join("bin");
        // ZLUDA consults HIP_PATH directly when probing HIP performance libraries.
        unsafe { std::env::set_var("HIP_PATH", &hip_root) };
        add_runtime_search_path(&hip_bin)?;
        add_runtime_search_path(&install_dir)?;

        for dll in ZLUDA_DLLS {
            preload_library(&install_dir.join(dll))?;
        }

        tracing::info!("Experimental ZLUDA {RELEASE_TAG} support enabled");
        Ok(())
    }

    async fn install_if_needed(runtime: &Runtime, install_dir: &Path) -> Result<()> {
        let source_id = source_id();
        let install = InstallState::new(install_dir, &source_id);
        if install.is_current() && ZLUDA_DLLS.iter().all(|dll| install_dir.join(dll).exists()) {
            return Ok(());
        }

        install.reset()?;

        let url = format!("{RELEASE_BASE_URL}/{RELEASE_TAG}/{ZLUDA_ASSET_NAME}");
        let archive = runtime
            .downloads()
            .cached_download(&url, ZLUDA_ASSET_NAME)
            .await
            .with_context(|| format!("failed to download `{url}`"))?;
        archive::extract(
            &archive,
            install_dir,
            ArchiveKind::Zip,
            ExtractPolicy::Selected(ZLUDA_DLLS),
        )?;

        install.commit()
    }

    fn install_dir(runtime: &Runtime) -> PathBuf {
        runtime.root().join("runtime").join("zluda")
    }

    fn hip_root_dir() -> Option<PathBuf> {
        std::env::var_os("HIP_PATH")
            .map(PathBuf::from)
            .into_iter()
            .chain(HIP_ROOT_CANDIDATES.iter().map(PathBuf::from))
            .into_iter()
            .find(|dir| {
                HIP_RUNTIME_DLLS
                    .iter()
                    .any(|dll| dir.join("bin").join(dll).exists())
            })
    }

    fn hip_sdk_missing_error() -> anyhow::Error {
        anyhow!(
            "HIP SDK not found. Set `HIP_PATH` or install the AMD HIP SDK from `{HIP_SDK_DOWNLOAD_URL}`."
        )
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use anyhow::Result;

    use crate::Runtime;

    pub(crate) fn package_enabled(_: &Runtime) -> bool {
        false
    }

    pub(crate) fn package_present(_: &Runtime) -> Result<bool> {
        Ok(false)
    }

    pub(crate) async fn package_prepare(_: &Runtime) -> Result<()> {
        Ok(())
    }
}

pub(crate) use platform::{package_enabled, package_prepare, package_present};

#[cfg(any(target_os = "windows", test))]
fn source_id() -> String {
    format!("zluda;tag={RELEASE_TAG};asset={ZLUDA_ASSET_NAME};extract={ZLUDA_EXTRACT_REVISION}")
}

crate::declare_native_package!(
    id: "runtime:zluda",
    bootstrap: true,
    order: 30,
    enabled: crate::zluda::package_enabled,
    present: crate::zluda::package_present,
    prepare: crate::zluda::package_prepare,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_id_mentions_release_asset() {
        let id = source_id();
        assert!(id.contains("zluda"));
        assert!(id.contains(ZLUDA_ASSET_NAME));
    }

    #[test]
    fn required_runtime_dlls_cover_zluda_cuda_entrypoints() {
        assert!(ZLUDA_DLLS.contains(&"nvcuda.dll"));
        assert!(ZLUDA_DLLS.contains(&"cublas64_13.dll"));
        assert!(ZLUDA_DLLS.contains(&"cublasLt64_13.dll"));
        assert!(ZLUDA_DLLS.contains(&"cufft64_12.dll"));
    }

    #[test]
    fn preload_order_keeps_driver_before_math_libraries() {
        let nvcuda_index = ZLUDA_DLLS
            .iter()
            .position(|dll| *dll == "nvcuda.dll")
            .unwrap();
        let cublas_index = ZLUDA_DLLS
            .iter()
            .position(|dll| *dll == "cublas64_13.dll")
            .unwrap();
        let cufft_index = ZLUDA_DLLS
            .iter()
            .position(|dll| *dll == "cufft64_12.dll")
            .unwrap();
        assert!(nvcuda_index < cublas_index);
        assert!(nvcuda_index < cufft_index);
    }

    #[test]
    fn runtime_extract_list_matches_preload_list() {
        assert_eq!(ZLUDA_DLLS.len(), 5);
        assert!(ZLUDA_DLLS.iter().all(|dll| dll.ends_with(".dll")));
    }
}
