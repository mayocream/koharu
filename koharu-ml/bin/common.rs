use directories::ProjectDirs;
use tracing_subscriber::fmt::format::FmtSpan;

pub fn default_models_root() -> std::path::PathBuf {
    ProjectDirs::from("rs", "Koharu", "Koharu")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("models")
}

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .init();
}
