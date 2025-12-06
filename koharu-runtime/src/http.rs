use std::time::{
    Duration,
    Instant
};

use std::sync::Arc;
use anyhow::Context;
use futures::{
    StreamExt, future::join_all, stream::{self, TryStreamExt}
};
use once_cell::sync::Lazy;
use reqwest::header::{ACCEPT_RANGES, CONTENT_LENGTH, RANGE};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use tracing::info;

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const RANGE_CHUNK_SIZE_BYTES: usize = 16 * 1024 * 1024;

static HTTP_CLIENT: Lazy<ClientWithMiddleware> = Lazy::new(|| {
    ClientBuilder::new(
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("build reqwest client"),
    )
    .with(RetryTransientMiddleware::new_with_policy(
        ExponentialBackoff::builder().build_with_max_retries(3),
    ))
    .build()
});

pub fn http_client() -> &'static ClientWithMiddleware {
    &HTTP_CLIENT
}


/// Download a file using aggressive HTTP range requests with maximum concurrency.
/// The server must advertise `Accept-Ranges: bytes`; otherwise this call fails.
pub async fn http_download(url: &str) -> anyhow::Result<Vec<u8>> {
    let head = HTTP_CLIENT.head(url).send().await?.error_for_status().context(format!("cannot download {url}"))?;
    let headers = head.headers();
    let total_bytes = headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .context("missing Content-Length header")?;

    anyhow::ensure!(total_bytes > 0, "resource reports zero Content-Length");

    let supports_ranges = headers
        .get(ACCEPT_RANGES)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.eq_ignore_ascii_case("bytes"))
        .unwrap_or(false);

    anyhow::ensure!(
        supports_ranges,
        "remote server does not advertise byte ranges"
    );

    let total_len =
        usize::try_from(total_bytes).context("resource too large to fit into memory")?;
    let chunk_size = total_len.clamp(1, RANGE_CHUNK_SIZE_BYTES);
    let segments = total_len.div_ceil(chunk_size);

    info!(
        %url,
        total_bytes,
        segments,
        "downloading resource via HTTP range requests"
    );

    let url = Arc::new(url.to_string());
    let chunks = stream::iter((0..segments).map(move |index| {
        let start = (index * chunk_size) as u64;
        let len = ((index + 1) * chunk_size).min(total_len) - (index * chunk_size);
        let end = start + len as u64 - 1;
        let url = Arc::clone(&url);
        async move {
            let chunk = http_range(&url, start, end).await?;
            Ok::<_, anyhow::Error>((start, chunk))
        }
    }))
    .buffer_unordered(segments)
    .try_collect::<Vec<_>>()
    .await?;

    let mut parts = chunks;
    parts.sort_by_key(|(start, _)| *start);

    let mut buffer = Vec::with_capacity(total_len);
    for (_start, mut chunk) in parts {
        buffer.append(&mut chunk);
    }

    anyhow::ensure!(
        buffer.len() == total_len,
        "range assembly mismatch: expected {} bytes, got {}",
        total_len,
        buffer.len()
    );

    Ok(buffer)
}

async fn http_range(url: &str, start: u64, end: u64) -> anyhow::Result<Vec<u8>> {
    let expected_len = usize::try_from(end - start + 1)?;
    let response = HTTP_CLIENT
        .get(url)
        .header(RANGE, format!("bytes={start}-{end}"))
        .send()
        .await?
        .error_for_status()?;

    let mut bytes = Vec::with_capacity(expected_len);
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.context("failed to read range body")?;
        bytes.extend_from_slice(&chunk);
    }

    anyhow::ensure!(
        bytes.len() == expected_len,
        "range returned {} bytes (expected {expected_len})",
        bytes.len()
    );

    Ok(bytes)
}


/// Given a list of base URLs and a test path, returns the fastest mirror
/// (by HTTP round-trip time) that successfully responds (2xx / 3xx).
///
/// `test_path` is appended to each base URL like `<base>/<test_path>`.
pub async fn fastest_mirror<'a>(
    base_urls: &[&'a str],
    test_path: &str,
    timeout: Duration,
) -> Option<&'a str> {
    // Build all the requests as futures
    let futures = base_urls.iter().map(|&base| {
        let client = http_client();

        // Join base + path safely
        let url = format!(
            "{}/{}",
            base.trim_end_matches('/'),
            test_path.trim_start_matches('/')
        );

        async move {
            let start = Instant::now();
            let res = client
                .head(&url)
                .timeout(timeout)
                .send()
                .await
                .and_then(|r| Ok(r.error_for_status()));

            match res {
                Ok(_) => Some((base, start.elapsed())),
                Err(_) => None,
            }
        }
    });

    let results = join_all(futures).await;

    // Pick the one with the smallest elapsed time
    let mut best: Option<(&str, Duration)> = None;

    for r in results.into_iter().flatten() {
        let (base, elapsed) = r;
        match &mut best {
            None => best = Some((base, elapsed)),
            Some((_, best_elapsed)) if elapsed < *best_elapsed => {
                *best_elapsed = elapsed;
                best.as_mut().unwrap().0 = base;
            }
            _ => {}
        }
    }

    best.map(|(base, _)| base)
}