use sentry::ClientOptions;
use tracing_subscriber::registry::LookupSpan;

pub fn initialize() -> sentry::ClientInitGuard {
    sentry::init(ClientOptions {
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
