use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow, bail, ensure};
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat};
use koharu_ml::manga_text_mask::{MangaTextMaskCleaningOptions, MangaTextMaskGenerator};
use koharu_scene::{PageAsset, PageId};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Artifact, Context, Processor};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct MangaTextMaskConfig {
    pub threshold: f32,
    pub max_side: Option<u32>,
    pub horizontal_flip: bool,
    pub vertical_flip: bool,
}

impl Default for MangaTextMaskConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            max_side: None,
            horizontal_flip: false,
            vertical_flip: false,
        }
    }
}

pub(super) struct MangaTextMaskProcessor {
    model: Arc<Mutex<MangaTextMaskGenerator>>,
    config: MangaTextMaskConfig,
}

impl MangaTextMaskProcessor {
    pub(super) async fn load(
        device: koharu_ml::Device,
        config: &MangaTextMaskConfig,
    ) -> Result<Self> {
        if config.max_side.is_some() && (config.horizontal_flip || config.vertical_flip) {
            bail!("manga text mask max_side and flip augmentation cannot be combined");
        }
        Ok(Self {
            model: Arc::new(Mutex::new(MangaTextMaskGenerator::load(device).await?)),
            config: config.clone(),
        })
    }
}

#[async_trait]
impl Processor for MangaTextMaskProcessor {
    fn name(&self) -> &'static str {
        "MangaTextMask"
    }

    fn inputs(&self) -> &'static [Artifact] {
        &[Artifact::SourceImage]
    }

    fn outputs(&self) -> &'static [Artifact] {
        &[Artifact::TextMaskCandidate]
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
                .map_err(|_| anyhow!("manga text mask model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let output = if let Some(max_side) = config.max_side {
                        model.inference_with_max_side(&input.image, max_side)?
                    } else if config.horizontal_flip || config.vertical_flip {
                        model.inference_with_tta(
                            &input.image,
                            config.horizontal_flip,
                            config.vertical_flip,
                        )?
                    } else {
                        model.inference(&input.image)?
                    };
                    let mask = output.process(&MangaTextMaskCleaningOptions {
                        threshold: config.threshold,
                        ..Default::default()
                    })?;
                    Ok((input, mask))
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
