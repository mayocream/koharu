use std::{
    collections::BTreeMap,
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, anyhow, bail};
use image::{DynamicImage, GenericImageView as _, GrayImage, ImageFormat, Luma};
use koharu_ml::{
    aot_inpainting::AotInpainting,
    flux2_klein::{Flux2KleinInpaint, Flux2KleinInpaintOptions},
    lama::{HDStrategy, InpaintRequest, LaMa},
    rorem_mixed::{DEFAULT_NEGATIVE_PROMPT, DEFAULT_PROMPT, RoremMixed, RoremMixedOptions},
};
use koharu_scene::{AssetInput, AssetMetadata, AssetRole};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{finish, generation, producer};
use crate::{InpaintingModel, NodeInput, NodeOutput, Stage};

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

pub(super) enum Model {
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
    pub(super) async fn load(device: koharu_ml::Device, config: &InpaintingModel) -> Result<Self> {
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

    pub(super) async fn run(&self, input: NodeInput) -> Result<NodeOutput> {
        let inputs = prepare(&input)?;
        let (model_name, outputs) = match self {
            Self::LaMa { model, request } => {
                let model = model.clone();
                let request = request.clone();
                (
                    "lama",
                    tokio::task::spawn_blocking(move || {
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
                    .await
                    .context("LaMa task panicked")??,
                )
            }
            Self::Aot { model, max_side } => {
                let model = model.clone();
                let max_side = *max_side;
                (
                    "aot-inpainting",
                    tokio::task::spawn_blocking(move || {
                        let model = model
                            .lock()
                            .map_err(|_| anyhow!("AOT model lock is poisoned"))?;
                        inputs
                            .into_iter()
                            .map(|input| {
                                let image =
                                    DynamicImage::ImageRgb8(model.inference_with_max_side(
                                        &input.image,
                                        &input.mask,
                                        max_side,
                                    )?);
                                Ok((input.page, image))
                            })
                            .collect::<Result<Vec<_>>>()
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
                    tokio::task::spawn_blocking(move || {
                        let model = model
                            .lock()
                            .map_err(|_| anyhow!("FLUX model lock is poisoned"))?;
                        inputs
                            .into_iter()
                            .map(|input| {
                                let width = input.image.width();
                                let height = input.image.height();
                                let mut image = model.inference(
                                    &config.prompt,
                                    &input.image,
                                    None,
                                    &DynamicImage::ImageLuma8(input.mask),
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
                                Ok((input.page, image))
                            })
                            .collect::<Result<Vec<_>>>()
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
                    tokio::task::spawn_blocking(move || {
                        let model = model
                            .lock()
                            .map_err(|_| anyhow!("RORem model lock is poisoned"))?;
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
                    .await
                    .context("RORem task panicked")??,
                )
            }
        };
        if input.cancellation.is_cancelled() {
            bail!("inpainting was cancelled");
        }
        let generation = generation(producer(Stage::Inpainting), model_name)?;
        let mut edit = input.scene.edit_as(generation);
        for (page, image) in outputs {
            let image = if let Some(bounds) = input.scope.region(page) {
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
        }
        finish(edit)
    }
}

fn preserve_outside(
    input: &NodeInput,
    page: koharu_scene::EntityId,
    bounds: crate::Bounds,
    image: DynamicImage,
) -> Result<DynamicImage> {
    let base = input
        .cache
        .image(&input.scene, page, "clean")?
        .or(input.cache.image(&input.scene, page, "source")?)
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
    page: koharu_scene::EntityId,
    image: Arc<DynamicImage>,
    mask: GrayImage,
}

fn prepare(input: &NodeInput) -> Result<Vec<InpaintInput>> {
    input
        .scope
        .pages()
        .iter()
        .map(|page| {
            let source = if input.scope.region(*page).is_some() {
                input
                    .cache
                    .image(&input.scene, *page, "clean")?
                    .or(input.cache.image(&input.scene, *page, "source")?)
            } else {
                input.cache.image(&input.scene, *page, "source")?
            }
            .ok_or_else(|| anyhow!("page {page} has no source image"))?;
            let mut mask = GrayImage::new(source.width(), source.height());
            for role in ["text-mask", "coo-mask", "brush-mask"] {
                if let Some(image) = input.cache.image(&input.scene, *page, role)? {
                    let layer = image.to_luma8();
                    if layer.dimensions() != mask.dimensions() {
                        bail!("{role} dimensions do not match page {page}");
                    }
                    for (target, source) in mask.as_mut().iter_mut().zip(layer.as_raw()) {
                        *target = (*target).max(*source);
                    }
                }
            }
            if let Some(bounds) = input.scope.region(*page) {
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
                page: *page,
                image: source,
                mask,
            })
        })
        .collect()
}
