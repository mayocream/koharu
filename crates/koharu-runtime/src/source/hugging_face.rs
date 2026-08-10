use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::{Arc, LazyLock},
};

use anyhow::{Context, ensure};
use serde::Deserialize;
use tokio::sync::{Mutex, OnceCell};

use crate::{Store, downloads::Transfer};

static REVISIONS: LazyLock<Mutex<HashMap<String, Arc<OnceCell<String>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Revision<'a> {
    Pinned(&'a str),
    Latest,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum RepositoryKind {
    Model,
    Dataset,
}

impl RepositoryKind {
    const fn api_route(self) -> &'static str {
        match self {
            Self::Model => "models",
            Self::Dataset => "datasets",
        }
    }

    const fn resolve_prefix(self) -> &'static str {
        match self {
            Self::Model => "",
            Self::Dataset => "datasets/",
        }
    }
}

/// An immutable file snapshot hosted by Hugging Face.
///
/// A latest file resolves the repository head once per process. Every file in
/// that repository then uses the same commit, so a model cannot be assembled
/// from different revisions if its repository changes during initialization.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HuggingFaceFile<'a> {
    kind: RepositoryKind,
    repository: &'a str,
    revision: Revision<'a>,
    filename: &'a str,
}

impl<'a> HuggingFaceFile<'a> {
    #[must_use]
    pub const fn pinned(repository: &'a str, revision: &'a str, filename: &'a str) -> Self {
        Self {
            kind: RepositoryKind::Model,
            repository,
            revision: Revision::Pinned(revision),
            filename,
        }
    }

    #[must_use]
    pub const fn latest(repository: &'a str, filename: &'a str) -> Self {
        Self {
            kind: RepositoryKind::Model,
            repository,
            revision: Revision::Latest,
            filename,
        }
    }

    #[must_use]
    pub const fn latest_dataset(repository: &'a str, filename: &'a str) -> Self {
        Self {
            kind: RepositoryKind::Dataset,
            repository,
            revision: Revision::Latest,
            filename,
        }
    }

    #[must_use]
    pub const fn pinned_dataset(repository: &'a str, revision: &'a str, filename: &'a str) -> Self {
        Self {
            kind: RepositoryKind::Dataset,
            repository,
            revision: Revision::Pinned(revision),
            filename,
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn resolve(self) -> anyhow::Result<PathBuf> {
        let mut repository = self.repository.split('/');
        let owner = repository.next().unwrap_or_default();
        let name = repository.next().unwrap_or_default();
        ensure!(
            repository.next().is_none()
                && [owner, name].into_iter().all(|part| {
                    !part.is_empty()
                        && part != "."
                        && part != ".."
                        && part.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                        })
                }),
            "invalid Hugging Face repository {}",
            self.repository
        );
        ensure!(
            !self.filename.is_empty()
                && !self.filename.contains('\\')
                && Path::new(self.filename)
                    .components()
                    .all(|component| matches!(component, Component::Normal(_))),
            "invalid Hugging Face filename {}",
            self.filename
        );

        let revision = match self.revision {
            Revision::Pinned(revision) => {
                ensure!(is_commit(revision), "invalid pinned revision {revision}");
                revision.to_owned()
            }
            Revision::Latest => latest_revision(self.kind, self.repository).await?,
        };
        let target = snapshot_path(self.kind, self.repository, &revision, self.filename);
        Store::file(target, move |stage| async move {
            let url = format!(
                "https://huggingface.co/{}{}/resolve/{revision}/{}",
                self.kind.resolve_prefix(),
                self.repository,
                self.filename
            );
            Transfer::new()?.fetch(&url, &stage).await
        })
        .await
    }
}

async fn latest_revision(kind: RepositoryKind, repository: &str) -> anyhow::Result<String> {
    let key = format!("{}/{repository}", kind.api_route());
    let revision = {
        let mut revisions = REVISIONS.lock().await;
        revisions
            .entry(key)
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone()
    };

    revision
        .get_or_try_init(|| async {
            #[derive(Deserialize)]
            struct Repository {
                sha: String,
            }

            let url = format!(
                "https://huggingface.co/api/{}/{repository}",
                kind.api_route()
            );
            let response = Transfer::new()?
                .get(&url)
                .send()
                .await
                .with_context(|| format!("failed to resolve {repository}"))?
                .error_for_status()
                .with_context(|| format!("failed to resolve {repository}"))?
                .json::<Repository>()
                .await
                .with_context(|| format!("invalid metadata for {repository}"))?;
            ensure!(
                is_commit(&response.sha),
                "{repository} returned invalid commit {}",
                response.sha
            );
            Ok(response.sha)
        })
        .await
        .cloned()
}

fn snapshot_path(
    kind: RepositoryKind,
    repository: &str,
    revision: &str,
    filename: &str,
) -> PathBuf {
    Store::root()
        .join("hugging-face")
        .join(kind.api_route())
        .join(repository.replace('/', "--"))
        .join("snapshots")
        .join(revision)
        .join(filename)
}

fn is_commit(revision: &str) -> bool {
    revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_full_commit_hashes() {
        assert!(is_commit("0123456789abcdef0123456789abcdef01234567"));
        assert!(is_commit("0123456789ABCDEF0123456789ABCDEF01234567"));
        assert!(!is_commit("01234567"));
        assert!(!is_commit("z123456789abcdef0123456789abcdef01234567"));
    }

    #[test]
    fn stores_files_by_immutable_snapshot() {
        let path = snapshot_path(
            RepositoryKind::Model,
            "owner/model",
            "0123456789abcdef0123456789abcdef01234567",
            "subdir/model.safetensors",
        );
        assert!(path.ends_with(
            "owner--model/snapshots/0123456789abcdef0123456789abcdef01234567/subdir/model.safetensors"
        ));
    }

    #[test]
    fn stores_dataset_files_in_the_dataset_namespace() {
        let path = snapshot_path(
            RepositoryKind::Dataset,
            "owner/dataset",
            "0123456789abcdef0123456789abcdef01234567",
            "previews/example.webp",
        );
        assert!(path.ends_with(
            "datasets/owner--dataset/snapshots/0123456789abcdef0123456789abcdef01234567/previews/example.webp"
        ));
    }

    #[tokio::test]
    async fn rejects_paths_that_escape_the_snapshot() {
        let revision = "0123456789abcdef0123456789abcdef01234567";
        assert!(
            HuggingFaceFile::pinned("owner/model", revision, "../model.bin")
                .resolve()
                .await
                .is_err()
        );
        assert!(
            HuggingFaceFile::pinned("owner\\model", revision, "model.bin")
                .resolve()
                .await
                .is_err()
        );
        assert!(
            HuggingFaceFile::pinned("owner/model", "main", "model.bin")
                .resolve()
                .await
                .is_err()
        );
    }
}
