use std::{
    cmp::Ordering,
    collections::BTreeMap,
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat, Luma, RgbImage};
use imageproc::{distance_transform::Norm, morphology::dilate};
use koharu_ml::koharu_layout_rfdetr_seg_2xl::{
    KoharuLayoutDetection, KoharuLayoutDetections, KoharuLayoutRFDetrSeg2XL, KoharuLayoutThresholds,
};
use koharu_scene::{
    AssetInput, AssetMetadata, AssetRole, At, DetectionAnalysis, DetectionLabel, EntityId,
    EntityOrigin, Generation, Geometry, Origin, Point, ReadingOrder, Region, RegionKind,
    RelationKind, RemovePolicy, TextRole, Typography, WritingMode,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{ModelRef, StageInput, StageProcessor, finish, generation};
use crate::{DetectionModel, ModelCell};

const MODEL_ID: &str = "mayocream/koharu-layout-rfdetr-seg-2xl-1152";
const MODEL_NAME: &str = "koharu-layout-rfdetr-seg-2xl";
const PRODUCER: &str = "dev.koharu.pipeline.detection";
const FOREGROUND_COLOR_EXTENSION: &str = "dev.koharu.typography.foreground-color";
const ANGLE_DEGREES_EXTENSION: &str = "dev.koharu.typography.angle-degrees";
const AXIS_ANISOTROPY_MINIMUM: f64 = 0.05;
const ANGLE_SNAP_DEGREES: f32 = 3.0;
const COLOR_SNAP_CHANNEL: u8 = 32;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct KoharuLayoutRFDetrSeg2XLConfig {
    pub text_threshold: Option<f32>,
    pub bubble_threshold: Option<f32>,
    pub panel_threshold: Option<f32>,
}

pub(super) struct Processor {
    config: DetectionModel,
    device: koharu_ml::Device,
    model: ModelCell<Model>,
}

impl Processor {
    pub(super) fn new(config: DetectionModel, device: koharu_ml::Device) -> Result<Self> {
        let DetectionModel::KoharuLayoutRFDetrSeg2XL(settings) = &config;
        for (name, value) in [
            ("text", settings.text_threshold),
            ("bubble", settings.bubble_threshold),
            ("panel", settings.panel_threshold),
        ] {
            if let Some(value) = value {
                ensure!(
                    value.is_finite() && (0.0..=1.0).contains(&value),
                    "{name} confidence threshold must be finite and between zero and one"
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
        ModelRef::new(MODEL_NAME, &self.model)
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
            .ok_or_else(|| anyhow!("detection model is not loaded"))?
            .run(input)
            .await
    }
}

struct Model {
    network: Arc<Mutex<KoharuLayoutRFDetrSeg2XL>>,
    thresholds: KoharuLayoutThresholds,
}

impl Model {
    async fn load(device: koharu_ml::Device, config: &DetectionModel) -> Result<Self> {
        let DetectionModel::KoharuLayoutRFDetrSeg2XL(config) = config;
        let network = KoharuLayoutRFDetrSeg2XL::load(device).await?;
        let mut thresholds = network.recommended_thresholds();
        thresholds.text = config.text_threshold.unwrap_or(thresholds.text);
        thresholds.bubble = config.bubble_threshold.unwrap_or(thresholds.bubble);
        thresholds.panel = config.panel_threshold.unwrap_or(thresholds.panel);
        Ok(Self {
            network: Arc::new(Mutex::new(network)),
            thresholds,
        })
    }

    async fn run(&self, input: StageInput) -> Result<koharu_scene::ScenePatch> {
        let page = input.page;
        let image = input
            .images
            .get(&input.scene, page, "source")?
            .ok_or_else(|| anyhow!("page {page} has no source image"))?;
        let output = self.detect(image.clone()).await?;
        build_patch(&input, &image, output, &generation(PRODUCER, MODEL_ID)?)
    }

    async fn detect(&self, image: Arc<DynamicImage>) -> Result<KoharuLayoutDetections> {
        let network = self.network.clone();
        let thresholds = self.thresholds;
        tokio::task::spawn_blocking(move || {
            let network = network
                .lock()
                .map_err(|_| anyhow!("layout model lock is poisoned"))?;
            network.inference_with_thresholds(&image, thresholds)
        })
        .await
        .context("layout detection task panicked")?
    }
}

#[derive(Clone, Copy)]
struct DetectedRegion {
    entity: EntityId,
    bounds: [f32; 4],
}

#[derive(Default)]
struct PageRegions {
    bubbles: Vec<DetectedRegion>,
    texts: Vec<DetectedRegion>,
}

#[derive(Clone, Copy)]
struct ImageSize {
    width: u32,
    height: u32,
}

fn build_patch(
    input: &StageInput,
    image: &DynamicImage,
    output: KoharuLayoutDetections,
    generation: &Generation,
) -> Result<koharu_scene::ScenePatch> {
    let page = input.page;
    let mut edit = input.scene.edit_as(generation.clone());
    edit.observe_subtree(page)?;
    remove_previous_detections(input, &mut edit, generation)?;
    write_page(input, &mut edit, page, image, output, generation)?;
    finish(edit)
}

fn remove_previous_detections(
    input: &StageInput,
    edit: &mut koharu_scene::SceneEdit,
    generation: &Generation,
) -> Result<()> {
    let mut remove = Vec::new();
    for entity in input.scene.descendants(input.page)? {
        let id = entity.id();
        if !input.contains_entity(id)? {
            continue;
        }
        let owned = entity
            .component::<EntityOrigin>("default")?
            .is_some_and(|origin| {
                matches!(origin.origin, Origin::Generated(ref owner) if owner.producer == generation.producer)
            });
        if owned {
            remove.push(id);
        }
    }
    for entity in remove {
        if input.scene.entity(entity).is_ok() {
            edit.remove_entity(entity, RemovePolicy::Cascade)?;
        }
    }
    Ok(())
}

fn write_page(
    input: &StageInput,
    edit: &mut koharu_scene::SceneEdit,
    page: EntityId,
    image: &DynamicImage,
    output: KoharuLayoutDetections,
    generation: &Generation,
) -> Result<()> {
    let KoharuLayoutDetections {
        mut detections,
        image_width,
        image_height,
    } = output;
    let size = ImageSize {
        width: image_width,
        height: image_height,
    };
    prepare_detections(&mut detections, input.region);

    let image = image.to_rgb8();
    let regions = write_regions(edit, page, &image, &detections, generation)?;
    link_dialogue_regions(edit, &regions, generation)?;
    write_masks(input, edit, page, &detections, size)
}

fn write_regions(
    edit: &mut koharu_scene::SceneEdit,
    page: EntityId,
    image: &RgbImage,
    detections: &[KoharuLayoutDetection],
    generation: &Generation,
) -> Result<PageRegions> {
    let mut regions = PageRegions::default();
    for (order, detection) in detections.iter().enumerate() {
        let entity = write_region(edit, page, image, detection, order as u32, generation)?;
        let detected = DetectedRegion {
            entity,
            bounds: detection.bbox,
        };
        match detection.label.as_str() {
            "bubble" => regions.bubbles.push(detected),
            "text" => regions.texts.push(detected),
            _ => {}
        }
    }
    Ok(regions)
}

fn write_region(
    edit: &mut koharu_scene::SceneEdit,
    page: EntityId,
    image: &RgbImage,
    detection: &KoharuLayoutDetection,
    order: u32,
    generation: &Generation,
) -> Result<EntityId> {
    let entity = edit.add_entity(page, At::End)?;
    let kind = region_kind(&detection.label)?;
    let inferred = (detection.label == "text")
        .then(|| infer_typography(image, detection))
        .flatten();
    let geometry = inferred.as_ref().map_or_else(
        || rectangle_geometry(detection.bbox),
        |inferred| rotated_geometry(detection.bbox, inferred.angle_degrees),
    );
    edit.set(entity, "default", &geometry)?;
    edit.set(
        entity,
        "default",
        &Region {
            origin: Origin::Generated(generation.clone()),
            kind: kind.clone(),
            label: Some(detection.label.clone()),
        },
    )?;
    edit.set(
        entity,
        "default",
        &DetectionAnalysis {
            origin: Origin::Generated(generation.clone()),
            labels: vec![DetectionLabel {
                kind,
                confidence: detection.score,
            }],
        },
    )?;
    edit.set(
        entity,
        "default",
        &ReadingOrder {
            origin: Origin::Generated(generation.clone()),
            index: order,
        },
    )?;
    if detection.label == "text" {
        write_text_role(edit, entity, "dev.koharu.text.free-text", generation)?;
        if let Some(inferred) = inferred {
            let mut extensions = BTreeMap::new();
            extensions.insert(
                FOREGROUND_COLOR_EXTENSION.to_owned(),
                format!(
                    "#{:02x}{:02x}{:02x}",
                    inferred.color[0], inferred.color[1], inferred.color[2]
                ),
            );
            extensions.insert(
                ANGLE_DEGREES_EXTENSION.to_owned(),
                inferred.angle_degrees.to_string(),
            );
            edit.set(
                entity,
                "default",
                &Typography {
                    origin: Origin::Generated(generation.clone()),
                    preferred_font: None,
                    size: Some(inferred.font_size),
                    alignment: None,
                    writing_mode: Some(inferred.writing_mode),
                    extensions,
                },
            )?;
        }
    }
    Ok(entity)
}

fn link_dialogue_regions(
    edit: &mut koharu_scene::SceneEdit,
    regions: &PageRegions,
    generation: &Generation,
) -> Result<()> {
    let relation = RelationKind::new("dev.koharu.relation.text-region")?;
    for text in &regions.texts {
        let bubble = regions
            .bubbles
            .iter()
            .filter(|bubble| containment(bubble.bounds, text.bounds) >= 0.5)
            .min_by(|left, right| area(left.bounds).total_cmp(&area(right.bounds)));
        if let Some(bubble) = bubble {
            edit.add_relation(relation.clone(), text.entity, bubble.entity)?;
            write_text_role(edit, text.entity, "dev.koharu.text.dialogue", generation)?;
        }
    }
    Ok(())
}

fn write_text_role(
    edit: &mut koharu_scene::SceneEdit,
    entity: EntityId,
    role: &str,
    generation: &Generation,
) -> Result<()> {
    edit.set(
        entity,
        "default",
        &TextRole {
            origin: Origin::Generated(generation.clone()),
            role: role.to_owned(),
        },
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct InferredTypography {
    font_size: f32,
    color: [u8; 3],
    angle_degrees: f32,
    writing_mode: WritingMode,
}

#[derive(Clone, Copy)]
struct MaskPoint {
    x: f64,
    y: f64,
}

// BallonsTranslator defines font size as the text-line cross-axis span and
// normalizes vertical-line angles relative to upright vertical text:
// https://github.com/dmMaze/BallonsTranslator/blob/4bcc635c19f6c63a902872cf77b3d554e14ed1b7/ballontranslator/utils/textblock.py#L576-L608
// RF-DETR provides foreground pixels rather than line quadrilaterals, so PCA
// supplies the line axis and a projection of the mask supplies its cross span.
fn infer_typography(
    image: &RgbImage,
    detection: &KoharuLayoutDetection,
) -> Option<InferredTypography> {
    let mask = &detection.mask;
    let width = image.width().min(mask.width);
    let height = image.height().min(mask.height);
    let [left, top, right, bottom] = mask_window(detection.bbox, width, height)?;
    let mut points = Vec::new();
    let mut channels = [Vec::new(), Vec::new(), Vec::new()];
    for y in top..bottom {
        let row = y as usize * mask.width as usize;
        for x in left..right {
            if mask.pixels.get(row + x as usize).copied().unwrap_or(0) == 0 {
                continue;
            }
            points.push(MaskPoint {
                x: f64::from(x) + 0.5,
                y: f64::from(y) + 0.5,
            });
            let pixel = image.get_pixel(x, y);
            for channel in 0..3 {
                channels[channel].push(pixel[channel]);
            }
        }
    }
    if points.is_empty() {
        return None;
    }

    let (angle_degrees, vertical) = mask_angle(&points, detection.bbox);
    let font_size = mask_font_size(&points, angle_degrees, vertical)?;
    let color = normalize_text_color(channels.each_mut().map(|values| median_channel(values)));
    Some(InferredTypography {
        font_size,
        color,
        angle_degrees,
        writing_mode: if vertical {
            WritingMode::Vertical
        } else {
            WritingMode::Horizontal
        },
    })
}

fn mask_window([left, top, right, bottom]: [f32; 4], width: u32, height: u32) -> Option<[u32; 4]> {
    if width == 0 || height == 0 {
        return None;
    }
    let left = left.floor().clamp(0.0, width as f32) as u32;
    let top = top.floor().clamp(0.0, height as f32) as u32;
    let right = right.ceil().clamp(0.0, width as f32) as u32;
    let bottom = bottom.ceil().clamp(0.0, height as f32) as u32;
    (right > left && bottom > top).then_some([left, top, right, bottom])
}

fn mask_angle(points: &[MaskPoint], [left, top, right, bottom]: [f32; 4]) -> (f32, bool) {
    let count = points.len() as f64;
    let mean_x = points.iter().map(|point| point.x).sum::<f64>() / count;
    let mean_y = points.iter().map(|point| point.y).sum::<f64>() / count;
    let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
    for point in points {
        let x = point.x - mean_x;
        let y = point.y - mean_y;
        xx += x * x;
        xy += x * y;
        yy += y * y;
    }
    let spread = xx + yy;
    let anisotropy = if spread > f64::EPSILON {
        ((xx - yy).powi(2) + 4.0 * xy.powi(2)).sqrt() / spread
    } else {
        0.0
    };
    let axis = if anisotropy >= AXIS_ANISOTROPY_MINIMUM {
        0.5 * (2.0 * xy).atan2(xx - yy)
    } else if bottom - top > right - left {
        std::f64::consts::FRAC_PI_2
    } else {
        0.0
    };
    let vertical = axis.abs() > std::f64::consts::FRAC_PI_4;
    let mut angle = if vertical {
        axis - axis.signum() * std::f64::consts::FRAC_PI_2
    } else {
        axis
    }
    .to_degrees() as f32;
    if angle.abs() < ANGLE_SNAP_DEGREES {
        angle = 0.0;
    }
    (angle, vertical)
}

fn mask_font_size(points: &[MaskPoint], angle_degrees: f32, vertical: bool) -> Option<f32> {
    let line_angle = f64::from(angle_degrees).to_radians()
        + if vertical {
            std::f64::consts::FRAC_PI_2
        } else {
            0.0
        };
    let cross_x = -line_angle.sin();
    let cross_y = line_angle.cos();
    let projections = points
        .iter()
        .map(|point| point.x * cross_x + point.y * cross_y)
        .collect::<Vec<_>>();
    let minimum = projections.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = projections
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let length = (maximum.ceil() - minimum.floor()).max(0.0) as usize + 1;
    let mut occupied = vec![false; length];
    for projection in projections {
        let index = (projection - minimum.floor()).floor() as usize;
        occupied[index.min(length - 1)] = true;
    }
    close_short_projection_gaps(&mut occupied, 2);

    let mut spans = Vec::new();
    let mut index = 0;
    while index < occupied.len() {
        if !occupied[index] {
            index += 1;
            continue;
        }
        let start = index;
        while index < occupied.len() && occupied[index] {
            index += 1;
        }
        spans.push((index - start) as u32);
    }
    let maximum = spans.iter().copied().max()?;
    spans.retain(|span| *span * 2 >= maximum);
    spans.sort_unstable();
    let middle = spans.len() / 2;
    let size = if spans.len().is_multiple_of(2) {
        (spans[middle - 1] + spans[middle]) as f32 * 0.5
    } else {
        spans[middle] as f32
    };
    Some(size.max(1.0))
}

fn close_short_projection_gaps(occupied: &mut [bool], maximum_gap: usize) {
    let mut index = 0;
    while index < occupied.len() {
        if occupied[index] {
            index += 1;
            continue;
        }
        let start = index;
        while index < occupied.len() && !occupied[index] {
            index += 1;
        }
        if start > 0 && index < occupied.len() && index - start <= maximum_gap {
            occupied[start..index].fill(true);
        }
    }
}

fn median_channel(values: &mut [u8]) -> u8 {
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        ((u16::from(values[middle - 1]) + u16::from(values[middle])) / 2) as u8
    } else {
        values[middle]
    }
}

fn normalize_text_color(color: [u8; 3]) -> [u8; 3] {
    if color.iter().copied().max().unwrap_or_default() <= COLOR_SNAP_CHANNEL {
        [0, 0, 0]
    } else if color.iter().copied().min().unwrap_or_default() >= u8::MAX - COLOR_SNAP_CHANNEL {
        [u8::MAX; 3]
    } else {
        color
    }
}

fn rectangle_geometry([left, top, right, bottom]: [f32; 4]) -> Geometry {
    Geometry::rectangle(
        f64::from(left),
        f64::from(top),
        f64::from((right - left).max(1.0)),
        f64::from((bottom - top).max(1.0)),
    )
}

fn rotated_geometry(bounds: [f32; 4], angle_degrees: f32) -> Geometry {
    let mut geometry = rectangle_geometry(bounds);
    if angle_degrees == 0.0 {
        return geometry;
    }
    let [left, top, right, bottom] = bounds.map(f64::from);
    let center_x = (left + right) * 0.5;
    let center_y = (top + bottom) * 0.5;
    let (sin, cos) = f64::from(angle_degrees).to_radians().sin_cos();
    for point in &mut geometry.points {
        let x = point.x - center_x;
        let y = point.y - center_y;
        *point = Point {
            x: center_x + x * cos - y * sin,
            y: center_y + x * sin + y * cos,
        };
    }
    geometry
}

fn write_masks(
    input: &StageInput,
    edit: &mut koharu_scene::SceneEdit,
    page: EntityId,
    detections: &[KoharuLayoutDetection],
    size: ImageSize,
) -> Result<()> {
    for spec in [
        MaskSpec {
            role: "text-mask",
            label: "text",
            dilate: true,
        },
        MaskSpec {
            role: "bubble-mask",
            label: "bubble",
            dilate: false,
        },
    ] {
        write_mask(input, edit, page, detections, spec, size)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct MaskSpec {
    role: &'static str,
    label: &'static str,
    dilate: bool,
}

fn write_mask(
    input: &StageInput,
    edit: &mut koharu_scene::SceneEdit,
    page: EntityId,
    detections: &[KoharuLayoutDetection],
    spec: MaskSpec,
    size: ImageSize,
) -> Result<()> {
    let mut mask = mask_for(detections, spec.label, size);
    if spec.dilate && size.width > 0 && size.height > 0 {
        let radius = ((size.width.max(size.height) as f32 / 1024.0) * 6.0)
            .round()
            .clamp(1.0, 255.0) as u8;
        mask = dilate(&mask, Norm::L2, radius);
    }
    if let Some(bounds) = input.region {
        preserve_mask_outside_region(input, page, spec.role, bounds, &mut mask)?;
    }

    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(mask).write_to(&mut bytes, ImageFormat::Png)?;
    edit.set_asset(
        page,
        &AssetRole::new(spec.role)?,
        AssetInput::new(
            Arc::<[u8]>::from(bytes.into_inner()),
            "image/png",
            AssetMetadata {
                width: Some(size.width),
                height: Some(size.height),
                attributes: BTreeMap::new(),
            },
        ),
    )?;
    Ok(())
}

fn preserve_mask_outside_region(
    input: &StageInput,
    page: EntityId,
    role: &str,
    bounds: crate::Bounds,
    mask: &mut GrayImage,
) -> Result<()> {
    let previous = input
        .images
        .get(&input.scene, page, role)?
        .map(|image| image.to_luma8());
    if previous
        .as_ref()
        .is_some_and(|image| image.dimensions() != mask.dimensions())
    {
        bail!("existing {role} dimensions do not match page {page}");
    }
    for (x, y, pixel) in mask.enumerate_pixels_mut() {
        if f64::from(x + 1) <= bounds.x
            || f64::from(y + 1) <= bounds.y
            || f64::from(x) >= bounds.x + bounds.width
            || f64::from(y) >= bounds.y + bounds.height
        {
            *pixel = previous
                .as_ref()
                .map_or(Luma([0]), |image| *image.get_pixel(x, y));
        }
    }
    Ok(())
}

fn region_kind(label: &str) -> Result<RegionKind> {
    RegionKind::new(match label {
        "text" => "dev.koharu.region.text",
        "bubble" => "dev.koharu.region.bubble",
        "panel" => "dev.koharu.region.panel",
        _ => "dev.koharu.region.unknown",
    })
    .map_err(Into::into)
}

fn mask_for(detections: &[KoharuLayoutDetection], label: &str, size: ImageSize) -> GrayImage {
    let mut mask = GrayImage::new(size.width, size.height);
    for detection in detections.iter().filter(|value| value.label == label) {
        for (target, source) in mask.as_mut().iter_mut().zip(&detection.mask.pixels) {
            if *source != 0 {
                *target = u8::MAX;
            }
        }
    }
    mask
}

fn prepare_detections(detections: &mut Vec<KoharuLayoutDetection>, region: Option<crate::Bounds>) {
    if let Some(region) = region {
        detections.retain(|detection| intersects(detection.bbox, region));
    }
    non_maximum_suppression(detections, 0.5);
    sort_by_layout(detections);
}

fn intersects([left, top, right, bottom]: [f32; 4], region: crate::Bounds) -> bool {
    left < (region.x + region.width) as f32
        && right > region.x as f32
        && top < (region.y + region.height) as f32
        && bottom > region.y as f32
}

fn detection_order(left: &KoharuLayoutDetection, right: &KoharuLayoutDetection) -> Ordering {
    left.bbox[1]
        .total_cmp(&right.bbox[1])
        .then_with(|| right.bbox[0].total_cmp(&left.bbox[0]))
        .then_with(|| left.label.cmp(&right.label))
}

fn non_maximum_suppression(detections: &mut Vec<KoharuLayoutDetection>, threshold: f32) {
    detections.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| detection_order(left, right))
    });
    let mut kept = Vec::with_capacity(detections.len());
    for candidate in detections.drain(..) {
        let suppressed = kept.iter().any(|existing: &KoharuLayoutDetection| {
            existing.label == candidate.label
                && intersection_over_union(existing.bbox, candidate.bbox) > threshold
        });
        if !suppressed {
            kept.push(candidate);
        }
    }
    *detections = kept;
}

fn sort_by_layout(detections: &mut Vec<KoharuLayoutDetection>) {
    let order = layout_order(detections);
    let mut values = std::mem::take(detections)
        .into_iter()
        .map(Some)
        .collect::<Vec<_>>();
    *detections = order
        .into_iter()
        .map(|index| {
            values[index]
                .take()
                .expect("layout order contains each detection once")
        })
        .collect();
}

fn layout_order(detections: &[KoharuLayoutDetection]) -> Vec<usize> {
    let panels = indices_with_label(detections, "panel");
    let bubbles = indices_with_label(detections, "bubble");
    let texts = indices_with_label(detections, "text");
    let panels = spatial_order(detections, panels);

    let mut panel_for_bubble = vec![None; detections.len()];
    for &bubble in &bubbles {
        panel_for_bubble[bubble] = best_container(detections, bubble, &panels);
    }
    let mut bubble_for_text = vec![None; detections.len()];
    let mut panel_for_text = vec![None; detections.len()];
    for &text in &texts {
        let bubble = best_container(detections, text, &bubbles);
        bubble_for_text[text] = bubble;
        panel_for_text[text] = bubble
            .and_then(|bubble| panel_for_bubble[bubble])
            .or_else(|| best_container(detections, text, &panels));
    }

    let mut order = Vec::with_capacity(detections.len());
    let mut included = vec![false; detections.len()];
    for &panel in &panels {
        append_once(panel, &mut order, &mut included);
        for bubble in spatial_order(
            detections,
            bubbles
                .iter()
                .copied()
                .filter(|&bubble| panel_for_bubble[bubble] == Some(panel))
                .collect(),
        ) {
            append_once(bubble, &mut order, &mut included);
            append_texts(detections, &texts, &mut order, &mut included, |text| {
                bubble_for_text[text] == Some(bubble)
            });
        }
        append_texts(detections, &texts, &mut order, &mut included, |text| {
            bubble_for_text[text].is_none() && panel_for_text[text] == Some(panel)
        });
    }

    for bubble in spatial_order(
        detections,
        bubbles
            .iter()
            .copied()
            .filter(|&bubble| panel_for_bubble[bubble].is_none())
            .collect(),
    ) {
        append_once(bubble, &mut order, &mut included);
        append_texts(detections, &texts, &mut order, &mut included, |text| {
            bubble_for_text[text] == Some(bubble)
        });
    }

    append_texts(detections, &texts, &mut order, &mut included, |text| {
        bubble_for_text[text].is_none() && panel_for_text[text].is_none()
    });
    for index in spatial_order(
        detections,
        (0..detections.len())
            .filter(|&index| !included[index])
            .collect(),
    ) {
        append_once(index, &mut order, &mut included);
    }
    order
}

fn indices_with_label(detections: &[KoharuLayoutDetection], label: &str) -> Vec<usize> {
    detections
        .iter()
        .enumerate()
        .filter_map(|(index, detection)| (detection.label == label).then_some(index))
        .collect()
}

fn append_texts(
    detections: &[KoharuLayoutDetection],
    texts: &[usize],
    order: &mut Vec<usize>,
    included: &mut [bool],
    belongs: impl Fn(usize) -> bool,
) {
    for text in spatial_order(
        detections,
        texts
            .iter()
            .copied()
            .filter(|text| belongs(*text))
            .collect(),
    ) {
        append_once(text, order, included);
    }
}

fn append_once(index: usize, order: &mut Vec<usize>, included: &mut [bool]) {
    if !included[index] {
        included[index] = true;
        order.push(index);
    }
}

fn spatial_order(detections: &[KoharuLayoutDetection], mut indices: Vec<usize>) -> Vec<usize> {
    indices.sort_by(|&left, &right| {
        detection_order(&detections[left], &detections[right])
            .then_with(|| detections[right].score.total_cmp(&detections[left].score))
            .then_with(|| left.cmp(&right))
    });
    indices
}

fn best_container(
    detections: &[KoharuLayoutDetection],
    value: usize,
    candidates: &[usize],
) -> Option<usize> {
    candidates
        .iter()
        .copied()
        .filter(|&candidate| containment(detections[candidate].bbox, detections[value].bbox) >= 0.5)
        .min_by(|&left, &right| {
            area(detections[left].bbox)
                .total_cmp(&area(detections[right].bbox))
                .then_with(|| detection_order(&detections[left], &detections[right]))
                .then_with(|| left.cmp(&right))
        })
}

fn containment(container: [f32; 4], value: [f32; 4]) -> f32 {
    let value_area = area(value);
    if value_area <= 0.0 {
        return 0.0;
    }
    intersection_area(container, value) / value_area
}

fn intersection_over_union(left: [f32; 4], right: [f32; 4]) -> f32 {
    let intersection = intersection_area(left, right);
    let union = area(left) + area(right) - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn intersection_area(left: [f32; 4], right: [f32; 4]) -> f32 {
    (left[2].min(right[2]) - left[0].max(right[0])).max(0.0)
        * (left[3].min(right[3]) - left[1].max(right[1])).max(0.0)
}

fn area(bounds: [f32; 4]) -> f32 {
    (bounds[2] - bounds[0]).max(0.0) * (bounds[3] - bounds[1]).max(0.0)
}

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};
    use koharu_ml::koharu_layout_rfdetr_seg_2xl::{KoharuLayoutDetection, KoharuLayoutMask};
    use koharu_scene::WritingMode;

    use super::{
        infer_typography, layout_order, non_maximum_suppression, normalize_text_color,
        rotated_geometry,
    };

    fn detection(label: &str, score: f32, bbox: [f32; 4]) -> KoharuLayoutDetection {
        KoharuLayoutDetection {
            label_id: 0,
            label: label.to_owned(),
            score,
            bbox,
            area: 0,
            mask: KoharuLayoutMask {
                width: 1,
                height: 1,
                pixels: vec![0],
            },
        }
    }

    fn masked_text(
        local_width: f64,
        local_height: f64,
        angle_degrees: f64,
        color: [u8; 3],
    ) -> (RgbImage, KoharuLayoutDetection) {
        let width = 96;
        let height = 96;
        let center_x = f64::from(width) * 0.5;
        let center_y = f64::from(height) * 0.5;
        let (sin, cos) = angle_degrees.to_radians().sin_cos();
        let mut image = RgbImage::from_pixel(width, height, Rgb([200, 180, 160]));
        let mut pixels = vec![0; width as usize * height as usize];
        for y in 0..height {
            for x in 0..width {
                let dx = f64::from(x) + 0.5 - center_x;
                let dy = f64::from(y) + 0.5 - center_y;
                let local_x = dx * cos + dy * sin;
                let local_y = -dx * sin + dy * cos;
                if local_x.abs() <= local_width * 0.5 && local_y.abs() <= local_height * 0.5 {
                    pixels[y as usize * width as usize + x as usize] = u8::MAX;
                    image.put_pixel(x, y, Rgb(color));
                }
            }
        }
        (
            image,
            KoharuLayoutDetection {
                label_id: 0,
                label: "text".to_owned(),
                score: 1.0,
                bbox: [0.0, 0.0, width as f32, height as f32],
                area: pixels.iter().filter(|value| **value != 0).count() as u32,
                mask: KoharuLayoutMask {
                    width,
                    height,
                    pixels,
                },
            },
        )
    }

    #[test]
    fn nms_removes_lower_scored_overlapping_regions_per_class() {
        let mut detections = vec![
            detection("text", 0.8, [5.0, 5.0, 105.0, 105.0]),
            detection("bubble", 0.7, [0.0, 0.0, 100.0, 100.0]),
            detection("text", 0.9, [0.0, 0.0, 100.0, 100.0]),
            detection("text", 0.6, [200.0, 0.0, 250.0, 50.0]),
        ];

        non_maximum_suppression(&mut detections, 0.5);

        let text_scores = detections
            .iter()
            .filter(|detection| detection.label == "text")
            .map(|detection| detection.score)
            .collect::<Vec<_>>();
        assert_eq!(text_scores, [0.9, 0.6]);
        assert!(
            detections
                .iter()
                .any(|detection| detection.label == "bubble")
        );
    }

    #[test]
    fn layout_order_follows_panels_then_bubbles_then_their_text() {
        let detections = vec![
            detection("text", 0.63, [20.0, 30.0, 70.0, 70.0]),
            detection("bubble", 0.7, [10.0, 20.0, 80.0, 80.0]),
            detection("panel", 0.9, [100.0, 0.0, 200.0, 200.0]),
            detection("text", 0.62, [130.0, 110.0, 180.0, 150.0]),
            detection("bubble", 0.8, [120.0, 20.0, 190.0, 80.0]),
            detection("panel", 0.9, [0.0, 0.0, 95.0, 200.0]),
            detection("text", 0.61, [130.0, 30.0, 180.0, 70.0]),
            detection("bubble", 0.7, [120.0, 100.0, 190.0, 160.0]),
        ];

        let text_scores = layout_order(&detections)
            .into_iter()
            .filter_map(|index| {
                (detections[index].label == "text").then_some(detections[index].score)
            })
            .collect::<Vec<_>>();

        assert_eq!(text_scores, [0.61, 0.62, 0.63]);
    }

    #[test]
    fn typography_comes_from_horizontal_text_mask() {
        let (image, detection) = masked_text(52.0, 12.0, 12.0, [24, 80, 160]);

        let inferred = infer_typography(&image, &detection).unwrap();

        assert!((inferred.angle_degrees - 12.0).abs() < 1.0);
        assert!((11.0..=14.0).contains(&inferred.font_size));
        assert_eq!(inferred.color, [24, 80, 160]);
        assert_eq!(inferred.writing_mode, WritingMode::Horizontal);
    }

    #[test]
    fn vertical_text_angle_is_relative_to_upright_vertical() {
        let (image, detection) = masked_text(12.0, 52.0, 9.0, [120, 80, 40]);

        let inferred = infer_typography(&image, &detection).unwrap();

        assert!((inferred.angle_degrees - 9.0).abs() < 1.0);
        assert!((11.0..=14.0).contains(&inferred.font_size));
        assert_eq!(inferred.writing_mode, WritingMode::Vertical);
    }

    #[test]
    fn near_neutral_extremes_snap_to_full_black_or_white() {
        assert_eq!(normalize_text_color([20, 31, 24]), [0, 0, 0]);
        assert_eq!(normalize_text_color([230, 240, 250]), [255, 255, 255]);
        assert_eq!(normalize_text_color([33, 33, 33]), [33, 33, 33]);
        assert_eq!(normalize_text_color([222, 240, 250]), [222, 240, 250]);
    }

    #[test]
    fn inferred_angle_rotates_region_geometry() {
        let geometry = rotated_geometry([10.0, 20.0, 70.0, 40.0], 15.0);
        let top = (
            geometry.points[1].x - geometry.points[0].x,
            geometry.points[1].y - geometry.points[0].y,
        );

        assert!((top.1.atan2(top.0).to_degrees() - 15.0).abs() < 1e-6);
    }
}
