use std::env;
use std::path::PathBuf;

const APP_NAME: &str = "Koharu";
const APP_ROOT_ENV: &str = "KOHARU_APP_ROOT";
const MODEL_ROOT_ENV: &str = "KOHARU_MODEL_ROOT";
const RUNTIME_ROOT_ENV: &str = "KOHARU_RUNTIME_ROOT";

pub fn app_root() -> PathBuf {
    if let Some(path) = env::var_os(APP_ROOT_ENV) {
        return PathBuf::from(path);
    }

    dirs::data_local_dir()
        .unwrap_or_else(env::temp_dir)
        .join(APP_NAME)
}

pub fn model_root() -> PathBuf {
    if let Some(path) = env::var_os(MODEL_ROOT_ENV) {
        return PathBuf::from(path);
    }
    app_root().join("models")
}

pub fn runtime_root() -> PathBuf {
    if let Some(path) = env::var_os(RUNTIME_ROOT_ENV) {
        return PathBuf::from(path);
    }
    app_root().join("runtime")
}
