use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use koharu_ml::lama::{HDStrategy, InpaintRequest, LaMa};
use koharu_scene::{PageAsset, PageId};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Context, Processor};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LaMaHDStrategy {
    Original,
    Resize,
    #[default]
    Crop,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct LaMaConfig {
    pub hd_strategy: LaMaHDStrategy,
    pub hd_strategy_crop_trigger_size: u32,
    pub hd_strategy_crop_margin: u32,
    pub hd_strategy_resize_limit: u32,
    pub keep_unmasked_area: bool,
}

impl Default for LaMaConfig {
    fn default() -> Self {
        let request = InpaintRequest::default();
        Self {
            hd_strategy: LaMaHDStrategy::Crop,
            hd_strategy_crop_trigger_size: request.hd_strategy_crop_trigger_size,
            hd_strategy_crop_margin: request.hd_strategy_crop_margin,
            hd_strategy_resize_limit: request.hd_strategy_resize_limit,
            keep_unmasked_area: request.sd_keep_unmasked_area,
        }
    }
}

impl LaMaConfig {
    fn request(&self) -> InpaintRequest {
        InpaintRequest {
            hd_strategy: match self.hd_strategy {
                LaMaHDStrategy::Original => HDStrategy::Original,
                LaMaHDStrategy::Resize => HDStrategy::Resize,
                LaMaHDStrategy::Crop => HDStrategy::Crop,
            },
            hd_strategy_crop_trigger_size: self.hd_strategy_crop_trigger_size,
            hd_strategy_crop_margin: self.hd_strategy_crop_margin,
            hd_strategy_resize_limit: self.hd_strategy_resize_limit,
            sd_keep_unmasked_area: self.keep_unmasked_area,
        }
    }
}

pub(super) struct LaMaProcessor {
    model: Arc<Mutex<LaMa>>,
    request: InpaintRequest,
}

impl LaMaProcessor {
    pub(super) async fn load(device: koharu_ml::Device, config: &LaMaConfig) -> Result<Self> {
        Ok(Self {
            model: Arc::new(Mutex::new(LaMa::load(device).await?)),
            request: config.request(),
        })
    }
}

#[async_trait]
impl Processor for LaMaProcessor {
    async fn run(&mut self, context: &Context) -> Result<koharu_scene::Commands> {
        let inputs = context
            .pages()
            .iter()
            .map(|page| {
                let image = if context.region(page.id).is_some() {
                    context
                        .asset(page.id, PageAsset::Clean)?
                        .unwrap_or(context.source(page.id)?)
                } else {
                    context.source(page.id)?
                };
                let mut mask = GrayImage::new(page.size.width, page.size.height);
                for asset in [
                    PageAsset::TextMask,
                    PageAsset::CooMask,
                    PageAsset::BrushMask,
                ] {
                    if let Some(value) = context.asset(page.id, asset)? {
                        for (target, source) in
                            mask.as_mut().iter_mut().zip(value.to_luma8().as_raw())
                        {
                            *target = (*target).max(*source);
                        }
                    }
                }
                if let Some(region) = context.region(page.id) {
                    for (x, y, pixel) in mask.enumerate_pixels_mut() {
                        if x as f32 + 1.0 <= region.x
                            || y as f32 + 1.0 <= region.y
                            || x as f32 >= region.x + region.width
                            || y as f32 >= region.y + region.height
                        {
                            *pixel = Luma([0]);
                        }
                    }
                }
                Ok(InpaintInput {
                    page: page.id,
                    image,
                    mask,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let request = self.request.clone();
        let model = self.model.clone();
        let outputs = tokio::task::spawn_blocking(move || {
            let model = model
                .lock()
                .map_err(|_| anyhow!("LaMa model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let image = DynamicImage::ImageRgb8(model.inference(
                        &input.image,
                        &input.mask,
                        &request,
                    )?);
                    Ok((input.page, image))
                })
                .collect::<Result<Vec<_>>>()
        })
        .await??;
        let mut commands = context.commands();
        for (page, image) in outputs {
            let mut bytes = Cursor::new(Vec::new());
            image.write_to(&mut bytes, ImageFormat::Png)?;
            let bytes: Arc<[u8]> = Arc::from(bytes.into_inner());
            commands.set_asset(page, PageAsset::Clean, Some(bytes))?;
        }
        Ok(commands)
    }
}

struct InpaintInput {
    page: PageId,
    image: Arc<DynamicImage>,
    mask: GrayImage,
}
