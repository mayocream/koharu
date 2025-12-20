use std::path::{Path, PathBuf};

use anyhow::Context;
use futures::{StreamExt, TryStreamExt};
use koharu_core::http::http_client;
use once_cell::sync::Lazy;
use serde::Deserialize;

pub static FONTS_DIR: Lazy<PathBuf> = Lazy::new(|| {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Koharu")
        .join("fonts")
});

/// Google Fonts downloader and cache manager.
#[derive(Default)]
pub struct GoogleFonts;

impl GoogleFonts {
    pub fn new() -> Self {
        Self
    }

    /// Downloads font families and returns paths to the font files.
    ///
    /// Downloaded fonts are cached locally and reused on subsequent calls.
    pub async fn font_families(&self, families: &[&str]) -> anyhow::Result<Vec<PathBuf>> {
        tokio::fs::create_dir_all(&*FONTS_DIR).await?;

        let fonts = futures::stream::iter(families)
            .map(|family| async move { fetch_and_extract(family).await })
            .buffer_unordered(num_cpus::get())
            .try_collect::<Vec<Vec<PathBuf>>>()
            .await?
            .into_iter()
            .flatten()
            .collect::<Vec<PathBuf>>();

        Ok(fonts)
    }
}

async fn fetch_and_extract(family: &str) -> anyhow::Result<Vec<PathBuf>> {
    let manifest = fetch_manifest(family).await?;
    let font_entries = manifest
        .file_refs
        .into_iter()
        .filter(|entry| {
            let name = entry.filename.to_ascii_lowercase();
            name.ends_with(".ttf") || name.ends_with(".otf")
        })
        .collect::<Vec<_>>();

    if font_entries.is_empty() {
        anyhow::bail!("no font files found for family {family}");
    }

    let fonts = futures::stream::iter(font_entries)
        .map(|entry| async move { download_font(entry).await })
        .buffer_unordered(num_cpus::get())
        .try_collect::<Vec<_>>()
        .await?;

    Ok(fonts)
}

#[derive(Debug, Deserialize)]
struct ListResponse {
    #[serde(default)]
    manifest: Manifest,
}

#[derive(Debug, Default, Deserialize)]
struct Manifest {
    #[serde(default, rename = "fileRefs")]
    file_refs: Vec<FileRef>,
}

#[derive(Debug, Deserialize)]
struct FileRef {
    filename: String,
    url: String,
}

fn encode_family(family: &str) -> String {
    family.replace(' ', "+")
}

async fn fetch_manifest(family: &str) -> anyhow::Result<Manifest> {
    let url = format!(
        "https://fonts.google.com/download/list?family={}",
        encode_family(family)
    );
    let resp = http_client()
        .get(&url)
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("failed to fetch Google Fonts manifest for {family}"))?;
    let bytes = resp.bytes().await?;
    parse_manifest(bytes.as_ref())
        .with_context(|| format!("failed to parse manifest response from {url}"))
}

fn parse_manifest(bytes: &[u8]) -> anyhow::Result<Manifest> {
    let text = std::str::from_utf8(bytes)?;
    let trimmed = text
        .strip_prefix(")]}'")
        .map(|s| s.trim_start_matches('\n'))
        .unwrap_or(text);
    let parsed: ListResponse = serde_json::from_str(trimmed)?;

    if parsed.manifest.file_refs.is_empty() {
        anyhow::bail!("manifest contained no fileRefs");
    }

    Ok(parsed.manifest)
}

async fn download_font(entry: FileRef) -> anyhow::Result<PathBuf> {
    let base = Path::new(&entry.filename)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(entry.filename.as_str());
    let out_path = FONTS_DIR.join(base);

    if tokio::fs::metadata(&out_path).await.is_ok() {
        return Ok(out_path);
    }

    let data = http_client()
        .get(entry.url.as_str())
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    tokio::fs::write(&out_path, &data)
        .await
        .with_context(|| format!("failed to write font file {}", out_path.display()))?;

    Ok(out_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetch_and_extract_works() -> anyhow::Result<()> {
        tokio::fs::create_dir_all(&*FONTS_DIR).await?;

        let family = "Roboto";
        let fonts = fetch_and_extract(family).await?;
        assert!(!fonts.is_empty(), "should fetch some font files");
        for font in &fonts {
            assert!(
                font.exists(),
                "extracted font file should exist: {:?}",
                font
            );
        }

        Ok(())
    }
}
