use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DownloaderConfig {
    pub proxy_url: Option<String>,
    pub pypi_base_url: Option<String>,
    pub github_release_base_url: Option<String>,
}

impl DownloaderConfig {
    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Self::default());
        }

        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read downloader config `{}`", path.display()))?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse downloader config `{}`", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create downloader config dir `{}`",
                    parent.display()
                )
            })?;
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        fs::write(&path, bytes)
            .with_context(|| format!("failed to write downloader config `{}`", path.display()))
    }

    pub fn apply(&self) -> Result<()> {
        koharu_http::config::set_download_config(self.clone().into())
    }

    fn path() -> PathBuf {
        koharu_http::paths::app_root().join("downloader-config.json")
    }
}

impl From<DownloaderConfig> for koharu_http::config::DownloadConfig {
    fn from(value: DownloaderConfig) -> Self {
        Self {
            proxy_url: value.proxy_url,
            pypi_base_url: value.pypi_base_url,
            github_release_base_url: value.github_release_base_url,
        }
    }
}
