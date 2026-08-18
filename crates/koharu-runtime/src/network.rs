use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

const USER_AGENT: &str = concat!("koharu/", env!("CARGO_PKG_VERSION"));
static API: OnceLock<Client> = OnceLock::new();
static DOWNLOADS: Mutex<Option<CachedClient>> = Mutex::new(None);

pub(crate) type DownloadClient = Arc<reqwest_middleware::ClientWithMiddleware>;

struct CachedClient {
    revision: koharu_config::ConfigRevision,
    client: DownloadClient,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Policy {
    #[serde(rename = "connect_timeout")]
    connect_timeout_seconds: u64,
    #[serde(rename = "read_timeout")]
    read_timeout_seconds: u64,
    max_retries: u32,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            connect_timeout_seconds: 20,
            read_timeout_seconds: 300,
            max_retries: 3,
        }
    }
}

pub(crate) fn api_client() -> Result<Client> {
    if let Some(client) = API.get() {
        return Ok(client.clone());
    }
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .http2_adaptive_window(true)
        .build()?;
    Ok(API.get_or_init(|| client).clone())
}

pub(crate) fn download_client() -> Result<DownloadClient> {
    let config = koharu_config::load::<Policy>("http")?;
    let policy = config.read()?;
    let revision = config.revision();
    let mut cached = DOWNLOADS
        .lock()
        .map_err(|_| anyhow::anyhow!("download client state is poisoned"))?;
    if let Some(active) = cached.as_ref()
        && active.revision == revision
    {
        return Ok(active.client.clone());
    }

    let base = Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(policy.connect_timeout_seconds.max(1)))
        .read_timeout(Duration::from_secs(policy.read_timeout_seconds.max(1)))
        // Parallel ranges need separate TCP congestion windows instead of one
        // HTTP/2 connection shared by every request.
        .http1_only()
        .tcp_keepalive(Duration::from_secs(60))
        .build()
        .context("failed to build download client")?;
    let client = Arc::new(
        reqwest_middleware::ClientBuilder::new(base)
            .with(reqwest_retry::RetryTransientMiddleware::new_with_policy(
                reqwest_retry::policies::ExponentialBackoff::builder()
                    .build_with_max_retries(policy.max_retries),
            ))
            .build(),
    );
    *cached = Some(CachedClient {
        revision,
        client: client.clone(),
    });
    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_policy_keeps_api_requests_unaffected() {
        let encoded = toml::Value::try_from(Policy::default()).unwrap();
        let fields = encoded.as_table().unwrap();
        assert!(fields.contains_key("connect_timeout"));
        assert!(fields.contains_key("read_timeout"));
        assert_eq!(fields["max_retries"].as_integer(), Some(3));
    }
}
