use std::path::PathBuf;

use hf_hub::{
    Cache, Repo,
    api::tokio::{Api, ApiBuilder},
};
use koharu_core::progress::progress_bar;
use once_cell::sync::{Lazy, OnceCell};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

static CACHE_DIR: OnceCell<PathBuf> = OnceCell::new();

static HF_API: Lazy<Api> = Lazy::new(|| {
    ApiBuilder::new()
        .with_endpoint(HF_ENDPOINT.to_string())
        .with_cache_dir(get_cache_dir().to_path_buf())
        .high()
        .build()
        .expect("build HF API client")
});
static HF_CACHE: Lazy<Cache> = Lazy::new(|| Cache::new(get_cache_dir().to_path_buf()));

// maybe we need to place hf-hub logic separately
const HF_MIRRORS: &[&str] = &["https://huggingface.co", "https://hf-mirror.com"];
static HF_ENDPOINT: once_cell::sync::Lazy<String> = once_cell::sync::Lazy::new(|| {
    HF_MIRRORS
        .par_iter()
        .filter_map(|endpoint| {
            reqwest::blocking::get(*endpoint)
                .ok()
                .filter(|resp| resp.status().is_success())
                .map(|_| *endpoint)
        })
        .max_by_key(|endpoint| *endpoint == HF_MIRRORS[0]) // prefer the official one
        .unwrap_or(HF_MIRRORS[0])
        .to_string()
});

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
        .await?;

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
    };
}
