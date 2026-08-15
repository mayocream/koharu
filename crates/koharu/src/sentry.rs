use sentry::{ClientOptions, IntoDsn};
use tracing_subscriber::registry::LookupSpan;

pub fn initialize() -> sentry::ClientInitGuard {
    let mut options = ClientOptions::new()
        .maybe_release(sentry::release_name!())
        .send_default_pii(true)
        .sample_rate(0.1)
        .auto_session_tracking(true);
    options.dsn = option_env!("SENTRY_DSN")
        .into_dsn()
        .expect("invalid SENTRY_DSN environment variable");
    sentry::init(options)
}

pub fn tracing_layer<S>() -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
{
    sentry::integrations::tracing::layer()
}
