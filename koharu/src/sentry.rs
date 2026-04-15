use sentry::{ClientOptions, IntoDsn};
use tracing_subscriber::registry::LookupSpan;

pub fn initialize() -> sentry::ClientInitGuard {
    sentry::init(ClientOptions {
        dsn: option_env!("SENTRY_DSN")
            .into_dsn()
            .expect("invalid SENTRY_DSN environment variable"),
        release: sentry::release_name!(),
        send_default_pii: true,
        ..Default::default()
    })
}

pub fn tracing_layer<S>() -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
{
    sentry::integrations::tracing::layer()
}
