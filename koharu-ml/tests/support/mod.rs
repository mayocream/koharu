use directories::ProjectDirs;
use std::path::PathBuf;

pub fn default_models_root() -> PathBuf {
    ProjectDirs::from("rs", "Koharu", "Koharu")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("models")
}
