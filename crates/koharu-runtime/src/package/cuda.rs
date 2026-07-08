use std::{fs::create_dir_all, path::PathBuf, sync::LazyLock};

use strum::EnumProperty;

use crate::{
    download::{
        archive::extract,
        client::Client,
        pypi::{Platform, wheel},
    },
    package::{Package, PreloadablePackage, STORE_DIR, loading::preload},
};

static CUDA_DIR: LazyLock<PathBuf> = LazyLock::new(|| STORE_DIR.join("cuda"));

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString, strum::Display, strum::EnumProperty,
)]
pub enum Cuda {
    #[strum(props(
        package = "nvidia-cuda-runtime-cu12/12.9.79",
        windows_dylibs = "cudart64_12.dll",
        linux_dylibs = "libcudart.so.12"
    ))]
    Runtime,
    #[strum(props(
        package = "nvidia-cublas-cu12/12.9.2.10",
        windows_dylibs = "cublasLt64_12.dll,cublas64_12.dll",
        linux_dylibs = "libcublasLt.so.12,libcublas.so.12"
    ))]
    Cublas,
    #[strum(props(
        package = "nvidia-cufft-cu12/11.4.1.4",
        windows_dylibs = "cufft64_11.dll",
        linux_dylibs = "libcufft.so.11"
    ))]
    Cufft,
    #[strum(props(
        package = "nvidia-curand-cu12/10.3.10.19",
        windows_dylibs = "curand64_10.dll",
        linux_dylibs = "libcurand.so.10"
    ))]
    Curand,
    #[strum(props(
        package = "nvidia-cudnn-cu12/9.24.0.43",
        windows_dylibs = "cudnn64_9.dll,cudnn_adv64_9.dll,cudnn_cnn64_9.dll,cudnn_engines_precompiled64_9.dll,cudnn_engines_runtime_compiled64_9.dll,cudnn_engines_tensor_ir64_9.dll,cudnn_graph64_9.dll,cudnn_heuristic64_9.dll,cudnn_ops64_9.dll",
        linux_dylibs = "libcudnn.so.9,libcudnn_adv.so.9,libcudnn_cnn.so.9,libcudnn_engines_precompiled.so.9,libcudnn_engines_runtime_compiled.so.9,libcudnn_engines_tensor_ir.so.9,libcudnn_graph.so.9,libcudnn_heuristic.so.9,libcudnn_ops.so.9"
    ))]
    Cudnn,
}

impl Cuda {
    pub fn package(&self) -> &'static str {
        self.get_str("package")
            .expect("package property 'package' not found")
    }

    pub fn windows_dylibs(&self) -> Vec<&str> {
        self.get_str("windows_dylibs")
            .expect("package property 'windows_dylibs' not found")
            .split(',')
            .collect()
    }

    pub fn linux_dylibs(&self) -> Vec<&str> {
        self.get_str("linux_dylibs")
            .expect("package property 'linux_dylibs' not found")
            .split(',')
            .collect()
    }

    pub fn dylibs(&self) -> Vec<&str> {
        if cfg!(target_os = "windows") {
            self.windows_dylibs()
        } else if cfg!(target_os = "linux") {
            self.linux_dylibs()
        } else {
            panic!("Unsupported OS");
        }
    }
}

#[async_trait::async_trait]
impl Package for Cuda {
    async fn resolve(&self) -> anyhow::Result<PathBuf> {
        let path = CUDA_DIR.join(self.package().replace("/", "--"));
        // if dylibs already exist, return the path
        if self.dylibs().iter().all(|dylib| path.join(dylib).exists()) {
            return Ok(path);
        }

        let platform =
            Platform::current().ok_or_else(|| anyhow::anyhow!("Unsupported platform"))?;
        let wheel = wheel(self.package(), platform).await?;

        let file = tempfile::Builder::new().suffix(".zip").tempfile()?;
        let client = Client::new();
        let archive = client.download(&wheel, file.path().to_path_buf()).await?;

        create_dir_all(&path)?;
        // extract only the dynamic libraries
        extract(archive, path.clone(), &["**/*.dll", "**/*.so.*"])?;

        Ok(path)
    }
}

#[async_trait::async_trait]
impl PreloadablePackage for Cuda {
    async fn preload(&self) -> anyhow::Result<()> {
        let path = self.resolve().await?;
        for dylib in self.dylibs() {
            let dylib_path = path.join(dylib);
            preload(dylib_path)?;
        }

        Ok(())
    }
}
