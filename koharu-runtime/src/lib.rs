use once_cell::sync::Lazy;

pub mod dylib;
pub mod zip;

// shared HTTP client with custom user agent
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
pub static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("build reqwest client")
});
