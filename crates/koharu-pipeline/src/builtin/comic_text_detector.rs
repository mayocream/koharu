use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow, bail, ensure};
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat};
use koharu_ml::comic_text_detector::{ComicTextDetector, TextBlock};
use koharu_scene::{
    Command, Frame, ModelPrediction, PageAsset, PageId, SourceText, TextBlock as SceneTextBlock,
    TextDirection, TextRole,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Artifact, Context, Processor};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct ComicTextDetectorConfig {}

pub(super) struct ComicTextDetectorProcessor {
    model: Arc<Mutex<ComicTextDetector>>,
}

impl ComicTextDetectorProcessor {
    pub(super) async fn load(
        device: koharu_ml::Device,
        _config: &ComicTextDetectorConfig,
    ) -> Result<Self> {
        Ok(Self {
            model: Arc::new(Mutex::new(ComicTextDetector::load(device).await?)),
        })
    }
}

#[async_trait]
impl Processor for ComicTextDetectorProcessor {
    fn name(&self) -> &'static str {
        "ComicTextDetector"
    }

    fn inputs(&self) -> &'static [Artifact] {
        &[Artifact::SourceImage]
    }

    fn outputs(&self) -> &'static [Artifact] {
        &[Artifact::TextRegion, Artifact::TextMaskCandidate]
    }

    async fn run(&mut self, context: &Context) -> Result<koharu_scene::Commands> {
        let inputs = context
            .pages()
            .iter()
            .map(|page| {
                let source = context.source(page.id)?;
                let area = if let Some(region) = context.region(page.id) {
                    let x = (region.x.floor().max(0.0) as u32).min(source.width());
                    let y = (region.y.floor().max(0.0) as u32).min(source.height());
                    let right =
                        ((region.x + region.width).ceil().max(0.0) as u32).min(source.width());
                    let bottom =
                        ((region.y + region.height).ceil().max(0.0) as u32).min(source.height());
                    if right <= x || bottom <= y {
                        bail!("pipeline region does not overlap page {}", page.id);
                    }
                    PixelArea {
                        x,
                        y,
                        width: right - x,
                        height: bottom - y,
                    }
                } else {
                    PixelArea {
                        x: 0,
                        y: 0,
                        width: source.width(),
                        height: source.height(),
                    }
                };
                let image = if area.x == 0
                    && area.y == 0
                    && area.width == source.width()
                    && area.height == source.height()
                {
                    source
                } else {
                    Arc::new(source.crop_imm(area.x, area.y, area.width, area.height))
                };
                Ok(PageInput {
                    page: page.id,
                    image,
                    area,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let model = self.model.clone();
        let outputs = tokio::task::spawn_blocking(move || {
            let model = model
                .lock()
                .map_err(|_| anyhow!("comic text detector model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let (mask, blocks) = model.inference(&input.image)?;
                    Ok((input, mask, blocks))
                })
                .collect::<Result<Vec<_>>>()
        })
        .await??;

        let mut commands = context.commands();
        for (input, mask, blocks) in outputs {
            let page = context.page(input.page).expect("captured page");
            for element in &page.elements {
                if element.text().is_some_and(|text| {
                    text.predictions
                        .iter()
                        .any(|prediction| prediction.model == "ComicTextDetector")
                }) && context.includes_element(input.page, element.id, element.frame)
                {
                    commands.push(Command::DeleteElement {
                        page: input.page,
                        element: element.id,
                    });
                }
            }
            let mut texts = blocks
                .into_iter()
                .filter_map(|block| detected_text(block, input.area))
                .collect::<Vec<_>>();
            texts.sort_by(|left, right| {
                left.frame
                    .y
                    .total_cmp(&right.frame.y)
                    .then_with(|| right.frame.x.total_cmp(&left.frame.x))
            });
            for text in texts {
                commands.add_text_block(
                    input.page,
                    text.frame,
                    SceneTextBlock {
                        source: Some(text.source),
                        role: TextRole::FreeText,
                        predictions: vec![ModelPrediction::new("ComicTextDetector", 1.0)],
                        ..SceneTextBlock::default()
                    },
                );
            }

            ensure!(
                mask.dimensions() == (input.area.width, input.area.height),
                "model mask dimensions do not match its input"
            );
            let mask = if input.area.x == 0
                && input.area.y == 0
                && input.area.width == page.size.width
                && input.area.height == page.size.height
            {
                mask
            } else {
                let mut full = context
                    .asset(input.page, PageAsset::TextMaskCandidate)?
                    .map(|image| image.to_luma8())
                    .unwrap_or_else(|| GrayImage::new(page.size.width, page.size.height));
                image::imageops::replace(
                    &mut full,
                    &mask,
                    i64::from(input.area.x),
                    i64::from(input.area.y),
                );
                full
            };
            let mut bytes = Cursor::new(Vec::new());
            DynamicImage::ImageLuma8(mask).write_to(&mut bytes, ImageFormat::Png)?;
            let bytes: Arc<[u8]> = Arc::from(bytes.into_inner());
            commands.set_asset(input.page, PageAsset::TextMaskCandidate, Some(bytes))?;
        }
        Ok(commands)
    }
}

#[derive(Clone, Copy)]
struct PixelArea {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

struct PageInput {
    page: PageId,
    image: Arc<DynamicImage>,
    area: PixelArea,
}

struct DetectedText {
    frame: Frame,
    source: SourceText,
}

fn detected_text(block: TextBlock, area: PixelArea) -> Option<DetectedText> {
    let [x1, y1, x2, y2] = block.xyxy;
    let width = (x2 - x1) as f32;
    let height = (y2 - y1) as f32;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    Some(DetectedText {
        frame: Frame {
            x: x1 as f32 + area.x as f32,
            y: y1 as f32 + area.y as f32,
            width,
            height,
            angle_degrees: block.angle as f32,
        },
        source: SourceText {
            text: String::new(),
            language: (!block.language.eq_ignore_ascii_case("unknown")).then_some(block.language),
            direction: if block.vertical {
                TextDirection::Vertical
            } else {
                TextDirection::Horizontal
            },
            confidence: Some(1.0),
            lines: block
                .lines
                .into_iter()
                .map(|line| {
                    line.map(|point| {
                        [
                            point[0] as f32 + area.x as f32,
                            point[1] as f32 + area.y as f32,
                        ]
                    })
                })
                .collect(),
        },
    })
}
