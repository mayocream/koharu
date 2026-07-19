use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow, bail, ensure};
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use koharu_ml::speech_bubble_yolov8m::{YoloV8mSpeechBubbleInstance, YoloV8mSpeechBubbleSegmenter};
use koharu_scene::{PageAsset, PageId};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Artifact, Context, Processor};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct YoloV8mSpeechBubbleConfig {
    pub confidence: Option<f32>,
    pub nms_iou: Option<f32>,
}

pub(super) struct YoloV8mSpeechBubbleProcessor {
    model: Arc<Mutex<YoloV8mSpeechBubbleSegmenter>>,
    config: YoloV8mSpeechBubbleConfig,
}

impl YoloV8mSpeechBubbleProcessor {
    pub(super) async fn load(
        device: koharu_ml::Device,
        config: &YoloV8mSpeechBubbleConfig,
    ) -> Result<Self> {
        if config.confidence.is_some() != config.nms_iou.is_some() {
            bail!("speech bubble confidence and nms_iou must be set together");
        }
        Ok(Self {
            model: Arc::new(Mutex::new(
                YoloV8mSpeechBubbleSegmenter::load(device).await?,
            )),
            config: config.clone(),
        })
    }
}

#[async_trait]
impl Processor for YoloV8mSpeechBubbleProcessor {
    fn name(&self) -> &'static str {
        "SpeechBubbleYoloV8m"
    }

    fn inputs(&self) -> &'static [Artifact] {
        &[Artifact::SourceImage]
    }

    fn outputs(&self) -> &'static [Artifact] {
        &[Artifact::BubbleMask]
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
        let config = self.config.clone();
        let model = self.model.clone();
        let outputs = tokio::task::spawn_blocking(move || {
            let model = model
                .lock()
                .map_err(|_| anyhow!("speech bubble mask model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let result = match (config.confidence, config.nms_iou) {
                        (Some(confidence), Some(nms_iou)) => {
                            model.inference_with_thresholds(&input.image, confidence, nms_iou)?
                        }
                        _ => model.inference(&input.image)?,
                    };
                    let mut regions = result.instances.iter().collect::<Vec<_>>();
                    regions.sort_by_key(|region| std::cmp::Reverse(region.area));
                    Ok((
                        input,
                        paint_mask(result.image_width, result.image_height, &regions),
                    ))
                })
                .collect::<Result<Vec<_>>>()
        })
        .await??;

        let mut commands = context.commands();
        for (input, mask) in outputs {
            ensure!(
                mask.dimensions() == (input.area.width, input.area.height),
                "model mask dimensions do not match its input"
            );
            let page = context.page(input.page).expect("captured page");
            let mask = if input.area.x == 0
                && input.area.y == 0
                && input.area.width == page.size.width
                && input.area.height == page.size.height
            {
                mask
            } else {
                let mut full = context
                    .asset(input.page, PageAsset::BubbleMask)?
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
            commands.set_asset(input.page, PageAsset::BubbleMask, Some(bytes))?;
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

fn paint_mask(width: u32, height: u32, regions: &[&YoloV8mSpeechBubbleInstance]) -> GrayImage {
    let mut mask = GrayImage::new(width, height);
    for (index, region) in regions.iter().take(255).enumerate() {
        if region.mask.is_empty() {
            continue;
        }
        let id = (index + 1) as u8;
        let source_width = region.mask.width as usize;
        let max_x = region.mask.width.min(width.saturating_sub(region.mask.x));
        let max_y = region.mask.height.min(height.saturating_sub(region.mask.y));
        for y in 0..max_y {
            for x in 0..max_x {
                if region.mask.pixels[y as usize * source_width + x as usize] != 0 {
                    mask.put_pixel(region.mask.x + x, region.mask.y + y, Luma([id]));
                }
            }
        }
    }
    mask
}
