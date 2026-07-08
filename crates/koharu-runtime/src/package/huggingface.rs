use std::{path::PathBuf, sync::LazyLock};

use crate::{
    download::{client::Client, huggingface::huggingface as huggingface_url},
    package::{Package, STORE_DIR},
};

static HUGGINGFACE_DIR: LazyLock<PathBuf> = LazyLock::new(|| STORE_DIR.join("huggingface"));

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct HuggingFace {
    pub repo: String,
    pub filename: String,
}

#[async_trait::async_trait]
impl Package for HuggingFace {
    async fn resolve(&self) -> anyhow::Result<PathBuf> {
        let repo = self.repo.replace('/', "--");
        let path = HUGGINGFACE_DIR.join(&repo).join(&self.filename);
        if path.exists() {
            return Ok(path);
        }

        let client = Client::new();
        let url = huggingface_url(&self.repo, &self.filename);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        client.download(&url, path.clone()).await?;

        Ok(path)
    }
}

/// Resolves a HuggingFace package given a repository and filename, returning the local path to the downloaded file.
pub async fn resolve((repo, filename): (&str, &str)) -> anyhow::Result<PathBuf> {
    let package = HuggingFace {
        repo: repo.to_owned(),
        filename: filename.to_owned(),
    };
    Package::resolve(&package).await
}

/// Macro to define HuggingFace packages in a concise manner.
#[macro_export]
macro_rules! huggingface {
    ($($vis:vis $name:ident => $repo:expr => $filename:expr),+ $(,)?) => {
        $(
            $vis const $name: (&'static str, &'static str) = ($repo, $filename);
        )+
    };
    ($($repo:expr => $filename:expr),* $(,)?) => {
        [
            $(
                ($repo, $filename)
            ),*
        ]
    };
}

pub use crate::huggingface;
