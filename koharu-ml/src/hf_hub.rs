use std::path::PathBuf;

use anyhow::Context;
use hf_hub::{
    Cache, Repo,
    api::tokio::{Api, ApiBuilder},
};
use koharu_core::progress::progress_bar;
use once_cell::sync::{Lazy, OnceCell};

static CACHE_DIR: OnceCell<PathBuf> = OnceCell::new();

static HF_API: Lazy<Api> = Lazy::new(|| {
    ApiBuilder::new()
        .with_cache_dir(get_cache_dir().to_path_buf())
        .high()
        .build()
        .expect("build HF API client")
});
static HF_CACHE: Lazy<Cache> = Lazy::new(|| Cache::new(get_cache_dir().to_path_buf()));

fn get_cache_dir() -> &'static PathBuf {
    CACHE_DIR.get_or_init(|| {
        dirs::cache_dir()
            .unwrap_or_default()
            .join("Koharu")
            .join("models")
    })
}

pub fn set_cache_dir(path: PathBuf) -> anyhow::Result<()> {
    CACHE_DIR
        .set(path)
        .map_err(|_| anyhow::anyhow!("cache dir has already been set"))
}

pub async fn hf_download(repo: &str, filename: &str) -> anyhow::Result<PathBuf> {
    let hf_repo = Repo::model(repo.to_string());
    if let Some(path) = HF_CACHE.repo(hf_repo.clone()).get(filename) {
        return Ok(path);
    }

    let span = tracing::info_span!("hf_download", repo, filename);
    let _enter = span.enter();

    let pb = progress_bar(filename);
    let path = HF_API
        .repo(hf_repo)
        .download_with_progress(filename, pb)
        .await
        .context("failed to download from HF Hub")?;

    Ok(path)
}

#[macro_export]
macro_rules! define_models {
    ($($variant:ident => ($repo:literal, $filename:literal)),* $(,)?) => {
        #[derive(Debug, Clone, strum::EnumIter, strum::EnumProperty)]
        pub enum Manifest {
            $(
                #[strum(props(repo = $repo, filename = $filename))]
                $variant,
            )*
        }

        impl Manifest {
            pub async fn get(&self) -> anyhow::Result<std::path::PathBuf> {
                use strum::EnumProperty;
                use $crate::hf_hub::hf_download;
                let repo = self.get_str("repo").expect("repo property");
                let filename = self.get_str("filename").expect("filename property");
                hf_download(repo, filename).await
            }
        }

        #[allow(unused)]
        pub async fn prefetch() -> anyhow::Result<()> {
            use futures::stream::{self, StreamExt, TryStreamExt};
            let manifests = [
                $(Manifest::$variant),*
            ];
            stream::iter(manifests)
                .map(|manifest| async move {
                    manifest.get().await
                })
                .buffer_unordered(num_cpus::get())
                .try_collect::<Vec<_>>()
                .await?;
            Ok(())
        }
    };
}
