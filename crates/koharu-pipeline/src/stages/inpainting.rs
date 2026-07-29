use std::{
    collections::BTreeMap,
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use async_trait::async_trait;
use image::{DynamicImage, GenericImageView as _, GrayImage, ImageFormat, Luma};
use koharu_ml::{
    aot_inpainting::AotInpainting,
    flux2_klein::{Flux2KleinInpaint, Flux2KleinInpaintOptions},
    lama::{HDStrategy, InpaintRequest, LaMa},
    rorem_mixed::{DEFAULT_NEGATIVE_PROMPT, DEFAULT_PROMPT, RoremMixed, RoremMixedOptions},
};
use koharu_scene::{Asset, AssetInput, AssetMetadata, AssetRole};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{ModelRef, StageInput, StageProcessor, finish, generation};
use crate::{InpaintingModel, ModelCell};

const PRODUCER: &str = "dev.koharu.pipeline.inpainting";

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LaMaHDStrategy {
    Original,
    Resize,
    #[default]
    Crop,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct AotInpaintingConfig {
    pub max_side: u32,
}

impl Default for AotInpaintingConfig {
    fn default() -> Self {
        Self { max_side: 2048 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct Flux2KleinConfig {
    pub prompt: String,
    pub padding_mask_crop: Option<u32>,
    pub strength: f64,
    #[specta(type = f64)]
    pub num_inference_steps: usize,
    #[specta(type = f64)]
    pub seed: i64,
}

impl Default for Flux2KleinConfig {
    fn default() -> Self {
        let options = Flux2KleinInpaintOptions::default();
        Self {
            prompt: "Remove the text and reconstruct the background.".to_owned(),
            padding_mask_crop: options.padding_mask_crop,
            strength: options.strength,
            num_inference_steps: options.num_inference_steps,
            seed: options.seed,
        }
    }
}

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
    #[specta(type = f64)]
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

pub(super) struct Processor {
    config: InpaintingModel,
    device: koharu_ml::Device,
    model: ModelCell<Model>,
}

impl Processor {
    pub(super) fn new(config: InpaintingModel, device: koharu_ml::Device) -> Result<Self> {
        match &config {
            InpaintingModel::LaMa(settings) => {
                ensure!(
                    settings.hd_strategy_crop_trigger_size > 0,
                    "LaMa crop trigger must be positive"
                );
                ensure!(
                    settings.hd_strategy_resize_limit > 0,
                    "LaMa resize limit must be positive"
                );
            }
            InpaintingModel::AotInpainting(settings) => {
                ensure!(
                    settings.max_side > 0,
                    "AOT max_side must be greater than zero"
                );
            }
            InpaintingModel::Flux2Klein(settings) => {
                ensure!(
                    !settings.prompt.contains('\0'),
                    "FLUX.2 prompt contains NUL"
                );
                ensure!(
                    settings.strength.is_finite()
                        && settings.strength > 0.0
                        && settings.strength <= 1.0,
                    "FLUX.2 strength must be finite and in (0, 1]"
                );
                ensure!(
                    settings.num_inference_steps > 0,
                    "FLUX.2 inference steps must be positive"
                );
            }
            InpaintingModel::RoremMixed(settings) => {
                ensure!(
                    matches!(settings.resolution, 512 | 1024),
                    "RORem resolution must be 512 or 1024"
                );
                ensure!(
                    settings.num_inference_steps > 0,
                    "RORem inference steps must be positive"
                );
                ensure!(
                    settings.guidance_scale.is_finite() && settings.guidance_scale > 0.0,
                    "RORem guidance must be finite and positive"
                );
                ensure!(
                    settings.strength.is_finite()
                        && settings.strength > 0.0
                        && settings.strength < 1.0,
                    "RORem strength must be finite and in (0, 1)"
                );
                ensure!(
                    !settings.prompt.contains('\0') && !settings.negative_prompt.contains('\0'),
                    "RORem prompt contains NUL"
                );
            }
        }

        Ok(Self {
            config,
            device,
            model: ModelCell::new(),
        })
    }
}

#[async_trait]
impl StageProcessor for Processor {
    fn model(&self) -> ModelRef<'_> {
        let name = match self.config {
            InpaintingModel::LaMa(_) => "lama",
            InpaintingModel::AotInpainting(_) => "aot-inpainting",
            InpaintingModel::Flux2Klein(_) => "flux2-klein",
            InpaintingModel::RoremMixed(_) => "rorem-mixed",
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
            .ok_or_else(|| anyhow!("inpainting model is not loaded"))?
            .run(input)
            .await
    }
}

enum Model {
    LaMa {
        model: Arc<Mutex<LaMa>>,
        request: InpaintRequest,
    },
    Aot {
        model: Arc<Mutex<AotInpainting>>,
        max_side: u32,
    },
    Flux {
        model: Arc<Mutex<Flux2KleinInpaint>>,
        config: Flux2KleinConfig,
    },
    Rorem {
        model: Arc<Mutex<RoremMixed>>,
        config: RoremMixedConfig,
    },
}

impl Model {
    async fn load(device: koharu_ml::Device, config: &InpaintingModel) -> Result<Self> {
        match config {
            InpaintingModel::LaMa(config) => Ok(Self::LaMa {
                model: Arc::new(Mutex::new(LaMa::load(device).await?)),
                request: config.request(),
            }),
            InpaintingModel::AotInpainting(config) => Ok(Self::Aot {
                model: Arc::new(Mutex::new(AotInpainting::load(device).await?)),
                max_side: config.max_side,
            }),
            InpaintingModel::Flux2Klein(config) => Ok(Self::Flux {
                model: Arc::new(Mutex::new(Flux2KleinInpaint::load(device).await?)),
                config: config.clone(),
            }),
            InpaintingModel::RoremMixed(config) => Ok(Self::Rorem {
                model: Arc::new(Mutex::new(RoremMixed::load(device).await?)),
                config: config.clone(),
            }),
        }
    }

    async fn run(&self, input: StageInput) -> Result<koharu_scene::ScenePatch> {
        let prepared = prepare(&input)?;
        if prepared.mask.as_raw().iter().all(|value| *value == 0) {
            return finish(input.scene.edit());
        }
        let (model_name, image) = match self {
            Self::LaMa { model, request } => {
                let model = model.clone();
                let request = request.clone();
                (
                    "lama",
                    tokio::task::spawn_blocking(move || -> Result<DynamicImage> {
                        let model = model
                            .lock()
                            .map_err(|_| anyhow!("LaMa model lock is poisoned"))?;
                        Ok(DynamicImage::ImageRgb8(model.inference(
                            &prepared.image,
                            &prepared.mask,
                            &request,
                        )?))
                    })
                    .await
                    .context("LaMa task panicked")??,
                )
            }
            Self::Aot { model, max_side } => {
                let model = model.clone();
                let max_side = *max_side;
                (
                    "aot-inpainting",
                    tokio::task::spawn_blocking(move || -> Result<DynamicImage> {
                        let model = model
                            .lock()
                            .map_err(|_| anyhow!("AOT model lock is poisoned"))?;
                        Ok(DynamicImage::ImageRgb8(model.inference_with_max_side(
                            &prepared.image,
                            &prepared.mask,
                            max_side,
                        )?))
                    })
                    .await
                    .context("AOT task panicked")??,
                )
            }
            Self::Flux { model, config } => {
                let model = model.clone();
                let config = config.clone();
                (
                    "flux2-klein",
                    tokio::task::spawn_blocking(move || -> Result<DynamicImage> {
                        let model = model
                            .lock()
                            .map_err(|_| anyhow!("FLUX model lock is poisoned"))?;
                        let width = prepared.image.width();
                        let height = prepared.image.height();
                        let mut image = model.inference(
                            &config.prompt,
                            &prepared.image,
                            None,
                            &DynamicImage::ImageLuma8(prepared.mask),
                            &Flux2KleinInpaintOptions {
                                padding_mask_crop: config.padding_mask_crop,
                                strength: config.strength,
                                num_inference_steps: config.num_inference_steps,
                                seed: config.seed,
                            },
                        )?;
                        if image.width() != width || image.height() != height {
                            image = image.resize_exact(
                                width,
                                height,
                                image::imageops::FilterType::Lanczos3,
                            );
                        }
                        Ok(image)
                    })
                    .await
                    .context("FLUX task panicked")??,
                )
            }
            Self::Rorem { model, config } => {
                let model = model.clone();
                let config = config.clone();
                (
                    "rorem-mixed",
                    tokio::task::spawn_blocking(move || -> Result<DynamicImage> {
                        let model = model
                            .lock()
                            .map_err(|_| anyhow!("RORem model lock is poisoned"))?;
                        Ok(DynamicImage::ImageRgb8(model.inference(
                            &prepared.image,
                            &prepared.mask,
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
                        )?))
                    })
                    .await
                    .context("RORem task panicked")??,
                )
            }
        };
        let generation = generation(PRODUCER, model_name)?;
        let page = input.page;
        let mut edit = input.scene.edit_as(generation);
        edit.observe::<Asset>(page, "source")?;
        if input.region.is_some() {
            edit.observe::<Asset>(page, "clean")?;
        }
        for role in ["text-mask", "coo-mask", "brush-mask"] {
            edit.observe::<Asset>(page, role)?;
        }
        let image = if let Some(bounds) = input.region {
            preserve_outside(&input, page, bounds, image)?
        } else {
            image
        };
        let mut bytes = Cursor::new(Vec::new());
        let width = image.width();
        let height = image.height();
        image.write_to(&mut bytes, ImageFormat::Png)?;
        edit.set_asset(
            page,
            &AssetRole::new("clean")?,
            AssetInput::new(
                Arc::<[u8]>::from(bytes.into_inner()),
                "image/png",
                AssetMetadata {
                    width: Some(width),
                    height: Some(height),
                    attributes: BTreeMap::new(),
                },
            ),
        )?;
        finish(edit)
    }
}

fn preserve_outside(
    input: &StageInput,
    page: koharu_scene::EntityId,
    bounds: crate::Bounds,
    image: DynamicImage,
) -> Result<DynamicImage> {
    let base = input
        .images
        .get(&input.scene, page, "clean")?
        .or(input.images.get(&input.scene, page, "source")?)
        .ok_or_else(|| anyhow!("page {page} has no source image"))?;
    if base.dimensions() != image.dimensions() {
        bail!("inpainted image dimensions do not match page {page}");
    }
    let base = base.to_rgba8();
    let mut image = image.to_rgba8();
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        if f64::from(x + 1) <= bounds.x
            || f64::from(y + 1) <= bounds.y
            || f64::from(x) >= bounds.x + bounds.width
            || f64::from(y) >= bounds.y + bounds.height
        {
            *pixel = *base.get_pixel(x, y);
        }
    }
    Ok(DynamicImage::ImageRgba8(image))
}

struct InpaintInput {
    image: Arc<DynamicImage>,
    mask: GrayImage,
}

fn prepare(input: &StageInput) -> Result<InpaintInput> {
    let page = input.page;
    let source = if input.region.is_some() {
        input
            .images
            .get(&input.scene, page, "clean")?
            .or(input.images.get(&input.scene, page, "source")?)
    } else {
        input.images.get(&input.scene, page, "source")?
    }
    .ok_or_else(|| anyhow!("page {page} has no source image"))?;
    let mut mask = GrayImage::new(source.width(), source.height());
    for role in ["text-mask", "coo-mask", "brush-mask"] {
        if let Some(image) = input.images.get(&input.scene, page, role)? {
            let layer = image.to_luma8();
            if layer.dimensions() != mask.dimensions() {
                bail!("{role} dimensions do not match page {page}");
            }
            for (target, source) in mask.as_mut().iter_mut().zip(layer.as_raw()) {
                *target = (*target).max(*source);
            }
        }
    }
    if let Some(bounds) = input.region {
        for (x, y, pixel) in mask.enumerate_pixels_mut() {
            if f64::from(x + 1) <= bounds.x
                || f64::from(y + 1) <= bounds.y
                || f64::from(x) >= bounds.x + bounds.width
                || f64::from(y) >= bounds.y + bounds.height
            {
                *pixel = Luma([0]);
            }
        }
    }
    Ok(InpaintInput {
        image: source,
        mask,
    })
}
