use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use imageproc::{drawing::draw_polygon_mut, point::Point};
use koharu_ml::pp_doclayout_v3::{PPDocLayoutV3, PPDocLayoutV3Region};
use koharu_scene::{
    Command, Frame, ModelPrediction, PageAsset, PageId, SourceText, TextBlock, TextDirection,
    TextRole,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Artifact, Context, Processor};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct PPDocLayoutV3Config {
    pub confidence: f32,
}

impl Default for PPDocLayoutV3Config {
    fn default() -> Self {
        Self { confidence: 0.25 }
    }
}

pub(super) struct PPDocLayoutV3Processor {
    model: Arc<Mutex<PPDocLayoutV3>>,
    confidence: f32,
}

impl PPDocLayoutV3Processor {
    pub(super) async fn load(
        device: koharu_ml::Device,
        config: &PPDocLayoutV3Config,
    ) -> Result<Self> {
        Ok(Self {
            model: Arc::new(Mutex::new(PPDocLayoutV3::load(device).await?)),
            confidence: config.confidence,
        })
    }
}

#[async_trait]
impl Processor for PPDocLayoutV3Processor {
    fn name(&self) -> &'static str {
        "PPDocLayoutV3"
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
        let confidence = self.confidence;
        let model = self.model.clone();
        let outputs = tokio::task::spawn_blocking(move || {
            let model = model
                .lock()
                .map_err(|_| anyhow!("PPDocLayoutV3 model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let detections = model.inference(&input.image, confidence)?;
                    Ok((input, detections.regions))
                })
                .collect::<Result<Vec<_>>>()
        })
        .await??;

        let mut commands = context.commands();
        for (input, regions) in outputs {
            let page = context.page(input.page).expect("captured page");
            for element in &page.elements {
                if element.text().is_some_and(|text| {
                    text.predictions
                        .iter()
                        .any(|prediction| prediction.model == "PPDocLayoutV3")
                }) && context.includes_element(input.page, element.id, element.frame)
                {
                    commands.push(Command::DeleteElement {
                        page: input.page,
                        element: element.id,
                    });
                }
            }
            let mut texts = detected_texts(&regions, input.area).collect::<Vec<_>>();
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
                    TextBlock {
                        source: Some(text.source),
                        role: TextRole::FreeText,
                        predictions: vec![ModelPrediction::new("PPDocLayoutV3", text.score)],
                        ..TextBlock::default()
                    },
                );
            }

            let mask = polygon_mask(&regions, input.area);
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
            commands.set_asset(
                input.page,
                PageAsset::TextMaskCandidate,
                Some(Arc::<[u8]>::from(bytes.into_inner())),
            )?;
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
    score: f32,
}

fn detected_texts(
    regions: &[PPDocLayoutV3Region],
    area: PixelArea,
) -> impl Iterator<Item = DetectedText> + '_ {
    regions.iter().filter_map(move |region| {
        if !is_text_region(region) {
            return None;
        }
        let x1 = region.bbox[0].min(region.bbox[2]).max(0.0);
        let y1 = region.bbox[1].min(region.bbox[3]).max(0.0);
        let width = (region.bbox[0].max(region.bbox[2]) - x1).max(0.0);
        let height = (region.bbox[1].max(region.bbox[3]) - y1).max(0.0);
        Some(DetectedText {
            frame: Frame::new(x1 + area.x as f32, y1 + area.y as f32, width, height),
            source: SourceText {
                text: String::new(),
                language: None,
                direction: if height >= width * 1.15 {
                    TextDirection::Vertical
                } else {
                    TextDirection::Horizontal
                },
                confidence: Some(region.score.clamp(0.0, 1.0)),
                lines: Vec::new(),
            },
            score: region.score.clamp(0.0, 1.0),
        })
    })
}

fn polygon_mask(regions: &[PPDocLayoutV3Region], area: PixelArea) -> GrayImage {
    let mut mask = GrayImage::new(area.width, area.height);
    for region in regions.iter().filter(|region| is_text_region(region)) {
        let Some(points) = &region.polygon_points else {
            continue;
        };
        let points = points
            .iter()
            .map(|[x, y]| Point::new(x.round() as i32, y.round() as i32))
            .collect::<Vec<_>>();
        if points.len() >= 3 {
            draw_polygon_mut(&mut mask, &points, Luma([255]));
        }
    }
    mask
}

fn is_text_region(region: &PPDocLayoutV3Region) -> bool {
    let label = region.label.to_ascii_lowercase();
    if label != "content" && !label.contains("text") && !label.contains("title") {
        return false;
    }
    let x1 = region.bbox[0].min(region.bbox[2]).max(0.0);
    let y1 = region.bbox[1].min(region.bbox[3]).max(0.0);
    let width = (region.bbox[0].max(region.bbox[2]) - x1).max(0.0);
    let height = (region.bbox[1].max(region.bbox[3]) - y1).max(0.0);
    width >= 6.0 && height >= 6.0 && width * height >= 48.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygon_points_are_rasterized_as_a_text_mask() {
        let region = PPDocLayoutV3Region {
            order_seq: 0,
            label_id: 0,
            label: "text".to_owned(),
            score: 0.9,
            bbox: [2.0, 2.0, 12.0, 12.0],
            polygon_points: Some(vec![[2.0, 2.0], [12.0, 2.0], [12.0, 12.0], [2.0, 12.0]]),
        };

        let mask = polygon_mask(
            &[region],
            PixelArea {
                x: 0,
                y: 0,
                width: 16,
                height: 16,
            },
        );

        assert_eq!(mask.get_pixel(7, 7), &Luma([255]));
        assert_eq!(mask.get_pixel(0, 0), &Luma([0]));
    }
}
