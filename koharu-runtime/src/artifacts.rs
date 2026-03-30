use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use hf_hub::{Cache, api::tokio::ApiBuilder};

use crate::downloads::TransferHub;
use crate::http::HttpStack;
use crate::layout::Layout;

#[derive(Clone)]
pub struct ArtifactStore {
    layout: Layout,
    http: HttpStack,
    transfers: TransferHub,
}

struct DownloadRequest<'a> {
    url: &'a str,
    bearer_token: Option<&'a str>,
    destination: &'a Path,
    label: &'a str,
}

impl ArtifactStore {
    pub(crate) fn new(layout: Layout, http: HttpStack, transfers: TransferHub) -> Self {
        Self {
            layout,
            http,
            transfers,
        }
    }

    pub async fn huggingface_model(&self, repo: &str, filename: &str) -> Result<PathBuf> {
        let destination = self.huggingface_path(repo, filename)?;
        if destination.exists() {
            return Ok(destination);
        }

        let api = ApiBuilder::from_env()
            .with_progress(false)
            .build()
            .context("failed to build HF Hub API")?;
        let url = api.model(repo.to_string()).url(filename);
        let token = Cache::from_env()
            .token()
            .filter(|value| !value.trim().is_empty());

        self.download(DownloadRequest {
            url: &url,
            bearer_token: token.as_deref(),
            destination: &destination,
            label: filename,
        })
        .await?;

        Ok(destination)
    }

    pub fn huggingface_path(&self, repo: &str, filename: &str) -> Result<PathBuf> {
        huggingface_path(self.layout.huggingface_root(), repo, filename)
    }

    pub(crate) async fn cached_download(
        &self,
        url: &str,
        file_name: &str,
        cache_dir: &Path,
    ) -> Result<PathBuf> {
        let destination = cache_dir.join(file_name);
        if destination.exists() {
            return Ok(destination);
        }

        self.download(DownloadRequest {
            url,
            bearer_token: None,
            destination: &destination,
            label: file_name,
        })
        .await?;

        Ok(destination)
    }

    async fn download(&self, request: DownloadRequest<'_>) -> Result<()> {
        let parent = request
            .destination
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid destination path for `{}`", request.label))?;
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create `{}`", parent.display()))?;

        let temp_path = part_path(request.destination)?;
        if temp_path.exists() {
            tokio::fs::remove_file(&temp_path).await.ok();
        }

        let mut http_request = self.http.client().get(request.url.to_string());
        if let Some(token) = request.bearer_token {
            http_request = http_request.bearer_auth(token);
        }

        let response = http_request
            .send()
            .await
            .with_context(|| format!("failed to start download `{}`", request.url))?
            .error_for_status()
            .with_context(|| format!("download failed for `{}`", request.url))?;

        let mut reporter = self.transfers.begin(request.label);
        reporter.start(response.content_length());

        let result = async {
            let mut file = tokio::fs::File::create(&temp_path)
                .await
                .with_context(|| format!("failed to create `{}`", temp_path.display()))?;
            let mut stream = response.bytes_stream();

            while let Some(chunk) = futures::StreamExt::next(&mut stream).await {
                let chunk = chunk.context("failed to read download chunk")?;
                tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                    .await
                    .with_context(|| format!("failed to write `{}`", temp_path.display()))?;
                reporter.advance(chunk.len());
            }

            tokio::io::AsyncWriteExt::flush(&mut file)
                .await
                .with_context(|| format!("failed to flush `{}`", temp_path.display()))?;
            drop(file);

            if request.destination.exists() {
                tokio::fs::remove_file(request.destination).await.ok();
            }
            tokio::fs::rename(&temp_path, request.destination)
                .await
                .with_context(|| {
                    format!(
                        "failed to move `{}` to `{}`",
                        temp_path.display(),
                        request.destination.display()
                    )
                })?;

            Ok::<_, anyhow::Error>(())
        }
        .await;

        match result {
            Ok(()) => {
                reporter.finish();
                Ok(())
            }
            Err(error) => {
                reporter.fail(&error);
                tokio::fs::remove_file(&temp_path).await.ok();
                Err(error)
            }
        }
    }
}

pub fn huggingface_path(root: &Path, repo: &str, filename: &str) -> Result<PathBuf> {
    let mut path = root.to_path_buf();

    for segment in repo.split('/') {
        anyhow::ensure!(
            !segment.is_empty() && segment != "." && segment != "..",
            "invalid HF repo segment `{segment}` in `{repo}`"
        );
        path.push(segment);
    }

    let relative = Path::new(filename);
    anyhow::ensure!(
        relative.is_relative(),
        "absolute HF filename `{filename}` is not allowed"
    );

    for component in relative.components() {
        match component {
            Component::Normal(segment) => path.push(segment),
            Component::CurDir => {}
            _ => anyhow::bail!("invalid HF filename `{filename}`"),
        }
    }

    Ok(path)
}

fn part_path(destination: &Path) -> Result<PathBuf> {
    let file_name = destination.file_name().ok_or_else(|| {
        anyhow::anyhow!(
            "destination `{}` does not have a filename",
            destination.display()
        )
    })?;
    Ok(destination.with_file_name(format!("{}.part", file_name.to_string_lossy())))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{huggingface_path, part_path};

    #[test]
    fn destination_path_is_stable() {
        let path = huggingface_path(
            Path::new("/tmp/models/huggingface"),
            "Qwen/Qwen2.5-VL-3B-Instruct",
            "config.json",
        )
        .unwrap();

        assert_eq!(
            path,
            Path::new("/tmp/models")
                .join("huggingface")
                .join("Qwen")
                .join("Qwen2.5-VL-3B-Instruct")
                .join("config.json")
        );
    }

    #[test]
    fn destination_path_rejects_parent_traversal() {
        let error = huggingface_path(
            Path::new("/tmp/models/huggingface"),
            "repo/name",
            "../config.json",
        )
        .expect_err("parent traversal should be rejected");
        assert!(error.to_string().contains("invalid HF filename"));
    }

    #[test]
    fn partial_download_path_appends_suffix() {
        let part = part_path(Path::new("/tmp/models/config.json")).unwrap();
        assert_eq!(part, Path::new("/tmp/models/config.json.part"));
    }
}
