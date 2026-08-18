use std::{
    path::Path,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use backon::{ExponentialBuilder, Retryable};
use futures::{StreamExt, TryStreamExt, stream};
use reqwest::{StatusCode, header};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncSeekExt, AsyncWriteExt, BufWriter},
    sync::broadcast,
};

use crate::network::{DownloadClient, download_client};

const EVENT_CAPACITY: usize = 256;
const PART_SIZE: u64 = 4 * 1024 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);
const RANGE_RETRIES: usize = 5;
const RANGE_RETRY_MAX_DELAY: Duration = Duration::from_secs(2);
const RANGE_RETRY_MIN_DELAY: Duration = Duration::from_millis(250);
const WRITE_BUFFER_SIZE: usize = 256 * 1024;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static EVENTS: LazyLock<broadcast::Sender<Event>> =
    LazyLock::new(|| broadcast::channel(EVENT_CAPACITY).0);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Event {
    Started {
        id: u64,
        name: String,
    },
    Progress {
        id: u64,
        name: String,
        completed: u64,
        total: u64,
    },
    Finished {
        id: u64,
    },
    Failed {
        id: u64,
        name: String,
        error: String,
    },
}

#[must_use]
pub fn subscribe() -> broadcast::Receiver<Event> {
    EVENTS.subscribe()
}

pub(crate) struct Transfer {
    client: DownloadClient,
}

impl Transfer {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            client: download_client()?,
        })
    }

    pub(crate) fn get(&self, url: &str) -> reqwest_middleware::RequestBuilder {
        self.client.get(url)
    }

    pub(crate) async fn fetch(&self, url: &str, destination: &Path) -> Result<()> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let name = display_name(url);
        publish(Event::Started {
            id,
            name: name.clone(),
        });

        let result = self.fetch_inner(id, &name, url, destination).await;
        match result {
            Ok(()) => {
                publish(Event::Finished { id });
                Ok(())
            }
            Err(error) => {
                let _ = tokio::fs::remove_file(destination).await;
                publish(Event::Failed {
                    id,
                    name,
                    error: error.to_string(),
                });
                Err(error)
            }
        }
    }

    async fn fetch_inner(&self, id: u64, name: &str, url: &str, destination: &Path) -> Result<()> {
        let response = self
            .client
            .get(url)
            .header(header::RANGE, "bytes=0-0")
            .header(header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            .with_context(|| format!("failed to inspect {url}"))?;
        if response.status() == StatusCode::PARTIAL_CONTENT {
            let content_range = response
                .headers()
                .get(header::CONTENT_RANGE)
                .context("range probe omitted Content-Range")?
                .to_str()?;
            let (range, total) = content_range
                .split_once('/')
                .context("range probe returned invalid Content-Range")?;
            ensure!(
                range == "bytes 0-0",
                "{url} returned Content-Range {content_range} for the range probe"
            );
            let total = total
                .parse::<u64>()
                .with_context(|| format!("{url} returned invalid size {total}"))?;
            ensure!(total > 0, "{url} returned an empty byte range");
            return self.fetch_parts(id, name, url, destination, total).await;
        }
        if response.status() == StatusCode::RANGE_NOT_SATISFIABLE
            && response
                .headers()
                .get(header::CONTENT_RANGE)
                .is_some_and(|value| value == "bytes */0")
        {
            tokio::fs::File::create(destination).await?;
            return Ok(());
        }

        let response = response
            .error_for_status()
            .with_context(|| format!("failed to inspect {url}"))?;
        fetch_stream(id, name, url, destination, response).await
    }

    async fn fetch_parts(
        &self,
        id: u64,
        name: &str,
        url: &str,
        destination: &Path,
        total: u64,
    ) -> Result<()> {
        tokio::fs::File::create(destination)
            .await?
            .set_len(total)
            .await?;

        let transfer = Arc::new(PartTransfer {
            url: Arc::from(url),
            destination: Arc::from(destination),
            name: Arc::from(name),
            id,
            total,
            completed: AtomicU64::new(0),
        });
        let parts = (0..total).step_by(PART_SIZE as usize).map(|start| Part {
            start,
            end: (start + PART_SIZE).min(total) - 1,
        });

        stream::iter(parts)
            .map(|part| fetch_with_retry(self.client.clone(), Arc::clone(&transfer), part))
            .buffer_unordered(num_cpus::get().saturating_mul(4).clamp(16, 64))
            .try_collect::<()>()
            .await?;
        ensure!(
            transfer.completed.load(Ordering::Relaxed) == total,
            "{url} download was incomplete"
        );
        Ok(())
    }
}

struct PartTransfer {
    url: Arc<str>,
    destination: Arc<Path>,
    name: Arc<str>,
    id: u64,
    total: u64,
    completed: AtomicU64,
}

impl PartTransfer {
    fn advance(&self, bytes: u64) {
        let completed = self.completed.fetch_add(bytes, Ordering::Relaxed) + bytes;
        publish_progress(self.id, &self.name, completed, self.total);
    }
}

struct Part {
    start: u64,
    end: u64,
}

impl Part {
    const fn len(&self) -> u64 {
        self.end - self.start + 1
    }
}

async fn fetch_stream(
    id: u64,
    name: &str,
    url: &str,
    destination: &Path,
    response: reqwest::Response,
) -> Result<()> {
    let total = response.content_length().unwrap_or(0);
    let file = tokio::fs::File::create(destination).await?;
    let mut file = BufWriter::with_capacity(WRITE_BUFFER_SIZE, file);
    let mut body = response.bytes_stream();
    let mut completed = 0;
    let mut reported = 0;
    let mut last_report = Instant::now();

    while let Some(bytes) = body.try_next().await? {
        file.write_all(&bytes).await?;
        completed += bytes.len() as u64;
        if last_report.elapsed() >= PROGRESS_INTERVAL {
            publish_progress(id, name, completed, total);
            reported = completed;
            last_report = Instant::now();
        }
    }
    file.flush().await?;
    if reported != completed {
        publish_progress(id, name, completed, total);
    }
    if total > 0 {
        ensure!(
            completed == total,
            "{url} ended after {completed} of {total} bytes"
        );
    }
    Ok(())
}

async fn fetch_with_retry(
    client: DownloadClient,
    transfer: Arc<PartTransfer>,
    part: Part,
) -> Result<()> {
    let start = part.start;
    let end = part.end;
    (|| fetch_range(&client, &transfer, &part))
        .retry(
            ExponentialBuilder::default()
                .with_min_delay(RANGE_RETRY_MIN_DELAY)
                .with_max_delay(RANGE_RETRY_MAX_DELAY)
                .with_max_times(RANGE_RETRIES)
                .with_jitter(),
        )
        .when(|error| {
            error.chain().any(|cause| {
                cause
                    .downcast_ref::<reqwest::Error>()
                    .is_some_and(|error| error.is_body() || error.is_decode())
            })
        })
        .notify(|error, delay| {
            tracing::warn!(
                start,
                end,
                ?delay,
                %error,
                "retrying interrupted download range"
            );
        })
        .await
        .with_context(|| format!("failed to download byte range {start}-{end}"))?;
    transfer.advance(part.len());
    Ok(())
}

async fn fetch_range(client: &DownloadClient, transfer: &PartTransfer, part: &Part) -> Result<()> {
    let response = client
        .get(transfer.url.as_ref())
        .header(header::RANGE, format!("bytes={}-{}", part.start, part.end))
        .header(header::ACCEPT_ENCODING, "identity")
        .send()
        .await?;
    ensure!(
        response.status() == StatusCode::PARTIAL_CONTENT,
        "{} did not honor byte range {}-{}",
        transfer.url,
        part.start,
        part.end
    );
    let actual_range = response
        .headers()
        .get(header::CONTENT_RANGE)
        .context("partial response omitted Content-Range")?
        .to_str()?;
    let expected_range = format!("bytes {}-{}/{}", part.start, part.end, transfer.total);
    ensure!(
        actual_range == expected_range,
        "{} returned Content-Range {actual_range}, expected {expected_range}",
        transfer.url
    );
    let expected = part.len();
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(transfer.destination.as_ref())
        .await?;
    file.seek(std::io::SeekFrom::Start(part.start)).await?;
    let mut file = BufWriter::with_capacity(WRITE_BUFFER_SIZE, file);
    let mut received = 0;
    let mut body = response.bytes_stream();
    while let Some(bytes) = body.try_next().await? {
        received += bytes.len() as u64;
        ensure!(
            received <= expected,
            "{} returned more than {expected} bytes for range {}-{}",
            transfer.url,
            part.start,
            part.end
        );
        file.write_all(&bytes).await?;
    }
    file.flush().await?;
    ensure!(
        received == expected,
        "{} returned {received} bytes for a {expected}-byte range",
        transfer.url
    );
    Ok(())
}

fn publish_progress(id: u64, name: &str, completed: u64, total: u64) {
    publish(Event::Progress {
        id,
        name: name.to_owned(),
        completed,
        total,
    });
}

fn publish(event: Event) {
    let _ = EVENTS.send(event);
}

fn display_name(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "download".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[test]
    fn names_downloads_from_the_url_path() {
        assert_eq!(
            display_name("https://example.test/a/model.bin?q=1"),
            "model.bin"
        );
        assert_eq!(display_name("https://example.test/"), "download");
    }

    #[test]
    fn subscribers_observe_events() {
        let mut receiver = subscribe();
        let event = Event::Finished { id: 42 };
        publish(event.clone());
        assert_eq!(receiver.try_recv().unwrap(), event);
    }

    #[tokio::test]
    async fn retries_interrupted_range_bodies() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let bodies: [&[u8]; 2] = [b"abcd", b"abcdefgh"];
            for body in bodies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let mut buffer = [0; 1024];
                    let read = socket.read(&mut buffer).await.unwrap();
                    assert!(read > 0);
                    request.extend_from_slice(&buffer[..read]);
                }
                assert!(
                    String::from_utf8_lossy(&request)
                        .to_ascii_lowercase()
                        .contains("range: bytes=0-7")
                );

                socket
                    .write_all(
                        b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-7/8\r\nContent-Length: 8\r\nConnection: close\r\n\r\n",
                    )
                    .await
                    .unwrap();
                socket.write_all(body).await.unwrap();
                socket.shutdown().await.unwrap();
            }
        });

        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("download");
        tokio::fs::File::create(&destination)
            .await
            .unwrap()
            .set_len(8)
            .await
            .unwrap();
        let transfer = Arc::new(PartTransfer {
            url: Arc::from(format!("http://{address}/artifact")),
            destination: Arc::from(destination.as_path()),
            name: Arc::from("artifact"),
            id: 1,
            total: 8,
            completed: AtomicU64::new(0),
        });
        let client =
            Arc::new(reqwest_middleware::ClientBuilder::new(reqwest::Client::new()).build());

        fetch_with_retry(client, Arc::clone(&transfer), Part { start: 0, end: 7 })
            .await
            .unwrap();
        server.await.unwrap();

        assert_eq!(tokio::fs::read(destination).await.unwrap(), b"abcdefgh");
        assert_eq!(transfer.completed.load(Ordering::Relaxed), 8);
    }
}
