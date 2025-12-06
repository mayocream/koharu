pub mod comic_text_detector;
pub mod lama;
pub mod llm;
pub mod manga_ocr;

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::Result;
use candle_core::{
    Device,
    utils::{cuda_is_available, metal_is_available},
};
use hf_hub::{Cache, Repo, api::tokio::ApiBuilder};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tracing::info;

const HF_MIRROS: &[&str] = &["https://huggingface.co", "https://hf-mirror.com"];

static HF_ENDPOINT: once_cell::sync::Lazy<String> = once_cell::sync::Lazy::new(|| {
    HF_MIRROS
        .par_iter()
        .map(|endpoint| {
            let start = Instant::now();
            let resp = reqwest::blocking::get(*endpoint);
            match resp {
                Ok(resp) if resp.status().is_success() => {
                    let duration = start.elapsed();
                    (duration, (*endpoint).to_string())
                }
                _ => (Duration::MAX, (*endpoint).to_string()),
            }
        })
        .min_by_key(|(duration, _)| *duration)
        .map(|(_, endpoint)| endpoint)
        .unwrap_or_else(|| HF_MIRROS[0].to_string())
});

static APP_ROOT: once_cell::sync::Lazy<PathBuf> = once_cell::sync::Lazy::new(|| {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
});

pub fn device(cpu: bool) -> Result<Device> {
    if cpu {
        Ok(Device::Cpu)
    } else if cuda_is_available() {
        Ok(Device::new_cuda(0)?)
    } else if metal_is_available() {
        Ok(Device::new_metal(0)?)
    } else {
        println!("CUDA and Metal are not available. Using CPU device.");
        Ok(Device::Cpu)
    }
}

pub async fn hf_hub(repo: impl AsRef<str>, filename: impl AsRef<str>) -> anyhow::Result<PathBuf> {
    let cache =  if APP_ROOT.join(".portable").exists() {
        Cache::new(APP_ROOT.join("models"))
    } else {
        Cache::default()
    };

    let api = ApiBuilder::new()
        .with_endpoint(HF_ENDPOINT.to_string())
        .with_cache_dir((*cache.path()).clone())
        .high()
        .build()?;
    
    let hf_repo = Repo::new(repo.as_ref().to_string(), hf_hub::RepoType::Model);
    let filename = filename.as_ref();

    tracing::info!("Models directory: {:?}", cache.path());
    // hit the cache first
    if let Some(path) = cache.repo(hf_repo.clone()).get(filename) {
        return Ok(path);
    }

    info!(
        "downloading {filename} from Hugging Face Hub repo {}",
        repo.as_ref()
    );

    let path = api.repo(hf_repo).download(filename).await?;
    Ok(path)
}
