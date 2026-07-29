use std::{
    collections::BTreeMap,
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use async_trait::async_trait;
use image::{DynamicImage, GenericImageView as _, GrayImage, ImageFormat, Luma, Rgb, RgbImage};
use koharu_ml::{
    aot_inpainting::AotInpainting,
    flux2_klein::{Flux2KleinInpaint, Flux2KleinInpaintOptions},
    lama::{InpaintRequest, LaMa},
    rorem_mixed::{DEFAULT_NEGATIVE_PROMPT, DEFAULT_PROMPT, RoremMixed, RoremMixedOptions},
};
use koharu_scene::{Asset, AssetInput, AssetMetadata, AssetRole};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{ModelRef, StageInput, StageProcessor, finish, generation};
use crate::{InpaintingModel, ModelCell};

const PRODUCER: &str = "dev.koharu.pipeline.inpainting";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct Flux2KleinConfig {
    pub prompt: String,
}

impl Default for Flux2KleinConfig {
    fn default() -> Self {
        Self {
            prompt: "Remove the text and reconstruct the background.".to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct RoremMixedConfig {
    pub prompt: String,
    pub negative_prompt: String,
}

impl Default for RoremMixedConfig {
    fn default() -> Self {
        Self {
            prompt: DEFAULT_PROMPT.to_owned(),
            negative_prompt: DEFAULT_NEGATIVE_PROMPT.to_owned(),
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
            InpaintingModel::LaMa {} | InpaintingModel::AotInpainting {} => {}
            InpaintingModel::Flux2Klein(settings) => {
                ensure!(
                    !settings.prompt.contains('\0'),
                    "FLUX.2 prompt contains NUL"
                );
            }
            InpaintingModel::RoremMixed(settings) => {
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
            InpaintingModel::LaMa {} => "lama",
            InpaintingModel::AotInpainting {} => "aot-inpainting",
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
    LaMa(Arc<Mutex<LaMa>>),
    Aot(Arc<Mutex<AotInpainting>>),
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
            InpaintingModel::LaMa {} => {
                Ok(Self::LaMa(Arc::new(Mutex::new(LaMa::load(device).await?))))
            }
            InpaintingModel::AotInpainting {} => Ok(Self::Aot(Arc::new(Mutex::new(
                AotInpainting::load(device).await?,
            )))),
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
            Self::LaMa(model) => {
                let model = model.clone();
                (
                    "lama",
                    tokio::task::spawn_blocking(move || -> Result<DynamicImage> {
                        let model = model
                            .lock()
                            .map_err(|_| anyhow!("LaMa model lock is poisoned"))?;
                        inpaint_tiled(&prepared.image, &prepared.mask, |image, mask| {
                            Ok(DynamicImage::ImageRgb8(model.inference(
                                image,
                                mask,
                                &InpaintRequest::default(),
                            )?))
                        })
                    })
                    .await
                    .context("LaMa task panicked")??,
                )
            }
            Self::Aot(model) => {
                let model = model.clone();
                (
                    "aot-inpainting",
                    tokio::task::spawn_blocking(move || -> Result<DynamicImage> {
                        let model = model
                            .lock()
                            .map_err(|_| anyhow!("AOT model lock is poisoned"))?;
                        inpaint_tiled(&prepared.image, &prepared.mask, |image, mask| {
                            Ok(DynamicImage::ImageRgb8(model.inference(image, mask)?))
                        })
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
                        inpaint_tiled(&prepared.image, &prepared.mask, |image, mask| {
                            model.inference(
                                &config.prompt,
                                image,
                                None,
                                &DynamicImage::ImageLuma8(mask.clone()),
                                &Flux2KleinInpaintOptions::default(),
                            )
                        })
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
                        inpaint_tiled(&prepared.image, &prepared.mask, |image, mask| {
                            Ok(DynamicImage::ImageRgb8(model.inference(
                                image,
                                mask,
                                &config.prompt,
                                &config.negative_prompt,
                                &RoremMixedOptions::default(),
                            )?))
                        })
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

const TILE_SIZE: u32 = 512;
const TILE_CONTEXT: u32 = 128;
const UNIFORM_BACKGROUND_MARGIN: u32 = 32;
const UNIFORM_BACKGROUND_MIN_PIXELS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InpaintTile {
    core: [u32; 4],
    crop: [u32; 4],
}

// BallonsTranslator avoids model inference for a text block when the non-text
// pixels inside its balloon are nearly uniform, and otherwise sends an enlarged
// block crop to the inpainter:
// https://github.com/dmMaze/BallonsTranslator/blob/4bcc635c19f6c63a902872cf77b3d554e14ed1b7/ballontranslator/modules/inpaint/base.py#L168-L200
// The scene stage has masks but no BallonsTranslator text blocks, so mask-bearing
// grid cells are the equivalent blocks. Each model crop gets surrounding context,
// while compositing remains restricted to that cell's original mask pixels.
fn inpaint_tiled(
    image: &DynamicImage,
    mask: &GrayImage,
    mut inference: impl FnMut(&DynamicImage, &GrayImage) -> Result<DynamicImage>,
) -> Result<DynamicImage> {
    ensure!(
        image.dimensions() == mask.dimensions(),
        "image and mask dimensions differ: image={:?}, mask={:?}",
        image.dimensions(),
        mask.dimensions()
    );

    let mut output = image.to_rgb8();
    for tile in inpaint_tiles(mask) {
        let [left, top, right, bottom] = tile.crop;
        let crop_width = right - left;
        let crop_height = bottom - top;
        let crop_image =
            image::imageops::crop_imm(&output, left, top, crop_width, crop_height).to_image();
        let crop_mask = crop_tile_mask(mask, tile);

        if let Some(color) = uniform_background_color(&crop_image, &crop_mask) {
            composite_color(&mut output, mask, tile.core, color);
            continue;
        }

        let generated = inference(&DynamicImage::ImageRgb8(crop_image), &crop_mask)?;
        let generated = if generated.dimensions() == (crop_width, crop_height) {
            generated.to_rgb8()
        } else {
            generated
                .resize_exact(
                    crop_width,
                    crop_height,
                    image::imageops::FilterType::Lanczos3,
                )
                .to_rgb8()
        };
        composite_generated(&mut output, mask, tile, &generated);
    }
    Ok(DynamicImage::ImageRgb8(output))
}

fn inpaint_tiles(mask: &GrayImage) -> Vec<InpaintTile> {
    let mut tiles = Vec::new();
    let mut top = 0;
    while top < mask.height() {
        let bottom = top.saturating_add(TILE_SIZE).min(mask.height());
        let mut left = 0;
        while left < mask.width() {
            let right = left.saturating_add(TILE_SIZE).min(mask.width());
            if let Some([mask_left, mask_top, mask_right, mask_bottom]) =
                mask_bounds_in(mask, [left, top, right, bottom])
            {
                tiles.push(InpaintTile {
                    core: [left, top, right, bottom],
                    crop: [
                        mask_left.saturating_sub(TILE_CONTEXT),
                        mask_top.saturating_sub(TILE_CONTEXT),
                        mask_right.saturating_add(TILE_CONTEXT).min(mask.width()),
                        mask_bottom.saturating_add(TILE_CONTEXT).min(mask.height()),
                    ],
                });
            }
            left = right;
        }
        top = bottom;
    }
    tiles
}

fn crop_tile_mask(mask: &GrayImage, tile: InpaintTile) -> GrayImage {
    let [left, top, right, bottom] = tile.crop;
    let [core_left, core_top, core_right, core_bottom] = tile.core;
    let mut crop = GrayImage::new(right - left, bottom - top);
    for y in core_top..core_bottom {
        for x in core_left..core_right {
            if mask.get_pixel(x, y)[0] >= 127 {
                crop.put_pixel(x - left, y - top, Luma([u8::MAX]));
            }
        }
    }
    crop
}

fn uniform_background_color(image: &RgbImage, mask: &GrayImage) -> Option<Rgb<u8>> {
    let [left, top, right, bottom] = mask_bounds(mask)?;
    let left = left.saturating_sub(UNIFORM_BACKGROUND_MARGIN);
    let top = top.saturating_sub(UNIFORM_BACKGROUND_MARGIN);
    let right = right
        .saturating_add(UNIFORM_BACKGROUND_MARGIN)
        .min(image.width());
    let bottom = bottom
        .saturating_add(UNIFORM_BACKGROUND_MARGIN)
        .min(image.height());
    let mut channels = [Vec::new(), Vec::new(), Vec::new()];
    for y in top..bottom {
        for x in left..right {
            if mask.get_pixel(x, y)[0] >= 127 {
                continue;
            }
            let pixel = image.get_pixel(x, y);
            for channel in 0..3 {
                channels[channel].push(pixel[channel]);
            }
        }
    }
    if channels[0].len() < UNIFORM_BACKGROUND_MIN_PIXELS {
        return None;
    }

    let medians = channels.each_mut().map(|values| median(values));
    let deviations = std::array::from_fn::<_, 3, _>(|channel| {
        standard_deviation(&channels[channel], f64::from(medians[channel]))
    });
    let mean_deviation = deviations.iter().sum::<f64>() / deviations.len() as f64;
    let channel_spread = (deviations
        .iter()
        .map(|deviation| (deviation - mean_deviation).powi(2))
        .sum::<f64>()
        / deviations.len() as f64)
        .sqrt();
    let threshold = if channel_spread > 1.0 { 7.0 } else { 10.0 };
    (deviations.iter().copied().fold(0.0, f64::max) < threshold).then_some(Rgb(medians))
}

fn mask_bounds(mask: &GrayImage) -> Option<[u32; 4]> {
    mask_bounds_in(mask, [0, 0, mask.width(), mask.height()])
}

fn mask_bounds_in(
    mask: &GrayImage,
    [region_left, region_top, region_right, region_bottom]: [u32; 4],
) -> Option<[u32; 4]> {
    let mut left = region_right;
    let mut top = region_bottom;
    let mut right = 0;
    let mut bottom = 0;
    for y in region_top..region_bottom {
        for x in region_left..region_right {
            if mask.get_pixel(x, y)[0] >= 127 {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
    }
    (right > left && bottom > top).then_some([left, top, right, bottom])
}

fn median(values: &mut [u8]) -> u8 {
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        ((u16::from(values[middle - 1]) + u16::from(values[middle])) / 2) as u8
    } else {
        values[middle]
    }
}

fn standard_deviation(values: &[u8], center: f64) -> f64 {
    (values
        .iter()
        .map(|value| (f64::from(*value) - center).powi(2))
        .sum::<f64>()
        / values.len() as f64)
        .sqrt()
}

fn composite_color(
    output: &mut RgbImage,
    mask: &GrayImage,
    [left, top, right, bottom]: [u32; 4],
    color: Rgb<u8>,
) {
    for y in top..bottom {
        for x in left..right {
            if mask.get_pixel(x, y)[0] >= 127 {
                output.put_pixel(x, y, color);
            }
        }
    }
}

fn composite_generated(
    output: &mut RgbImage,
    mask: &GrayImage,
    tile: InpaintTile,
    generated: &RgbImage,
) {
    let [left, top, _, _] = tile.crop;
    let [core_left, core_top, core_right, core_bottom] = tile.core;
    for y in core_top..core_bottom {
        for x in core_left..core_right {
            if mask.get_pixel(x, y)[0] >= 127 {
                output.put_pixel(x, y, *generated.get_pixel(x - left, y - top));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_mask_background_is_filled_without_inference() {
        let mut image = RgbImage::from_pixel(96, 96, Rgb([240, 241, 242]));
        let mut mask = GrayImage::new(96, 96);
        for y in 40..56 {
            for x in 32..64 {
                image.put_pixel(x, y, Rgb([20, 20, 20]));
                mask.put_pixel(x, y, Luma([u8::MAX]));
            }
        }
        let mut calls = 0;

        let output = inpaint_tiled(&DynamicImage::ImageRgb8(image), &mask, |_, _| {
            calls += 1;
            Ok(DynamicImage::new_rgb8(1, 1))
        })
        .unwrap()
        .to_rgb8();

        assert_eq!(calls, 0);
        assert_eq!(output.get_pixel(48, 48), &Rgb([240, 241, 242]));
    }

    #[test]
    fn textured_mask_regions_are_inferred_as_bounded_tiles() {
        let mut image = RgbImage::new(1200, 700);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = if (x + y).is_multiple_of(2) {
                Rgb([0, 40, 80])
            } else {
                Rgb([255, 215, 175])
            };
        }
        let original = image.clone();
        let mut mask = GrayImage::new(1200, 700);
        mask.put_pixel(10, 10, Luma([u8::MAX]));
        mask.put_pixel(1100, 600, Luma([u8::MAX]));
        let mut calls = 0;

        let output = inpaint_tiled(&DynamicImage::ImageRgb8(image), &mask, |tile, _| {
            calls += 1;
            assert!(tile.width() <= TILE_CONTEXT * 2 + 1);
            assert!(tile.height() <= TILE_CONTEXT * 2 + 1);
            Ok(DynamicImage::ImageRgb8(RgbImage::from_pixel(
                tile.width(),
                tile.height(),
                Rgb([1, 2, 3]),
            )))
        })
        .unwrap()
        .to_rgb8();

        assert_eq!(calls, 2);
        assert_eq!(output.get_pixel(10, 10), &Rgb([1, 2, 3]));
        assert_eq!(output.get_pixel(1100, 600), &Rgb([1, 2, 3]));
        assert_eq!(output.get_pixel(500, 300), original.get_pixel(500, 300));
    }
}
