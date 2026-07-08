use serde::Deserialize;

use crate::download::client::Client;

/// Platforms supported by PyPI wheels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display)]
pub enum Platform {
    #[strum(serialize = "win_amd64")]
    WindowsX64,
    #[strum(serialize = "manylinux,x86_64")]
    LinuxX64,
    #[strum(serialize = "manylinux,aarch64")]
    LinuxAarch64,
}

impl Platform {
    pub fn matches(&self, filename: &str) -> bool {
        self.to_string()
            .split(',')
            .all(|platform| filename.contains(platform))
    }

    pub fn current() -> Option<Self> {
        if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
            Some(Platform::WindowsX64)
        } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
            Some(Platform::LinuxX64)
        } else if cfg!(target_os = "linux") && cfg!(target_arch = "aarch64") {
            Some(Platform::LinuxAarch64)
        } else {
            None
        }
    }
}

#[derive(Debug, Deserialize)]
struct Release {
    urls: Vec<File>,
}

#[derive(Debug, Deserialize)]
struct File {
    filename: String,
    url: String,
}

pub async fn wheel(package: &str, platform: Platform) -> anyhow::Result<String> {
    let client = Client::new();
    let metadata_url = format!("https://pypi.org/pypi/{package}/json");
    let release: Release = client
        .get(&metadata_url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch metadata for {package}: {e}"))?
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("failed to parse metadata for {package}: {e}"))?;

    release
        .urls
        .into_iter()
        .find(|file| file.filename.ends_with(".whl") && platform.matches(&file.filename))
        .map(|file| file.url)
        .ok_or_else(|| anyhow::anyhow!("No wheel file found for {package} on {platform}"))
}
