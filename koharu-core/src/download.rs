use std::path::PathBuf;

use anyhow::Context;
use futures::StreamExt;
use hf_hub::{Cache, Repo, api::tokio::Api};
use reqwest::Client;

use crate::progress::Emitter;

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// Download arbitrary bytes over HTTP using reqwest's async client.
/// All progress updates are emitted via `tracing::event!` with the `koharu::download` target.
pub async fn http(url: impl Into<String>) -> anyhow::Result<Vec<u8>> {
    let url = url.into();
    let client = Client::builder().user_agent(USER_AGENT).build()?;

    let response = client.get(&url).send().await?.error_for_status()?;

    let total = response.content_length().unwrap_or(0) as usize;

    let mut emitter = Emitter::new(url.clone());
    emitter.begin(total);

    let mut bytes = Vec::with_capacity(total);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed to read a chunk from the response stream")?;
        emitter.advance(chunk.len());
        bytes.extend(&chunk);
    }

    emitter.complete();
    Ok(bytes)
}

/// Download a file from the Hugging Face Hub.
/// Returns the on-disk path managed by hf-hub's cache.
pub async fn hf_hub(repo: impl AsRef<str>, filename: impl AsRef<str>) -> anyhow::Result<PathBuf> {
    let api = Api::new()?;
    let repo = Repo::new(repo.as_ref().to_string(), hf_hub::RepoType::Model);
    let filename = filename.as_ref();

    // hit the cache first
    if let Some(path) = Cache::default().repo(repo.clone()).get(filename) {
        return Ok(path);
    }

    let repo = api.repo(repo);
    let url = repo.url(filename);
    let emitter = Emitter::new(url);

    let path = repo.download_with_progress(filename, emitter).await?;

    Ok(path)
}
