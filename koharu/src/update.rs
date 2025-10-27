use std::sync::mpsc::Sender;

use tracing::info;
use velopack::{Error, VelopackAsset, VelopackAssetFeed, download, sources::UpdateSource};

#[derive(Clone)]
pub struct GithubSource {
    owner: String,
    repo: String,
}

impl GithubSource {
    pub fn new<S: AsRef<str>>(owner: S, repo: S, _asset_name: S) -> Self {
        Self {
            owner: owner.as_ref().to_string(),
            repo: repo.as_ref().to_string(),
        }
    }
}

impl UpdateSource for GithubSource {
    fn get_release_feed(
        &self,
        _channel: &str,
        _app: &velopack::bundle::Manifest,
        _staged_user_id: &str,
    ) -> Result<VelopackAssetFeed, Error> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/releases",
            self.owner, self.repo
        );
        info!("Fetching release feed from {}", url);

        let json = download::download_url_as_string(&url)?;
        let releases: Vec<serde_json::Value> = serde_json::from_str(&json)?;

        let assets = releases
            .iter()
            .filter(|r| !r["draft"].as_bool().unwrap_or(false))
            .filter_map(|release| {
                let tag_name = release["tag_name"].as_str()?;
                let version_str = tag_name.trim_start_matches('v');
                let version = semver::Version::parse(version_str).ok()?;

                let nupkg_asset = release["assets"].as_array()?.iter().find(|asset| {
                    asset["name"]
                        .as_str()
                        .map(|name| name.ends_with(".nupkg") && name.contains("full"))
                        .unwrap_or(false)
                })?;

                Some(VelopackAsset {
                    PackageId: String::new(),
                    Version: version.to_string(),
                    Type: "Full".to_string(),
                    FileName: nupkg_asset["name"].as_str().unwrap_or("").to_string(),
                    SHA1: String::new(),
                    SHA256: String::new(),
                    Size: nupkg_asset["size"].as_u64().unwrap_or(0),
                    NotesMarkdown: release["body"].as_str().unwrap_or("").to_string(),
                    NotesHtml: String::new(),
                })
            })
            .collect();

        Ok(VelopackAssetFeed { Assets: assets })
    }

    fn download_release_entry(
        &self,
        asset: &VelopackAsset,
        local_file: &str,
        progress_sender: Option<Sender<i16>>,
    ) -> Result<(), Error> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/releases",
            self.owner, self.repo
        );
        let json = download::download_url_as_string(&url)?;
        let releases: Vec<serde_json::Value> = serde_json::from_str(&json)?;

        let download_url = releases
            .iter()
            .find(|release| {
                release["tag_name"]
                    .as_str()
                    .map(|tag| tag.trim_start_matches('v'))
                    == Some(&asset.Version)
            })
            .and_then(|release| release["assets"].as_array())
            .and_then(|assets| {
                assets.iter().find(|asset| {
                    asset["name"]
                        .as_str()
                        .map(|name| name.ends_with(".nupkg") && name.contains("full"))
                        .unwrap_or(false)
                })
            })
            .and_then(|asset| asset["browser_download_url"].as_str())
            .ok_or_else(|| {
                Error::FileNotFound(format!("nupkg file for version {}", asset.Version))
            })?;

        info!("Downloading from: {}", download_url);

        download::download_url_to_file(download_url, local_file, move |p| {
            if let Some(sender) = &progress_sender {
                let _ = sender.send(p);
            }
        })
    }

    fn clone_boxed(&self) -> Box<dyn UpdateSource> {
        Box::new(self.clone())
    }
}
