use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};

use crate::Settings;

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

#[derive(Clone)]
pub(crate) struct HttpStack {
    client: Arc<ClientWithMiddleware>,
}

impl HttpStack {
    pub(crate) fn new(settings: &Settings) -> Result<Self> {
        Ok(Self {
            client: Arc::new(
                ClientBuilder::new(build_base_client(settings)?)
                    .with(RetryTransientMiddleware::new_with_policy(retry_policy()))
                    .build(),
            ),
        })
    }

    pub(crate) fn client(&self) -> Arc<ClientWithMiddleware> {
        Arc::clone(&self.client)
    }
}

fn build_base_client(settings: &Settings) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60));

    if let Some(proxy) = settings.http_proxy() {
        builder = builder.proxy(reqwest::Proxy::all(proxy.as_str())?);
    }

    Ok(builder.build()?)
}

fn retry_policy() -> ExponentialBackoff {
    ExponentialBackoff::builder().build_with_max_retries(3)
}
