use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct KoharuLayoutRFDetrSeg2XLConfig {
    pub architecture: String,
    pub rfdetr_version: String,
    pub resolution: i64,
    pub num_select: i64,
    pub classes: BTreeMap<String, String>,
    pub recommended_thresholds: KoharuLayoutThresholds,
    pub checkpoint: String,
    pub checkpoint_sha256: String,
    pub source_checkpoint_sha256: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct KoharuLayoutThresholds {
    pub text: f32,
    pub onomatopoeia: f32,
    pub bubble: f32,
    pub panel: f32,
}

impl KoharuLayoutThresholds {
    pub(super) fn with_background(self) -> [f32; 5] {
        [
            self.text,
            self.onomatopoeia,
            self.bubble,
            self.panel,
            f32::INFINITY,
        ]
    }
}

impl KoharuLayoutRFDetrSeg2XLConfig {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        serde_json::from_slice(
            &fs::read(path).with_context(|| format!("failed to read {}", path.display()))?,
        )
        .with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn class_names(&self) -> Vec<String> {
        (0..self.classes.len())
            .map(|index| self.classes[&index.to_string()].clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::KoharuLayoutThresholds;

    #[test]
    fn class_threshold_order_matches_checkpoint_labels() {
        let thresholds = KoharuLayoutThresholds {
            text: 0.1,
            onomatopoeia: 0.2,
            bubble: 0.3,
            panel: 0.4,
        };

        assert_eq!(
            thresholds.with_background(),
            [0.1, 0.2, 0.3, 0.4, f32::INFINITY]
        );
    }
}
