use std::sync::{Arc, Mutex};

use super::{ModelRef, StageInput, StageProcessor, finish, generation, observe_page_hierarchy};
use crate::{ModelCell, OcrModel, scope::geometry_extents};
use anyhow::{Context as _, Result, anyhow, bail};
use async_trait::async_trait;
use image::DynamicImage;
use koharu_ml::{
    baberu_ocr::BaberuOcr,
    manga_ocr::MangaOcr,
    paddle_ocr_vl::{PaddleOCRVL, PaddleOCRVLTask},
};
use koharu_scene::{
    Asset, Authored, Geometry, LanguageTag, OcrAnalysis, Origin, Region, SourceText, TextDirection,
};

const PRODUCER: &str = "dev.koharu.pipeline.ocr";

pub(super) struct Processor {
    config: OcrModel,
    device: koharu_ml::Device,
    model: ModelCell<Model>,
}

impl Processor {
    pub(super) fn new(config: OcrModel, device: koharu_ml::Device) -> Self {
        Self {
            config,
            device,
            model: ModelCell::new(),
        }
    }
}

#[async_trait]
impl StageProcessor for Processor {
    fn model(&self) -> ModelRef<'_> {
        let name = match self.config {
            OcrModel::MangaOcr => "manga-ocr",
            OcrModel::BaberuOcr => "baberu-ocr",
            OcrModel::PaddleOcrVl1_6 => "paddleocr-vl-1.6",
        };
        ModelRef::new(name, &self.model)
    }

    async fn load(&self) -> Result<()> {
        self.model
            .ensure(|| Model::load(self.device.clone(), &self.config))
            .await
    }

    async fn process(&self, input: StageInput) -> Result<koharu_scene::ScenePatch> {
        self.model
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| anyhow!("OCR model is not loaded"))?
            .run(input)
            .await
    }
}

enum Model {
    Manga(Arc<Mutex<MangaOcr>>),
    Baberu(Arc<Mutex<BaberuOcr>>),
    Paddle(Arc<Mutex<PaddleOCRVL>>),
}

impl Model {
    async fn load(device: koharu_ml::Device, config: &OcrModel) -> Result<Self> {
        match config {
            OcrModel::MangaOcr => Ok(Self::Manga(Arc::new(Mutex::new(
                MangaOcr::load(device).await?,
            )))),
            OcrModel::BaberuOcr => Ok(Self::Baberu(Arc::new(Mutex::new(
                BaberuOcr::load(device).await?,
            )))),
            OcrModel::PaddleOcrVl1_6 => Ok(Self::Paddle(Arc::new(Mutex::new(
                PaddleOCRVL::load(device).await?,
            )))),
        }
    }

    async fn run(&self, input: StageInput) -> Result<koharu_scene::ScenePatch> {
        let model_name = match self {
            Self::Manga(_) => "manga-ocr",
            Self::Baberu(_) => "baberu-ocr",
            Self::Paddle(_) => "paddleocr-vl-1.6",
        };
        let page = input.page;
        let mut targets = Vec::new();
        let source = input
            .images
            .get(&input.scene, page, "source")?
            .ok_or_else(|| anyhow!("page {page} has no source image"))?;
        for entity in input.scene.descendants(page)? {
            let id = entity.id();
            if !input.contains_entity(id)? {
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

        let generation = generation(PRODUCER, model_name)?;
        let mut edit = input.scene.edit_as(generation.clone());
        edit.observe::<Asset>(page, "source")?;
        for entity in observe_page_hierarchy(&mut edit, &input.scene, page)? {
            edit.observe::<Region>(entity, "default")?;
            edit.observe::<SourceText>(entity, "default")?;
            edit.observe::<Geometry>(entity, "default")?;
        }
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
                Ok((
                    entity,
                    geometry,
                    previous,
                    normalize_ocr_text(inference(&model, &image)?),
                ))
            })
            .collect()
    })
    .await
    .context("OCR task panicked")?
}

// Manga OCR can emit replacement-box glyphs for an isolated Japanese ellipsis.
// Normalize only an all-placeholder sequence so ordinary OCR output is preserved.
fn normalize_ocr_text(text: String) -> String {
    let visible = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<Vec<_>>();
    if visible.len() >= 2
        && visible
            .iter()
            .all(|character| matches!(character, '☐' | '□' | '▢' | '▣' | '�'))
    {
        "…".to_owned()
    } else {
        text
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
        bail!("geometry does not overlap the image");
    }
    Ok(source.crop_imm(x, y, right - x, bottom - y))
}

#[cfg(test)]
mod tests {
    use super::normalize_ocr_text;

    #[test]
    fn repeated_placeholder_glyphs_are_an_ellipsis() {
        assert_eq!(normalize_ocr_text("☐ ☐ ☐".to_owned()), "…");
        assert_eq!(normalize_ocr_text("□\n□".to_owned()), "…");
    }

    #[test]
    fn ordinary_text_and_single_boxes_are_unchanged() {
        assert_eq!(normalize_ocr_text("待って…".to_owned()), "待って…");
        assert_eq!(normalize_ocr_text("☐".to_owned()), "☐");
    }
}
