use std::{path::PathBuf, sync::LazyLock};

use crate::{
    download::{client::Client, huggingface::huggingface as huggingface_url},
    package::{Package, STORE_DIR},
};

static HUGGINGFACE_DIR: LazyLock<PathBuf> = LazyLock::new(|| STORE_DIR.join("huggingface"));

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct HuggingFace {
    pub repo: String,
    pub revision: String,
    pub filename: String,
}

#[async_trait::async_trait]
impl Package for HuggingFace {
    async fn resolve(&self) -> anyhow::Result<PathBuf> {
        let path = self.local_path();
        if path.exists() {
            return Ok(path);
        }

        let client = Client::new()?;
        let url = huggingface_url(&self.repo, &self.revision, &self.filename);
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid Hugging Face package path"))?;
        tokio::fs::create_dir_all(parent).await?;
        let temporary = tempfile::NamedTempFile::new_in(parent)?.into_temp_path();
        client.download(&url, temporary.to_path_buf()).await?;
        temporary.persist(&path)?;

        Ok(path)
    }
}

impl HuggingFace {
    fn local_path(&self) -> PathBuf {
        HUGGINGFACE_DIR
            .join(self.repo.replace('/', "--"))
            .join("revisions")
            .join(self.revision.replace('/', "--"))
            .join(&self.filename)
    }
}

/// Resolves a Hugging Face package at an immutable revision.
pub async fn resolve((repo, revision, filename): (&str, &str, &str)) -> anyhow::Result<PathBuf> {
    let package = HuggingFace {
        repo: repo.to_owned(),
        revision: revision.to_owned(),
        filename: filename.to_owned(),
    };
    Package::resolve(&package).await
}

/// Returns whether an immutable artifact already exists in the local package store.
#[must_use]
pub fn is_resolved((repo, revision, filename): (&str, &str, &str)) -> bool {
    HuggingFace {
        repo: repo.to_owned(),
        revision: revision.to_owned(),
        filename: filename.to_owned(),
    }
    .local_path()
    .is_file()
}

/// Macro to define HuggingFace packages in a concise manner.
#[macro_export]
macro_rules! huggingface {
    ($($vis:vis $name:ident => $repo:expr => $revision:expr => $filename:expr),+ $(,)?) => {
        $(
            $vis const $name: (&'static str, &'static str, &'static str) =
                ($repo, $revision, $filename);
        )+
    };
    ($($repo:expr => $revision:expr => $filename:expr),* $(,)?) => {
        [
            $(
                ($repo, $revision, $filename)
            ),*
        ]
    };
}

pub use crate::huggingface;
