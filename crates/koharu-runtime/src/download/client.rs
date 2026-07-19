use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::stream::{self, StreamExt, TryStreamExt};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use super::{
    event::{self, Event},
    progress,
};
use crate::config::HttpConfig;

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const CHUNK_SIZE: u64 = 10 * 1024 * 1024;

pub type HttpClient = Arc<reqwest_middleware::ClientWithMiddleware>;

static HTTP_CLIENT: Mutex<Option<VersionedClient>> = Mutex::new(None);

struct VersionedClient {
    revision: koharu_config::ConfigRevision,
    client: HttpClient,
}

fn resolve(
    state: &Mutex<Option<VersionedClient>>,
    config: &koharu_config::Config<HttpConfig>,
) -> anyhow::Result<HttpClient> {
    let value = config.read()?;
    let revision = config.revision();
    let mut state = state
        .lock()
        .map_err(|_| anyhow::anyhow!("shared HTTP client lock is poisoned"))?;

    if let Some(current) = state.as_ref()
        && current.revision == revision
    {
        return Ok(current.client.clone());
    }

    let client = build(&value)?;
    *state = Some(VersionedClient {
        revision,
        client: client.clone(),
    });
    Ok(client)
}

/// Return the process-wide HTTP client for the latest HTTP configuration.
///
/// Calls at the same configuration revision return the same `Arc`. The first
/// call after a configuration change replaces it with a newly built client.
pub fn shared() -> anyhow::Result<HttpClient> {
    let config = HttpConfig::load()?;
    resolve(&HTTP_CLIENT, &config)
}

fn build(config: &HttpConfig) -> anyhow::Result<HttpClient> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(config.connect_timeout_secs.max(1)))
        .read_timeout(Duration::from_secs(config.read_timeout_secs.max(1)))
        .build()?;

    Ok(Arc::new(
        reqwest_middleware::ClientBuilder::new(client)
            .with(reqwest_retry::RetryTransientMiddleware::new_with_policy(
                reqwest_retry::policies::ExponentialBackoff::builder()
                    .build_with_max_retries(config.max_retries),
            ))
            .build(),
    ))
}

pub struct Client {
    inner: HttpClient,
}

impl Client {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self { inner: shared()? })
    }

    pub fn get(&self, url: &str) -> reqwest_middleware::RequestBuilder {
        self.inner.get(url)
    }

    /// Downloads a file from the given URL to the specified destination path.
    pub async fn download(&self, url: &str, path: PathBuf) -> anyhow::Result<PathBuf> {
        let id = event::next_id();
        let progress = progress::new(url);
        let name = progress.message();
        event::publish(Event::Started {
            id,
            name: name.clone(),
        });
        let result: anyhow::Result<()> = async {
            let content_length = self.content_length(url).await?;
            progress.set_length(content_length);
            event::publish(Event::Progress {
                id,
                name: name.clone(),
                completed: 0,
                total: content_length,
            });

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

            stream::iter(chunks)
                .map(|(start, end)| {
                    self.chunk(id, &name, url, path.clone(), start, end, progress.clone())
                })
                .buffer_unordered(num_cpus::get())
                .try_collect::<Vec<()>>()
                .await?;
            Ok(())
        }
        .await;

        if let Err(error) = result {
            let message = format!("{} failed", progress.message());
            progress.abandon_with_message(message);
            tokio::fs::remove_file(&path).await.ok();
            event::publish(Event::Failed {
                id,
                name,
                error: error.to_string(),
            });
            return Err(error);
        }

        let message = format!("{} downloaded", progress.message());
        progress.finish_with_message(message);
        event::publish(Event::Finished { id });
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

    async fn chunk(
        &self,
        id: u64,
        name: &str,
        url: &str,
        path: PathBuf,
        start: u64,
        end: u64,
        progress: indicatif::ProgressBar,
    ) -> anyhow::Result<()> {
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
        file.write_all(&bytes).await?;
        progress.inc(bytes.len() as u64);
        event::publish(Event::Progress {
            id,
            name: name.to_owned(),
            completed: progress.position(),
            total: progress.length().unwrap_or_default(),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuses_one_client_for_a_configuration_revision() {
        let config = koharu_config::Config::memory(HttpConfig::default());
        let state = Mutex::new(None);

        let first = resolve(&state, &config).unwrap();
        let second = resolve(&state, &config).unwrap();

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn replaces_the_client_after_configuration_changes() {
        let config = koharu_config::Config::memory(HttpConfig::default());
        let state = Mutex::new(None);
        let first = resolve(&state, &config).unwrap();

        config.write().unwrap().read_timeout_secs = 30;
        let second = resolve(&state, &config).unwrap();
        let third = resolve(&state, &config).unwrap();

        assert!(!Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&second, &third));
    }
}
