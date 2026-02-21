use tracing_subscriber::fmt::format::FmtSpan;

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .init();
}
