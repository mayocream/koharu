use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, anyhow, bail};
use image::DynamicImage;
use koharu_ml::font_detector::{FontDetector, FontPrediction, TextDirection as FontDirection};
use koharu_scene::{Geometry, Origin, Region, SourceText, Typography, WritingMode};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{finish, generation, producer};
use crate::{NodeInput, NodeOutput, Stage, TypographyModel, scope::geometry_extents};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct FontDetectorConfig {
    #[specta(type = f64)]
    pub top_k: usize,
}

impl Default for FontDetectorConfig {
    fn default() -> Self {
        Self { top_k: 3 }
    }
}

pub(super) struct Model {
    model: Arc<Mutex<FontDetector>>,
    top_k: usize,
}

impl Model {
    pub(super) async fn load(device: koharu_ml::Device, config: &TypographyModel) -> Result<Self> {
        let TypographyModel::FontDetector(config) = config;
        Ok(Self {
            model: Arc::new(Mutex::new(FontDetector::load(device).await?)),
            top_k: config.top_k,
        })
    }

    pub(super) async fn run(&self, input: NodeInput) -> Result<NodeOutput> {
        let mut targets = Vec::new();
        for page in input.scope.pages() {
            let source = input
                .cache
                .image(&input.scene, *page, "source")?
                .ok_or_else(|| anyhow!("page {page} has no source image"))?;
            for entity in input.scene.descendants(*page)? {
                let id = entity.id();
                if !input.scope.contains_entity(&input.scene, id)? {
                    continue;
                }
                if input
                    .scene
                    .component::<Typography>(id, "default")?
                    .is_some_and(|value| matches!(value.origin, Origin::User))
                {
                    continue;
                }
                let is_text = input
                    .scene
                    .component::<Region>(id, "default")?
                    .is_some_and(|region| region.kind.as_str() == "dev.koharu.region.text")
                    || input
                        .scene
                        .component::<SourceText>(id, "default")?
                        .is_some();
                if !is_text {
                    continue;
                }
                let geometry = input
                    .scene
                    .component::<Geometry>(id, "default")?
                    .ok_or_else(|| anyhow!("text entity {id} has no geometry"))?;
                targets.push((id, crop(&source, &geometry)?));
            }
        }
        let images = targets
            .iter()
            .map(|(_, image)| image.clone())
            .collect::<Vec<_>>();
        let predictions = if images.is_empty() {
            Vec::new()
        } else {
            let model = self.model.clone();
            let top_k = self.top_k;
            tokio::task::spawn_blocking(move || {
                model
                    .lock()
                    .map_err(|_| anyhow!("font detector model lock is poisoned"))?
                    .inference(&images, top_k)
            })
            .await
            .context("font detector task panicked")??
        };
        if input.cancellation.is_cancelled() {
            bail!("typography detection was cancelled");
        }
        let generation = generation(producer(Stage::Typography), "font-detector")?;
        let mut edit = input.scene.edit_as(generation.clone());
        for ((entity, _), prediction) in targets.into_iter().zip(predictions) {
            edit.set(
                entity,
                "default",
                &typography(prediction, generation.clone()),
            )?;
        }
        finish(edit)
    }
}

fn typography(prediction: FontPrediction, generation: koharu_scene::Generation) -> Typography {
    let preferred_font = prediction.named_fonts.first().map(|font| font.name.clone());
    Typography {
        origin: Origin::Generated(generation),
        preferred_font,
        size: (prediction.font_size_px.is_finite() && prediction.font_size_px > 0.0)
            .then_some(prediction.font_size_px),
        alignment: None,
        writing_mode: Some(match prediction.direction {
            FontDirection::Horizontal => WritingMode::Horizontal,
            FontDirection::Vertical => WritingMode::Vertical,
        }),
        extensions: BTreeMap::new(),
    }
}

fn crop(source: &DynamicImage, geometry: &Geometry) -> Result<DynamicImage> {
    let (min_x, min_y, max_x, max_y) =
        geometry_extents(geometry).ok_or_else(|| anyhow!("geometry is empty"))?;
    let x = min_x.floor().max(0.0) as u32;
    let y = min_y.floor().max(0.0) as u32;
    let right = max_x.ceil().max(0.0).min(f64::from(source.width())) as u32;
    let bottom = max_y.ceil().max(0.0).min(f64::from(source.height())) as u32;
    if right <= x || bottom <= y {
        bail!("text geometry does not overlap its source image");
    }
    Ok(source.crop_imm(x, y, right - x, bottom - y))
}
