use std::{
    collections::BTreeMap,
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use async_trait::async_trait;
use image::{
    DynamicImage, GenericImageView as _, GrayImage, ImageFormat, Luma, Rgb, RgbImage, Rgba,
    RgbaImage,
};
use koharu_ml::{
    aot_inpainting::AotInpainting,
    flux2_klein::{Flux2KleinInpaint, Flux2KleinInpaintOptions},
    lama::{InpaintRequest, LaMa},
    rorem_mixed::{DEFAULT_NEGATIVE_PROMPT, DEFAULT_PROMPT, RoremMixed, RoremMixedOptions},
};
use koharu_scene::{
    AssetInput, AssetMetadata, AssetRole, At, BubbleRegion, EntityOrigin, Geometry, Origin,
    RasterLayer, RasterLayerKind, Region, RegionSpec,
};
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

    async fn process(&self, input: StageInput) -> Result<koharu_scene::Patch> {
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

    async fn run(&self, input: StageInput) -> Result<koharu_scene::Patch> {
        let mut prepared = prepare(&input)?;
        if prepared.mask.as_raw().iter().all(|value| *value == 0) {
            return finish(input.scene.edit());
        }
        let mask = prepared.mask.clone();
        let original = prepared.original.clone();
        let cleanup = prepared.cleanup.take();
        let cleanup_entity = prepared.cleanup_entity;
        let (model_name, image) = match self {
            Self::LaMa(model) => {
                let model = model.clone();
                (
                    "lama",
                    tokio::task::spawn_blocking(move || -> Result<DynamicImage> {
                        let model = model
                            .lock()
                            .map_err(|_| anyhow!("LaMa model lock is poisoned"))?;
                        inpaint_tiled(
                            &prepared.image,
                            &prepared.mask,
                            &prepared.text_mask,
                            &prepared.flat_fill_regions,
                            |image, mask| {
                                Ok(DynamicImage::ImageRgb8(model.inference(
                                    image,
                                    mask,
                                    &InpaintRequest::default(),
                                )?))
                            },
                        )
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
                        inpaint_tiled(
                            &prepared.image,
                            &prepared.mask,
                            &prepared.text_mask,
                            &prepared.flat_fill_regions,
                            |image, mask| {
                                Ok(DynamicImage::ImageRgb8(model.inference(image, mask)?))
                            },
                        )
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
                        inpaint_tiled(
                            &prepared.image,
                            &prepared.mask,
                            &prepared.text_mask,
                            &prepared.flat_fill_regions,
                            |image, mask| {
                                model.inference(
                                    &config.prompt,
                                    image,
                                    None,
                                    &DynamicImage::ImageLuma8(mask.clone()),
                                    &Flux2KleinInpaintOptions::default(),
                                )
                            },
                        )
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
                        inpaint_tiled(
                            &prepared.image,
                            &prepared.mask,
                            &prepared.text_mask,
                            &prepared.flat_fill_regions,
                            |image, mask| {
                                Ok(DynamicImage::ImageRgb8(model.inference(
                                    image,
                                    mask,
                                    &config.prompt,
                                    &config.negative_prompt,
                                    &RoremMixedOptions::default(),
                                )?))
                            },
                        )
                    })
                    .await
                    .context("RORem task panicked")??,
                )
            }
        };
        let page = input.page;
        let manual = input.inpainting_mask.is_some();
        let mut edit = if manual {
            input.scene.edit()
        } else {
            input.scene.edit_as(generation(PRODUCER, model_name)?)
        };
        edit.observe_assets(page)?;
        if let Some(entity) = cleanup_entity {
            edit.observe::<RasterLayer>(entity)?;
            edit.observe_assets(entity)?;
        }
        let image = image.to_rgba8();
        if image.dimensions() != original.dimensions() || image.dimensions() != mask.dimensions() {
            bail!("inpainted image dimensions do not match page {page}");
        }
        let mut overlay = if manual {
            cleanup.unwrap_or_else(|| RgbaImage::new(image.width(), image.height()))
        } else {
            RgbaImage::new(image.width(), image.height())
        };
        for (x, y, target) in overlay.enumerate_pixels_mut() {
            if mask.get_pixel(x, y)[0] < 127 {
                continue;
            }
            let generated = image.get_pixel(x, y);
            // Keep the complete cleanup mask opaque. Transparent texels next to
            // edited pixels let linear canvas sampling reveal the artwork below.
            *target = Rgba([generated[0], generated[1], generated[2], 255]);
        }
        let mut bytes = Cursor::new(Vec::new());
        let width = overlay.width();
        let height = overlay.height();
        DynamicImage::ImageRgba8(overlay).write_to(&mut bytes, ImageFormat::Png)?;
        let cleanup_entity = if let Some(entity) = cleanup_entity {
            if manual {
                let mut layer = input
                    .scene
                    .component::<RasterLayer>(entity)?
                    .context("cleanup entity has no raster layer component")?;
                let generated = layer.origin != Origin::User
                    || input
                        .scene
                        .component::<EntityOrigin>(entity)?
                        .is_some_and(|origin| origin.origin != Origin::User);
                if generated {
                    edit.promote_entity_to_user(entity)?;
                    layer.origin = Origin::User;
                    edit.set(entity, &layer)?;
                }
            }
            entity
        } else {
            let entity = edit.add_entity(page, At::Start)?;
            edit.set(
                entity,
                &RasterLayer {
                    origin: Origin::User,
                    name: "Cleanup".to_owned(),
                    kind: RasterLayerKind::Cleanup,
                },
            )?;
            entity
        };
        edit.set_asset(
            cleanup_entity,
            &AssetRole::new("source")?,
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

#[derive(Clone, Debug)]
struct FlatFillRegion {
    bounds: [u32; 4],
    polygon: Vec<(f32, f32)>,
}

fn flat_fill_regions(input: &StageInput, width: u32, height: u32) -> Result<Vec<FlatFillRegion>> {
    let mut regions = Vec::new();
    for entity in input.scene.descendants(input.page)? {
        let id = entity.id();
        let is_bubble = input
            .scene
            .component::<Region>(id)?
            .is_some_and(|region| region.kind == BubbleRegion::kind());
        if !is_bubble {
            continue;
        }
        let Some(geometry) = input.scene.component::<Geometry>(id)? else {
            continue;
        };
        let polygon = geometry
            .points
            .iter()
            .map(|point| (point.x as f32, point.y as f32))
            .collect::<Vec<_>>();
        if polygon.len() < 3 {
            continue;
        }
        let (mut left, mut top) = (f32::INFINITY, f32::INFINITY);
        let (mut right, mut bottom) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for &(x, y) in &polygon {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
        let bounds = [
            left.floor().clamp(0.0, width as f32) as u32,
            top.floor().clamp(0.0, height as f32) as u32,
            right.ceil().clamp(0.0, width as f32) as u32,
            bottom.ceil().clamp(0.0, height as f32) as u32,
        ];
        if bounds[2] > bounds[0] && bounds[3] > bounds[1] {
            regions.push(FlatFillRegion { bounds, polygon });
        }
    }
    Ok(regions)
}

struct InpaintInput {
    image: Arc<DynamicImage>,
    original: Arc<DynamicImage>,
    cleanup_entity: Option<koharu_scene::EntityId>,
    cleanup: Option<RgbaImage>,
    mask: GrayImage,
    text_mask: GrayImage,
    flat_fill_regions: Vec<FlatFillRegion>,
}

fn prepare(input: &StageInput) -> Result<InpaintInput> {
    let page = input.page;
    let original = input
        .images
        .get(&input.scene, page, "source")?
        .ok_or_else(|| anyhow!("page {page} has no source image"))?;
    let cleanup_entity = input.scene.children(page)?.find(|entity| {
        input
            .scene
            .component::<RasterLayer>(*entity)
            .ok()
            .flatten()
            .is_some_and(|layer| layer.kind == RasterLayerKind::Cleanup)
    });
    let cleanup = cleanup_entity
        .map(|entity| input.images.get(&input.scene, entity, "source"))
        .transpose()?
        .flatten()
        .map(|image| image.to_rgba8());
    if cleanup
        .as_ref()
        .is_some_and(|image| image.dimensions() != original.dimensions())
    {
        bail!("cleanup layer dimensions do not match page {page}");
    }
    let source = if input.inpainting_mask.is_some() {
        if let Some(cleanup) = cleanup.as_ref() {
            let mut composite = original.to_rgba8();
            image::imageops::overlay(&mut composite, cleanup, 0, 0);
            Arc::new(DynamicImage::ImageRgba8(composite))
        } else {
            original.clone()
        }
    } else {
        original.clone()
    };
    let mut mask = GrayImage::new(source.width(), source.height());
    let mut text_mask = GrayImage::new(source.width(), source.height());
    if let Some(transient) = &input.inpainting_mask {
        let layer = image::load_from_memory(&transient.png)?.to_luma8();
        if layer.dimensions() != mask.dimensions() {
            bail!("inpainting mask dimensions do not match page {page}");
        }
        mask = layer;
    } else {
        for role in ["text-mask", "coo-mask"] {
            if let Some(image) = input.images.get(&input.scene, page, role)? {
                let layer = image.to_luma8();
                if layer.dimensions() != mask.dimensions() {
                    bail!("{role} dimensions do not match page {page}");
                }
                for (target, source) in mask.as_mut().iter_mut().zip(layer.as_raw()) {
                    *target = (*target).max(*source);
                }
                if role == "text-mask" {
                    text_mask = layer;
                }
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
                text_mask.put_pixel(x, y, Luma([0]));
            }
        }
    }
    let flat_fill_regions = flat_fill_regions(input, source.width(), source.height())?;
    Ok(InpaintInput {
        image: source,
        original,
        cleanup_entity,
        cleanup,
        mask,
        text_mask,
        flat_fill_regions,
    })
}

const TILE_SIZE: u32 = 512;
const TILE_CONTEXT: u32 = 128;
const UNIFORM_BACKGROUND_MIN_PIXELS: usize = 16;
const FLAT_FILL_EDGE_MARGIN: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InpaintTile {
    core: [u32; 4],
    crop: [u32; 4],
}

// BallonsTranslator avoids model inference for a text block when the non-text
// pixels inside its balloon are nearly uniform, and otherwise sends an enlarged
// block crop to the inpainter:
// https://github.com/dmMaze/BallonsTranslator/blob/4bcc635c19f6c63a902872cf77b3d554e14ed1b7/ballontranslator/modules/inpaint/base.py#L168-L200
// Koharu uses the detected bubble polygons as those blocks. Uniform bubbles are
// filled first; only the remaining mask is split into bounded model crops.
fn inpaint_tiled(
    image: &DynamicImage,
    mask: &GrayImage,
    text_mask: &GrayImage,
    flat_fill_regions: &[FlatFillRegion],
    mut inference: impl FnMut(&DynamicImage, &GrayImage) -> Result<DynamicImage>,
) -> Result<DynamicImage> {
    ensure!(
        image.dimensions() == mask.dimensions() && mask.dimensions() == text_mask.dimensions(),
        "image and mask dimensions differ: image={:?}, mask={:?}, text_mask={:?}",
        image.dimensions(),
        mask.dimensions(),
        text_mask.dimensions()
    );

    let mut output = image.to_rgb8();
    let mut pending_mask = mask.clone();
    fill_uniform_regions(&mut output, &mut pending_mask, text_mask, flat_fill_regions);
    for tile in inpaint_tiles(&pending_mask) {
        let [left, top, right, bottom] = tile.crop;
        let crop_width = right - left;
        let crop_height = bottom - top;
        let crop_image =
            image::imageops::crop_imm(&output, left, top, crop_width, crop_height).to_image();
        let crop_mask = crop_tile_mask(&pending_mask, tile);

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
        composite_generated(&mut output, &pending_mask, tile, &generated);
    }
    Ok(DynamicImage::ImageRgb8(output))
}

fn fill_uniform_regions(
    output: &mut RgbImage,
    pending_mask: &mut GrayImage,
    text_mask: &GrayImage,
    regions: &[FlatFillRegion],
) {
    for region in regions {
        let [left, top, right, bottom] = region.bounds;
        let mut targets = Vec::new();
        for y in top..bottom {
            for x in left..right {
                if pending_mask.get_pixel(x, y)[0] >= 127
                    && text_mask.get_pixel(x, y)[0] >= 127
                    && point_in_polygon((x as f32 + 0.5, y as f32 + 0.5), &region.polygon)
                {
                    targets.push((x, y));
                }
            }
        }
        if targets.is_empty() {
            continue;
        }
        let Some(color) = uniform_region_color(output, pending_mask, region) else {
            continue;
        };
        for (x, y) in targets {
            output.put_pixel(x, y, color);
            pending_mask.put_pixel(x, y, Luma([0]));
        }
    }
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

fn uniform_region_color(
    image: &RgbImage,
    mask: &GrayImage,
    region: &FlatFillRegion,
) -> Option<Rgb<u8>> {
    let [left, top, right, bottom] = region.bounds;
    let mut channels = [Vec::new(), Vec::new(), Vec::new()];
    for y in top..bottom {
        for x in left..right {
            let point = (x as f32 + 0.5, y as f32 + 0.5);
            if mask.get_pixel(x, y)[0] >= 127
                || !point_in_polygon(point, &region.polygon)
                || polygon_edge_distance_squared(point, &region.polygon)
                    < FLAT_FILL_EDGE_MARGIN * FLAT_FILL_EDGE_MARGIN
            {
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

fn point_in_polygon(point: (f32, f32), polygon: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    for &current in polygon {
        if (current.1 > point.1) != (previous.1 > point.1) {
            let intersection_x = (previous.0 - current.0) * (point.1 - current.1)
                / (previous.1 - current.1)
                + current.0;
            if point.0 < intersection_x {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

fn polygon_edge_distance_squared(point: (f32, f32), polygon: &[(f32, f32)]) -> f32 {
    let mut minimum = f32::INFINITY;
    let mut start = polygon[polygon.len() - 1];
    for &end in polygon {
        let segment = (end.0 - start.0, end.1 - start.1);
        let length_squared = segment.0 * segment.0 + segment.1 * segment.1;
        let projection = if length_squared > 0.0 {
            ((point.0 - start.0) * segment.0 + (point.1 - start.1) * segment.1) / length_squared
        } else {
            0.0
        }
        .clamp(0.0, 1.0);
        let closest = (
            start.0 + segment.0 * projection,
            start.1 + segment.1 * projection,
        );
        let dx = point.0 - closest.0;
        let dy = point.1 - closest.1;
        minimum = minimum.min(dx * dx + dy * dy);
        start = end;
    }
    minimum
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
    fn transient_inpainting_mask_replaces_persistent_page_masks() {
        let mut session = koharu_scene::Session::memory().unwrap();
        let mut page = None;
        let source = DynamicImage::new_rgb8(8, 8);
        let persistent = DynamicImage::ImageLuma8(GrayImage::from_pixel(8, 8, Luma([255])));
        let encode = |image: &DynamicImage| {
            let mut bytes = Cursor::new(Vec::new());
            image.write_to(&mut bytes, ImageFormat::Png).unwrap();
            Arc::<[u8]>::from(bytes.into_inner())
        };
        let patch = session
            .snapshot()
            .patch(|edit| {
                let id = edit.add_page(
                    koharu_scene::PageDraft::new("page", 8.0, 8.0),
                    koharu_scene::At::End,
                )?;
                for (role, image) in [("source", &source), ("text-mask", &persistent)] {
                    edit.set_asset(
                        id,
                        &AssetRole::new(role)?,
                        AssetInput::new(
                            encode(image),
                            "image/png",
                            AssetMetadata {
                                width: Some(8),
                                height: Some(8),
                                attributes: BTreeMap::new(),
                            },
                        ),
                    )?;
                }
                page = Some(id);
                Ok(())
            })
            .unwrap();
        let snapshot = session.commit(patch).unwrap().snapshot;
        let page = page.unwrap();
        let mut transient = GrayImage::new(8, 8);
        transient.put_pixel(3, 4, Luma([255]));
        let input = StageInput::new(
            snapshot,
            page,
            None,
            None,
            Arc::new(crate::ImageCache::default()),
            Some(crate::InpaintingMask {
                page,
                png: encode(&DynamicImage::ImageLuma8(transient)),
            }),
        );

        let prepared = prepare(&input).unwrap();
        assert_eq!(prepared.mask.get_pixel(3, 4), &Luma([255]));
        assert_eq!(prepared.mask.get_pixel(0, 0), &Luma([0]));
        assert!(prepared.text_mask.pixels().all(|pixel| pixel[0] == 0));
    }

    fn rectangle_region([left, top, right, bottom]: [u32; 4]) -> FlatFillRegion {
        FlatFillRegion {
            bounds: [left, top, right, bottom],
            polygon: vec![
                (left as f32, top as f32),
                (right as f32, top as f32),
                (right as f32, bottom as f32),
                (left as f32, bottom as f32),
            ],
        }
    }

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

        let output = inpaint_tiled(
            &DynamicImage::ImageRgb8(image),
            &mask,
            &mask,
            &[rectangle_region([0, 0, 96, 96])],
            |_, _| {
                calls += 1;
                Ok(DynamicImage::new_rgb8(1, 1))
            },
        )
        .unwrap()
        .to_rgb8();

        assert_eq!(calls, 0);
        assert_eq!(output.get_pixel(48, 48), &Rgb([240, 241, 242]));
    }

    #[test]
    fn uniform_bubbles_are_filled_independently_inside_one_textured_tile() {
        let mut image = RgbImage::new(200, 100);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = if (x + y).is_multiple_of(2) {
                Rgb([20, 80, 140])
            } else {
                Rgb([220, 80, 120])
            };
        }
        for y in 10..90 {
            for x in 10..90 {
                image.put_pixel(x, y, Rgb([245, 245, 245]));
            }
            for x in 110..190 {
                image.put_pixel(x, y, Rgb([250, 240, 220]));
            }
        }
        let mut mask = GrayImage::new(200, 100);
        for y in 40..60 {
            for x in 30..60 {
                image.put_pixel(x, y, Rgb([10, 10, 10]));
                mask.put_pixel(x, y, Luma([u8::MAX]));
            }
            for x in 135..165 {
                image.put_pixel(x, y, Rgb([10, 10, 10]));
                mask.put_pixel(x, y, Luma([u8::MAX]));
            }
        }
        let mut calls = 0;

        let output = inpaint_tiled(
            &DynamicImage::ImageRgb8(image),
            &mask,
            &mask,
            &[
                rectangle_region([10, 10, 90, 90]),
                rectangle_region([110, 10, 190, 90]),
            ],
            |_, _| {
                calls += 1;
                Ok(DynamicImage::new_rgb8(1, 1))
            },
        )
        .unwrap()
        .to_rgb8();

        assert_eq!(calls, 0);
        assert_eq!(output.get_pixel(45, 50), &Rgb([245, 245, 245]));
        assert_eq!(output.get_pixel(150, 50), &Rgb([250, 240, 220]));
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

        let output = inpaint_tiled(
            &DynamicImage::ImageRgb8(image),
            &mask,
            &mask,
            &[],
            |tile, _| {
                calls += 1;
                assert!(tile.width() <= TILE_CONTEXT * 2 + 1);
                assert!(tile.height() <= TILE_CONTEXT * 2 + 1);
                Ok(DynamicImage::ImageRgb8(RgbImage::from_pixel(
                    tile.width(),
                    tile.height(),
                    Rgb([1, 2, 3]),
                )))
            },
        )
        .unwrap()
        .to_rgb8();

        assert_eq!(calls, 2);
        assert_eq!(output.get_pixel(10, 10), &Rgb([1, 2, 3]));
        assert_eq!(output.get_pixel(1100, 600), &Rgb([1, 2, 3]));
        assert_eq!(output.get_pixel(500, 300), original.get_pixel(500, 300));
    }
}
