use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result, anyhow, bail};
use image::DynamicImage;
use koharu_ml::{
    baberu_ocr::BaberuOcr,
    manga_ocr::MangaOcr,
    paddle_ocr_vl::{PaddleOCRVL, PaddleOCRVLTask},
};
use koharu_scene::{
    Authored, Geometry, LanguageTag, OcrAnalysis, Origin, Region, SourceText, TextDirection,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{finish, generation, producer};
use crate::{NodeInput, NodeOutput, OcrModel, Stage, scope::geometry_extents};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct MangaOcrConfig {}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct BaberuOcrConfig {}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PaddleOcrVl1_6Config {}

pub(super) enum Model {
    Manga(Arc<Mutex<MangaOcr>>),
    Baberu(Arc<Mutex<BaberuOcr>>),
    Paddle(Arc<Mutex<PaddleOCRVL>>),
}

impl Model {
    pub(super) async fn load(device: koharu_ml::Device, config: &OcrModel) -> Result<Self> {
        match config {
            OcrModel::MangaOcr(_) => Ok(Self::Manga(Arc::new(Mutex::new(
                MangaOcr::load(device).await?,
            )))),
            OcrModel::BaberuOcr(_) => Ok(Self::Baberu(Arc::new(Mutex::new(
                BaberuOcr::load(device).await?,
            )))),
            OcrModel::PaddleOcrVl1_6(_) => Ok(Self::Paddle(Arc::new(Mutex::new(
                PaddleOCRVL::load(device).await?,
            )))),
        }
    }

    pub(super) async fn run(&self, input: NodeInput) -> Result<NodeOutput> {
        let model_name = match self {
            Self::Manga(_) => "manga-ocr",
            Self::Baberu(_) => "baberu-ocr",
            Self::Paddle(_) => "paddleocr-vl-1.6",
        };
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
                let source_text = input.scene.component::<SourceText>(id, "default")?;
                if source_text
                    .as_ref()
                    .is_some_and(|value| matches!(value.text.origin, Origin::User))
                {
                    continue;
                }
                let is_text_region = input
                    .scene
                    .component::<Region>(id, "default")?
                    .is_some_and(|region| region.kind.as_str() == "dev.koharu.region.text");
                if !is_text_region && source_text.is_none() {
                    continue;
                }
                let geometry = input
                    .scene
                    .component::<Geometry>(id, "default")?
                    .ok_or_else(|| anyhow!("text entity {id} has no geometry"))?;
                let crop = crop(&source, &geometry)
                    .with_context(|| format!("text entity {id} is outside its source image"))?;
                targets.push((id, geometry, source_text, crop));
            }
        }

        let results = match self {
            Self::Manga(model) => {
                infer_text(model.clone(), targets, |model, image| {
                    model.inference(image)
                })
                .await?
            }
            Self::Baberu(model) => {
                infer_text(model.clone(), targets, |model, image| {
                    model.inference(image)
                })
                .await?
            }
            Self::Paddle(model) => {
                infer_text(model.clone(), targets, |model, image| {
                    Ok(model.inference(image, PaddleOCRVLTask::Ocr)?.text)
                })
                .await?
            }
        };

        if input.cancellation.is_cancelled() {
            bail!("OCR was cancelled");
        }
        let generation = generation(producer(Stage::Ocr), model_name)?;
        let mut edit = input.scene.edit_as(generation.clone());
        for (entity, geometry, previous, text) in results {
            let language = previous
                .and_then(|value| value.language)
                .or_else(|| LanguageTag::new("ja-JP").ok());
            edit.set_source_text(
                entity,
                SourceText {
                    text: Authored::generated(text, generation.clone()),
                    language,
                },
            )?;
            let (_, min_y, _, max_y) = geometry_extents(&geometry)
                .ok_or_else(|| anyhow!("text entity {entity} has empty geometry"))?;
            let (min_x, _, max_x, _) = geometry_extents(&geometry).unwrap();
            edit.set(
                entity,
                "default",
                &OcrAnalysis {
                    origin: Origin::Generated(generation.clone()),
                    direction: if max_y - min_y >= (max_x - min_x) * 1.15 {
                        TextDirection::Vertical
                    } else {
                        TextDirection::Horizontal
                    },
                    confidence: None,
                    line_boundaries: Vec::new(),
                },
            )?;
        }
        finish(edit)
    }
}

type Target = (
    koharu_scene::EntityId,
    Geometry,
    Option<SourceText>,
    DynamicImage,
);
type ResultTarget = (koharu_scene::EntityId, Geometry, Option<SourceText>, String);

async fn infer_text<M: Send + 'static>(
    model: Arc<Mutex<M>>,
    targets: Vec<Target>,
    inference: impl Fn(&M, &DynamicImage) -> Result<String> + Send + Sync + 'static,
) -> Result<Vec<ResultTarget>> {
    tokio::task::spawn_blocking(move || {
        let model = model
            .lock()
            .map_err(|_| anyhow!("OCR model lock is poisoned"))?;
        targets
            .into_iter()
            .map(|(entity, geometry, previous, image)| {
                Ok((entity, geometry, previous, inference(&model, &image)?))
            })
            .collect()
    })
    .await
    .context("OCR task panicked")?
}

fn crop(source: &DynamicImage, geometry: &Geometry) -> Result<DynamicImage> {
    let (min_x, min_y, max_x, max_y) =
        geometry_extents(geometry).ok_or_else(|| anyhow!("geometry is empty"))?;
    let x = min_x.floor().max(0.0) as u32;
    let y = min_y.floor().max(0.0) as u32;
    let right = max_x.ceil().max(0.0).min(f64::from(source.width())) as u32;
    let bottom = max_y.ceil().max(0.0).min(f64::from(source.height())) as u32;
    if right <= x || bottom <= y {
        bail!("geometry does not overlap the image");
    }
    Ok(source.crop_imm(x, y, right - x, bottom - y))
}
