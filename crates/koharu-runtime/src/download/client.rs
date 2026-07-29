use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use futures::stream::{self, StreamExt, TryStreamExt};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};

use super::{
    event::{self, Event},
    progress,
};
use crate::config::HttpConfig;

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

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
        .http2_adaptive_window(true)
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

struct Context {
    id: u64,
    name: String,
    url: String,
    path: PathBuf,
    total: u64,
    progress: indicatif::ProgressBar,
    reported: AtomicU64,
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

            // Like HF Transfer, large artifacts use bounded concurrent range requests:
            // https://github.com/huggingface/hf_transfer/blob/3d370084b68729b4756003df41d232958a008f00/src/lib.rs#L151-L278
            let parallel_requests = std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(16)
                .clamp(16, 64);
            let target_chunks = parallel_requests as u64 * 8;
            let chunk_size = content_length
                .div_ceil(target_chunks)
                .div_ceil(8 * 1024 * 1024)
                .saturating_mul(8 * 1024 * 1024)
                .clamp(8 * 1024 * 1024, 64 * 1024 * 1024);
            let chunks = (0..content_length)
                .step_by(chunk_size as usize)
                .map(|start| {
                    let end = start.saturating_add(chunk_size).min(content_length) - 1;
                    (start, end)
                });
            let context = Arc::new(Context {
                id,
                name: name.clone(),
                url: url.to_owned(),
                path: path.clone(),
                total: content_length,
                progress: progress.clone(),
                reported: AtomicU64::new(0),
            });

            stream::iter(chunks)
                .map(|(start, end)| self.chunk(context.clone(), start, end))
                .buffer_unordered(parallel_requests)
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
        let response = self
            .inner
            .head(url)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await?
            .error_for_status()?;

        let content_length = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .ok_or_else(|| anyhow::anyhow!("missing Content-Length for `{url}`"))?
            .to_str()?;
        Ok(content_length.trim().parse::<u64>()?)
    }

    async fn chunk(&self, context: Arc<Context>, start: u64, end: u64) -> anyhow::Result<()> {
        let response = self
            .inner
            .get(&context.url)
            .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await?;
        if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            anyhow::bail!(
                "range {start}-{end} for `{}` returned {}, expected 206 Partial Content",
                context.url,
                response.status()
            );
        }
        let actual_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "range {start}-{end} for `{}` omitted Content-Range",
                    context.url
                )
            })?
            .to_str()?;
        let expected_range = format!("bytes {start}-{end}/{}", context.total);
        if actual_range != expected_range {
            anyhow::bail!(
                "range {start}-{end} for `{}` returned Content-Range `{actual_range}`, expected `{expected_range}`",
                context.url
            );
        }

        let expected = end - start + 1;

        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&context.path)
            .await?;
        file.seek(SeekFrom::Start(start)).await?;
        let mut writer = BufWriter::with_capacity(1024 * 1024, file);
        let mut received = 0u64;
        let mut body = response.bytes_stream();
        while let Some(bytes) = body.try_next().await? {
            let next = received.saturating_add(bytes.len() as u64);
            if next > expected {
                anyhow::bail!(
                    "range {start}-{end} for `{}` returned more than {expected} bytes",
                    context.url
                );
            }
            writer.write_all(&bytes).await?;
            received = next;
            context.progress.inc(bytes.len() as u64);
            let completed = context.progress.position();
            let mut previous = context.reported.load(Ordering::Relaxed);
            loop {
                if completed <= previous
                    || (completed != context.total && completed - previous < 8 * 1024 * 1024)
                {
                    break;
                }
                match context.reported.compare_exchange_weak(
                    previous,
                    completed,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        event::publish(Event::Progress {
                            id: context.id,
                            name: context.name.clone(),
                            completed,
                            total: context.total,
                        });
                        break;
                    }
                    Err(current) => previous = current,
                }
            }
        }
        writer.flush().await?;

        if received != expected {
            anyhow::bail!(
                "range {start}-{end} for `{}` returned {received} bytes, expected {expected}",
                context.url
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tokio::io::AsyncReadExt as _;

    async fn range_server(
        contents: Arc<Vec<u8>>,
    ) -> anyhow::Result<(String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let server_maximum = maximum.clone();
        let server = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let contents = contents.clone();
                let active = active.clone();
                let maximum = server_maximum.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 4096];
                    loop {
                        let Ok(read) = socket.read(&mut buffer).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }

                    let request = String::from_utf8_lossy(&request);
                    let Some(method) = request.split_whitespace().next() else {
                        return;
                    };
                    if method == "HEAD" {
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                            contents.len()
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        return;
                    }

                    let Some(range) = request.lines().find_map(|line| {
                        line.split_once(':').and_then(|(name, value)| {
                            name.eq_ignore_ascii_case("range").then_some(value.trim())
                        })
                    }) else {
                        return;
                    };
                    let Some((start, end)) = range
                        .strip_prefix("bytes=")
                        .and_then(|value| value.split_once('-'))
                        .and_then(|(start, end)| {
                            Some((start.parse::<usize>().ok()?, end.parse::<usize>().ok()?))
                        })
                    else {
                        return;
                    };
                    let current = active.fetch_add(1, Ordering::Relaxed) + 1;
                    maximum.fetch_max(current, Ordering::Relaxed);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    let body = &contents[start..=end];
                    let response = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                        body.len(),
                        contents.len()
                    );
                    if socket.write_all(response.as_bytes()).await.is_ok() {
                        let _ = socket.write_all(body).await;
                    }
                    active.fetch_sub(1, Ordering::Relaxed);
                });
            }
        });
        Ok((format!("http://{address}/model.bin"), maximum, server))
    }

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

    #[tokio::test]
    async fn streams_parallel_ranges_to_their_file_offsets() -> anyhow::Result<()> {
        let contents = Arc::new(
            (0..(8 * 1024 * 1024 * 2 + 123))
                .map(|index| (index % 251) as u8)
                .collect::<Vec<_>>(),
        );
        let (url, maximum, server) = range_server(contents.clone()).await?;
        let client = Client {
            inner: Arc::new(reqwest_middleware::ClientBuilder::new(reqwest::Client::new()).build()),
        };
        let temporary = tempfile::NamedTempFile::new()?;

        client
            .download(&url, temporary.path().to_path_buf())
            .await?;
        let downloaded = tokio::fs::read(temporary.path()).await?;
        server.abort();

        assert_eq!(downloaded, *contents);
        assert!(maximum.load(Ordering::Relaxed) >= 2);
        Ok(())
    }
}
