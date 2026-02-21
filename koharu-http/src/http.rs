use once_cell::sync::Lazy;
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
