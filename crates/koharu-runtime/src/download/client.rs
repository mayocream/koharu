use std::{io::SeekFrom, path::PathBuf};

use futures::stream::{self, StreamExt, TryStreamExt};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

static USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const CHUNK_SIZE: u64 = 10 * 1024 * 1024;

pub struct Client {
    inner: reqwest_middleware::ClientWithMiddleware,
}

impl Client {
    // TODO: config
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(std::time::Duration::from_secs(60))
            .read_timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("failed to build reqwest client");

        let middleware = reqwest_middleware::ClientBuilder::new(client)
            .with(reqwest_retry::RetryTransientMiddleware::new_with_policy(
                reqwest_retry::policies::ExponentialBackoff::builder().build_with_max_retries(3),
            ))
            .build();

        Self { inner: middleware }
    }

    pub fn get(&self, url: &str) -> reqwest_middleware::RequestBuilder {
        self.inner.get(url)
    }

    /// Downloads a file from the given URL to the specified destination path.
    pub async fn download(&self, url: &str, path: PathBuf) -> anyhow::Result<PathBuf> {
        let content_length = self.content_length(url).await?;

        tokio::fs::File::create(&path)
            .await?
            .set_len(content_length)
            .await?;

        let chunks = (0..content_length)
            .step_by(CHUNK_SIZE as usize)
            .map(|start| {
                let end = start.saturating_add(CHUNK_SIZE).min(content_length) - 1;
                (start, end)
            });

        let result: anyhow::Result<Vec<()>> = stream::iter(chunks)
            .map(|(start, end)| self.chunk(url, path.clone(), start, end))
            .buffer_unordered(num_cpus::get())
            .try_collect()
            .await;

        if let Err(error) = result {
            tokio::fs::remove_file(&path).await.ok();
            return Err(error);
        }

        Ok(path)
    }

    /// Returns the content length of the file at the given URL.
    /// Returns an error if the server does not provide a Content-Length header.
    pub async fn content_length(&self, url: &str) -> anyhow::Result<u64> {
        let response = self.inner.head(url).send().await?.error_for_status()?;

        let content_length = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .ok_or_else(|| anyhow::anyhow!("missing Content-Length for `{url}`"))?
            .to_str()?;
        Ok(content_length.trim().parse::<u64>()?)
    }

    async fn chunk(&self, url: &str, path: PathBuf, start: u64, end: u64) -> anyhow::Result<()> {
        let response = self
            .inner
            .get(url)
            .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
            .send()
            .await?
            .error_for_status()?;

        let bytes = response.bytes().await?;
        if bytes.len() != (end - start + 1) as usize {
            anyhow::bail!(
                "range {start}-{end} for `{url}` returned {} bytes",
                bytes.len()
            );
        }

        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .await?;
        file.seek(SeekFrom::Start(start)).await?;
        Ok(file.write_all(&bytes).await?)
    }
}
