use anyhow::{Context, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadConfig {
    pub proxy_url: Option<String>,
    pub pypi_base_url: Option<String>,
    pub github_release_base_url: Option<String>,
}

static DOWNLOAD_CONFIG: std::sync::LazyLock<std::sync::RwLock<DownloadConfig>> =
    std::sync::LazyLock::new(|| std::sync::RwLock::new(DownloadConfig::default()));

const DEFAULT_PYPI_BASE_URL: &str = "https://pypi.org";
const DEFAULT_GITHUB_RELEASE_BASE_URL: &str =
    "https://github.com/ggml-org/llama.cpp/releases/download";

pub fn download_config() -> DownloadConfig {
    DOWNLOAD_CONFIG
        .read()
        .expect("download config poisoned")
        .clone()
}

pub fn set_download_config(config: DownloadConfig) -> Result<()> {
    validate_download_config(&config)?;
    *DOWNLOAD_CONFIG.write().expect("download config poisoned") =
        normalize_download_config(config)?;
    Ok(())
}

pub fn reset_download_config() {
    *DOWNLOAD_CONFIG.write().expect("download config poisoned") = DownloadConfig::default();
}

pub fn pypi_base_url() -> String {
    download_config()
        .pypi_base_url
        .unwrap_or_else(|| DEFAULT_PYPI_BASE_URL.to_string())
}

pub fn github_release_base_url() -> String {
    download_config()
        .github_release_base_url
        .unwrap_or_else(|| DEFAULT_GITHUB_RELEASE_BASE_URL.to_string())
}

fn validate_download_config(config: &DownloadConfig) -> Result<()> {
    if let Some(proxy_url) = &config.proxy_url {
        reqwest::Proxy::all(proxy_url)
            .with_context(|| format!("invalid proxy url `{proxy_url}`"))?;
    }

    for (label, value) in [
        ("pypi base url", config.pypi_base_url.as_deref()),
        (
            "github release base url",
            config.github_release_base_url.as_deref(),
        ),
    ] {
        if let Some(url) = value {
            reqwest::Url::parse(url).with_context(|| format!("invalid {label} `{url}`"))?;
        }
    }

    Ok(())
}

fn normalize_download_config(mut config: DownloadConfig) -> Result<DownloadConfig> {
    config.proxy_url = normalize_optional_value(config.proxy_url);
    config.pypi_base_url = normalize_optional_url(config.pypi_base_url)?;
    config.github_release_base_url = normalize_optional_url(config.github_release_base_url)?;
    Ok(config)
}

fn normalize_optional_value(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn normalize_optional_url(value: Option<String>) -> Result<Option<String>> {
    let Some(value) = normalize_optional_value(value) else {
        return Ok(None);
    };
    let mut url = reqwest::Url::parse(&value)?;
    while url.path().ends_with('/') && url.path() != "/" {
        let trimmed = url.path().trim_end_matches('/').to_string();
        url.set_path(&trimmed);
    }
    Ok(Some(url.to_string().trim_end_matches('/').to_string()))
}
