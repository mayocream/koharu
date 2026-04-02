use std::path::PathBuf;

use anyhow::{Context, Result};
use koharu_core::TextBlock;
use serde::{Deserialize, Serialize};

/// Per-page manifest stored as JSON at `~/.koharu/pages/{id}.json`.
///
/// JSON is used instead of TOML because `TextBlock` contains types that TOML
/// cannot represent (tuples in `FontPrediction::top_fonts`, fixed-size arrays
/// in `line_polygons`, binary image data in `rendered`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageManifest {
    pub id: String,
    /// Blob hash of the original image.
    pub source: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub layers: LayerRefs,
    #[serde(default)]
    pub text_blocks: Vec<TextBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayerRefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inpainted: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brush_layer: Option<String>,
}

/// Manages manifest files at `~/.koharu/pages/`.
#[derive(Clone)]
pub struct ManifestStore {
    root: PathBuf,
}

impl ManifestStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn save(&self, manifest: &PageManifest) -> Result<()> {
        let path = self.manifest_path(&manifest.id);
        let content =
            serde_json::to_string_pretty(manifest).context("Failed to serialize manifest")?;
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write manifest {}", manifest.id))?;
        Ok(())
    }

    pub fn load(&self, id: &str) -> Result<PageManifest> {
        let path = self.manifest_path(id);
        let content =
            std::fs::read_to_string(&path).with_context(|| format!("Manifest not found: {id}"))?;
        serde_json::from_str(&content).with_context(|| format!("Failed to parse manifest {id}"))
    }

    pub fn load_all(&self) -> Result<Vec<PageManifest>> {
        let mut manifests = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json")
                    && let Ok(content) = std::fs::read_to_string(&path)
                    && let Ok(manifest) = serde_json::from_str::<PageManifest>(&content)
                {
                    manifests.push(manifest);
                }
            }
        }
        Ok(manifests)
    }

    pub fn replace_all(&self, manifests: &[PageManifest]) -> Result<()> {
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    std::fs::remove_file(&path)
                        .with_context(|| format!("Failed to remove manifest {}", path.display()))?;
                }
            }
        }

        for manifest in manifests {
            self.save(manifest)?;
        }

        Ok(())
    }

    fn manifest_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.json"))
    }
}
