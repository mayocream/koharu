use anyhow::Context;
use futures::StreamExt;
use once_cell::sync::Lazy;
use reqwest::header::RANGE;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

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

pub async fn http_chunk(url: &str, start: u64, end: u64) -> anyhow::Result<Vec<u8>> {
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
