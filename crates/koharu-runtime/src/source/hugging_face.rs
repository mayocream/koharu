use std::{path::PathBuf, sync::OnceLock};

use anyhow::Context;
use hf_hub::{HFClient, repository::download::HFByteStream, split_id};

use crate::{Store, download, network};

static CLIENT: OnceLock<HFClient> = OnceLock::new();

fn client() -> anyhow::Result<HFClient> {
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let max_retries = network::config()?.max_retries as usize;
    let http = network::http()?;
    let client = hf_hub::HFClient::builder()
        .client(http)
        .cache_enabled(false)
        .retry_max_attempts(max_retries)
        .build()?;
    Ok(CLIENT.get_or_init(|| client).clone())
}

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

#[derive(Clone, Copy)]
struct Repository<'a> {
    id: &'a str,
    owner: &'a str,
    name: &'a str,
}

impl<'a> Repository<'a> {
    fn new(id: &'a str) -> Self {
        let (owner, name) = split_id(id);
        Self { id, owner, name }
    }
}

impl RepositoryKind {
    const fn api_route(self) -> &'static str {
        match self {
            Self::Model => "models",
            Self::Dataset => "datasets",
        }
    }

    async fn latest_revision(self, repository: Repository<'_>) -> anyhow::Result<String> {
        let client = client()?;
        let sha = match self {
            Self::Model => {
                client
                    .model(repository.owner, repository.name)
                    .info()
                    .send()
                    .await?
                    .sha
            }
            Self::Dataset => {
                client
                    .dataset(repository.owner, repository.name)
                    .info()
                    .send()
                    .await?
                    .sha
            }
        };
        sha.with_context(|| format!("{} metadata omitted its commit", repository.id))
    }

    async fn download(
        self,
        client: &HFClient,
        repository: Repository<'_>,
        revision: String,
        filename: &str,
    ) -> anyhow::Result<(Option<u64>, HFByteStream)> {
        let result = match self {
            Self::Model => {
                client
                    .model(repository.owner, repository.name)
                    .download_file_stream()
                    .filename(filename)
                    .revision(revision)
                    .send()
                    .await
            }
            Self::Dataset => {
                client
                    .dataset(repository.owner, repository.name)
                    .download_file_stream()
                    .filename(filename)
                    .revision(revision)
                    .send()
                    .await
            }
        };
        result.map_err(Into::into)
    }
}

/// An immutable file snapshot hosted by Hugging Face.
///
/// A latest file resolves the current repository head before locating its
/// snapshot.
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
        let repository = Repository::new(self.repository);
        let revision = match self.revision {
            Revision::Pinned(revision) => revision.to_owned(),
            Revision::Latest => self.kind.latest_revision(repository).await?,
        };
        let repository_name = repository.id.replace(['/', '\\'], "--");
        let target = Store::root()
            .join("hugging-face")
            .join(self.kind.api_route())
            .join(repository_name)
            .join("snapshots")
            .join(&revision)
            .join(self.filename);
        Store::file(target, move |stage| async move {
            let client = client()?;
            download::receive(
                self.filename,
                &stage,
                self.kind
                    .download(&client, repository, revision, self.filename),
            )
            .await
        })
        .await
    }
}
