use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow, ensure};
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use koharu_ml::rorem_mixed::{
    DEFAULT_NEGATIVE_PROMPT, DEFAULT_PROMPT, RoremMixed, RoremMixedOptions,
};
use koharu_scene::{PageAsset, PageId};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Artifact, Context, Processor};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct RoremMixedConfig {
    pub prompt: String,
    pub negative_prompt: String,
    pub resolution: u32,
    pub mask_dilation: u8,
    pub num_inference_steps: i32,
    pub guidance_scale: f32,
    pub strength: f32,
    pub seed: i64,
}

impl Default for RoremMixedConfig {
    fn default() -> Self {
        let options = RoremMixedOptions::default();
        Self {
            prompt: DEFAULT_PROMPT.to_owned(),
            negative_prompt: DEFAULT_NEGATIVE_PROMPT.to_owned(),
            resolution: options.resolution,
            mask_dilation: options.mask_dilation,
            num_inference_steps: options.num_inference_steps,
            guidance_scale: options.guidance_scale,
            strength: options.strength,
            seed: options.seed,
        }
    }
}

pub(super) struct RoremMixedProcessor {
    model: Arc<Mutex<RoremMixed>>,
    config: RoremMixedConfig,
}

impl RoremMixedProcessor {
    pub(super) async fn load(device: koharu_ml::Device, config: &RoremMixedConfig) -> Result<Self> {
        ensure!(
            matches!(config.resolution, 512 | 1024),
            "RORem mixed resolution must be 512 or 1024"
        );
        ensure!(
            config.num_inference_steps > 0,
            "RORem mixed inference steps must be greater than zero"
        );
        ensure!(
            config.guidance_scale.is_finite() && config.guidance_scale > 0.0,
            "RORem mixed guidance scale must be finite and greater than zero"
        );
        ensure!(
            config.strength.is_finite() && config.strength > 0.0 && config.strength < 1.0,
            "RORem mixed strength must be finite, greater than zero, and less than one"
        );
        ensure!(
            !config.prompt.contains('\0') && !config.negative_prompt.contains('\0'),
            "RORem mixed prompts cannot contain an interior NUL byte"
        );
        Ok(Self {
            model: Arc::new(Mutex::new(RoremMixed::load(device).await?)),
            config: config.clone(),
        })
    }
}

#[async_trait]
impl Processor for RoremMixedProcessor {
    fn name(&self) -> &'static str {
        "RORem Mixed"
    }

    fn inputs(&self) -> &'static [Artifact] {
        &[
            Artifact::SourceImage,
            Artifact::TextMask,
            Artifact::CooMask,
            Artifact::BrushMask,
        ]
    }

    fn outputs(&self) -> &'static [Artifact] {
        &[Artifact::CleanImage]
    }

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
        let config = self.config.clone();
        let model = self.model.clone();
        let outputs = tokio::task::spawn_blocking(move || {
            let model = model
                .lock()
                .map_err(|_| anyhow!("RORem mixed model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let image = DynamicImage::ImageRgb8(model.inference(
                        &input.image,
                        &input.mask,
                        &config.prompt,
                        &config.negative_prompt,
                        &RoremMixedOptions {
                            resolution: config.resolution,
                            mask_dilation: config.mask_dilation,
                            num_inference_steps: config.num_inference_steps,
                            guidance_scale: config.guidance_scale,
                            strength: config.strength,
                            seed: config.seed,
                        },
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
