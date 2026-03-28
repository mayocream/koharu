use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};

use crate::config::{DownloadConfig, download_config};

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

struct HttpClientState {
    config: DownloadConfig,
    client: ClientWithMiddleware,
}

static HTTP_CLIENT_STATE: std::sync::LazyLock<std::sync::RwLock<HttpClientState>> =
    std::sync::LazyLock::new(|| {
        let config = download_config();
        let client = build_http_client(&config).expect("build reqwest client");
        std::sync::RwLock::new(HttpClientState { config, client })
    });

pub fn http_client() -> ClientWithMiddleware {
    let config = download_config();

    if let Some(client) = {
        let state = HTTP_CLIENT_STATE
            .read()
            .expect("http client state poisoned");
        (state.config == config).then(|| state.client.clone())
    } {
        return client;
    }

    let mut state = HTTP_CLIENT_STATE
        .write()
        .expect("http client state poisoned");
    if state.config != config {
        state.client = build_http_client(&config).expect("build reqwest client");
        state.config = config;
    }

    state.client.clone()
}

fn build_http_client(config: &DownloadConfig) -> anyhow::Result<ClientWithMiddleware> {
    let mut builder = reqwest::Client::builder().user_agent(USER_AGENT);
    if let Some(proxy_url) = &config.proxy_url {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url)?);
    }

    Ok(ClientBuilder::new(builder.build()?)
        .with(RetryTransientMiddleware::new_with_policy(
            ExponentialBackoff::builder().build_with_max_retries(3),
        ))
        .build())
}
