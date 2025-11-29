pub mod comic_text_detector;
pub mod lama;
pub mod llm;
pub mod manga_ocr;

use std::path::PathBuf;

use anyhow::Result;
use candle_core::{
    Device,
    utils::{cuda_is_available, metal_is_available},
};
use hf_hub::{Cache, Repo, api::tokio::ApiBuilder};
use tracing::info;

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
    let api = ApiBuilder::new().high().build()?;
    let hf_repo = Repo::new(repo.as_ref().to_string(), hf_hub::RepoType::Model);
    let filename = filename.as_ref();

    // hit the cache first
    if let Some(path) = Cache::default().repo(hf_repo.clone()).get(filename) {
        return Ok(path);
    }

    info!(
        "downloading {filename} from Hugging Face Hub repo {}",
        repo.as_ref()
    );

    let path = api.repo(hf_repo).download(filename).await?;
    Ok(path)
}
