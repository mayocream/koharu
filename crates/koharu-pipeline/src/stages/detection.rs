use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat, Luma, RgbImage};
use imageproc::{
    contours::{BorderType, find_contours_with_threshold},
    distance_transform::Norm,
    geometry::{approximate_polygon_dp, arc_length, contour_area},
    morphology::dilate,
};
use koharu_ml::koharu_layout_rfdetr_seg_2xl::{
    KoharuLayoutDetection, KoharuLayoutDetections, KoharuLayoutMask, KoharuLayoutRFDetrSeg2XL,
    KoharuLayoutThresholds,
};
use koharu_scene::{
    AssetInput, AssetMetadata, AssetRole, At, BubbleRegion, DetectionAnalysis, DetectionLabel,
    EntityId, EntityOrigin, FitsTo, Generation, Geometry, Inside, Origin, PanelRegion, Point,
    Presents, RecognizedFrom, Region, RegionKind, RegionSpec, RemovePolicy, TextLayout,
    TextLayoutKind, TextRegion, TextRole, Typography, WritingMode,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{ModelRef, StageInput, StageProcessor, finish, generation};
use crate::{DetectionModel, ModelCell};

const MODEL_ID: &str = "mayocream/koharu-layout-rfdetr-seg-2xl-1152";
const MODEL_NAME: &str = "koharu-layout-rfdetr-seg-2xl";
const PRODUCER: &str = "dev.koharu.pipeline.detection";
const ANGLE_SNAP_DEGREES: f32 = 3.0;
const ANGLE_SEARCH_HALF_STEPS: i32 = 90;
const ANGLE_SEARCH_STEP_DEGREES: f64 = 0.5;
const COLOR_SNAP_CHROMA: u8 = 24;
const COLOR_SNAP_DARK_LUMINANCE: u16 = 64;
const COLOR_SNAP_LIGHT_LUMINANCE: u16 = 191;
const NMS_CONTAINMENT_THRESHOLD: f32 = 0.9;

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

    async fn process(&self, input: StageInput) -> Result<koharu_scene::Patch> {
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

    async fn run(&self, input: StageInput) -> Result<koharu_scene::Patch> {
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

#[derive(Clone, Copy)]
struct DetectedText {
    region: DetectedRegion,
    content: EntityId,
    layer: EntityId,
}

#[derive(Clone)]
struct PreviousText {
    bounds: [f32; 4],
    geometry: Geometry,
    content: EntityId,
    layer: EntityId,
}

#[derive(Default)]
struct PageRegions {
    bubbles: Vec<DetectedRegion>,
    texts: Vec<DetectedText>,
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
) -> Result<koharu_scene::Patch> {
    let page = input.page;
    let mut previous_texts = previous_texts(input, generation)?;
    let mut reused_contents = BTreeSet::new();
    let mut edit = input.scene.edit_as(generation.clone());
    edit.observe_subtree(page)?;
    remove_previous_regions(input, &mut edit, generation)?;
    write_page(
        input,
        &mut edit,
        page,
        image,
        output,
        generation,
        &mut previous_texts,
        &mut reused_contents,
    )?;
    remove_unmatched_texts(
        input,
        &mut edit,
        generation,
        previous_texts,
        &reused_contents,
    )?;
    finish(edit)
}

fn remove_previous_regions(
    input: &StageInput,
    edit: &mut koharu_scene::Edit,
    generation: &Generation,
) -> Result<()> {
    let mut remove = Vec::new();
    for entity in input.scene.descendants(input.page)? {
        let id = entity.id();
        if !input.contains_entity(id)? {
            continue;
        }
        let owned_region = entity
            .component::<EntityOrigin>()?
            .is_some_and(|origin| {
                matches!(origin.origin, Origin::Generated(ref owner) if owner.producer == generation.producer)
            })
            && entity.component::<Region>()?.is_some();
        if owned_region {
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

fn previous_texts(input: &StageInput, generation: &Generation) -> Result<Vec<PreviousText>> {
    let mut previous = Vec::new();
    for entity in input.scene.descendants(input.page)? {
        let region = entity.id();
        if !input.contains_entity(region)? {
            continue;
        }
        let owned_text_region = entity
            .component::<EntityOrigin>()?
            .is_some_and(|origin| {
                matches!(origin.origin, Origin::Generated(ref owner) if owner.producer == generation.producer)
            })
            && entity
                .component::<Region>()?
                .is_some_and(|value| value.kind == TextRegion::kind());
        if !owned_text_region {
            continue;
        }
        let Some(geometry) = entity.component::<Geometry>()? else {
            continue;
        };
        let Some(bounds) = geometry_bounds(&geometry) else {
            continue;
        };
        for recognized in input.scene.relations_to_as::<RecognizedFrom>(region) {
            let content = recognized.value().source;
            for presentation in input.scene.relations_to_as::<Presents>(content) {
                previous.push(PreviousText {
                    bounds,
                    geometry: geometry.clone(),
                    content,
                    layer: presentation.value().source,
                });
            }
        }
    }
    Ok(previous)
}

fn remove_unmatched_texts(
    input: &StageInput,
    edit: &mut koharu_scene::Edit,
    generation: &Generation,
    previous: Vec<PreviousText>,
    reused_contents: &BTreeSet<EntityId>,
) -> Result<()> {
    let mut contents = BTreeSet::new();
    for previous in previous {
        contents.insert(previous.content);
        let layer_generated = input
            .scene
            .component::<EntityOrigin>(previous.layer)?
            .is_some_and(|origin| {
                matches!(origin.origin, Origin::Generated(ref owner) if owner.producer == generation.producer)
            });
        if layer_generated {
            edit.remove_entity(previous.layer, RemovePolicy::Cascade)?;
        } else if input.scene.component::<Geometry>(previous.layer)?.is_none() {
            let mut geometry = previous.geometry;
            geometry.origin = Origin::User;
            edit.set(previous.layer, &geometry)?;
        }
    }
    for content in contents {
        if reused_contents.contains(&content) {
            continue;
        }
        let content_generated = input
            .scene
            .component::<EntityOrigin>(content)?
            .is_some_and(|origin| {
                matches!(origin.origin, Origin::Generated(ref owner) if owner.producer == generation.producer)
            });
        if content_generated {
            edit.remove_entity(content, RemovePolicy::Cascade)?;
        }
    }
    Ok(())
}

fn write_page(
    input: &StageInput,
    edit: &mut koharu_scene::Edit,
    page: EntityId,
    image: &DynamicImage,
    output: KoharuLayoutDetections,
    generation: &Generation,
    previous_texts: &mut Vec<PreviousText>,
    reused_contents: &mut BTreeSet<EntityId>,
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
    if let Some(region) = input.region {
        detections.retain(|detection| intersects(detection.bbox, region));
    }
    non_maximum_suppression(&mut detections, 0.5);
    sort_by_layout(&mut detections);

    let image = image.to_rgb8();
    let regions = write_regions(
        &input.scene,
        edit,
        page,
        &image,
        &detections,
        generation,
        previous_texts,
        reused_contents,
    )?;
    link_dialogue_regions(edit, &regions, generation)?;
    write_masks(input, edit, page, &detections, size)
}

fn write_regions(
    snapshot: &koharu_scene::Snapshot,
    edit: &mut koharu_scene::Edit,
    page: EntityId,
    image: &RgbImage,
    detections: &[KoharuLayoutDetection],
    generation: &Generation,
    previous_texts: &mut Vec<PreviousText>,
    reused_contents: &mut BTreeSet<EntityId>,
) -> Result<PageRegions> {
    let mut regions = PageRegions::default();
    let text_group = snapshot.page(page)?.text_group()?;
    let managed_text_group = if let Some(group) = text_group {
        snapshot
            .component::<EntityOrigin>(group.id())?
            .is_some_and(|origin| origin.origin != Origin::User)
            .then_some(group.id())
    } else {
        None
    };
    for detection in detections {
        let previous = (detection.label == "text")
            .then(|| take_previous_text(previous_texts, detection.bbox))
            .flatten();
        let (detected, text) = write_region(
            snapshot,
            edit,
            page,
            image,
            detection,
            generation,
            previous,
            reused_contents,
        )?;
        match detection.label.as_str() {
            "bubble" => regions.bubbles.push(detected),
            "text" => {
                let text = text.expect("text detections create text semantics");
                if let Some(group) = managed_text_group {
                    edit.move_entity(text.layer, Some(group), At::End)?;
                }
                regions.texts.push(text);
            }
            _ => {}
        }
    }
    Ok(regions)
}

fn write_region(
    snapshot: &koharu_scene::Snapshot,
    edit: &mut koharu_scene::Edit,
    page: EntityId,
    image: &RgbImage,
    detection: &KoharuLayoutDetection,
    generation: &Generation,
    previous: Option<PreviousText>,
    reused_contents: &mut BTreeSet<EntityId>,
) -> Result<(DetectedRegion, Option<DetectedText>)> {
    let entity = edit.add_entity(page, At::End)?;
    let kind = region_kind(&detection.label)?;
    let inferred = (detection.label == "text")
        .then(|| infer_typography(image, detection))
        .flatten();
    let geometry = if detection.label == "bubble" {
        mask_geometry(&detection.mask).unwrap_or_else(|| rectangle_geometry(detection.bbox))
    } else if detection.label == "text" {
        inferred.map_or_else(
            || rectangle_geometry(detection.bbox),
            |typography| rotated_rectangle_geometry(detection.bbox, typography.angle_degrees),
        )
    } else {
        rectangle_geometry(detection.bbox)
    };
    edit.set(entity, &geometry)?;
    edit.set(
        entity,
        &Region {
            origin: Origin::Generated(generation.clone()),
            kind: kind.clone(),
            label: Some(detection.label.clone()),
        },
    )?;
    edit.set(
        entity,
        &DetectionAnalysis {
            origin: Origin::Generated(generation.clone()),
            labels: vec![DetectionLabel {
                kind,
                confidence: detection.score,
            }],
        },
    )?;
    let region = DetectedRegion {
        entity,
        bounds: detection.bbox,
    };
    if detection.label != "text" {
        return Ok((region, None));
    }

    let (content, layer, created) = previous.map_or_else(
        || -> Result<_> {
            let content = edit.add_text_content(page, At::End)?;
            let layer = edit.add_text_layer(
                page,
                At::End,
                content,
                &TextLayout {
                    origin: Origin::Generated(generation.clone()),
                    kind: TextLayoutKind::Paragraph,
                },
            )?;
            Ok((content, layer, true))
        },
        |previous| Ok((previous.content, previous.layer, false)),
    )?;
    if !created {
        reused_contents.insert(content);
    }
    if created
        || snapshot
            .component::<TextRole>(content)?
            .is_none_or(|value| value.origin != Origin::User)
    {
        write_text_role(edit, content, "dev.koharu.text.free-text", generation)?;
    }
    if created || snapshot.component::<TextLayout>(layer)?.is_none() {
        edit.set(
            layer,
            &TextLayout {
                origin: Origin::Generated(generation.clone()),
                kind: TextLayoutKind::Paragraph,
            },
        )?;
    }
    if created
        || snapshot
            .component::<Typography>(layer)?
            .is_none_or(|value| value.origin != Origin::User)
    {
        edit.set(
            layer,
            &Typography {
                origin: Origin::Generated(generation.clone()),
                preferred_font: None,
                font_weight: None,
                font_style: None,
                size: inferred.map(|value| value.font_size),
                auto_fit: true,
                color: inferred
                    .map(|value| [value.color[0], value.color[1], value.color[2], u8::MAX]),
                stroke_color: None,
                stroke_width: None,
                alignment: None,
                writing_mode: inferred.map(|value| value.writing_mode),
                extensions: Default::default(),
            },
        )?;
    }
    edit.relate::<RecognizedFrom>(content, entity)?;

    Ok((
        region,
        Some(DetectedText {
            region,
            content,
            layer,
        }),
    ))
}

fn link_dialogue_regions(
    edit: &mut koharu_scene::Edit,
    regions: &PageRegions,
    generation: &Generation,
) -> Result<()> {
    for text in &regions.texts {
        let bubble = containing_bubble(&regions.bubbles, text.region.bounds);
        if let Some(bubble) = bubble {
            edit.relate::<Inside>(text.region.entity, bubble.entity)?;
            edit.relate::<FitsTo>(text.layer, bubble.entity)?;
            write_text_role(edit, text.content, "dev.koharu.text.dialogue", generation)?;
        } else {
            edit.relate::<FitsTo>(text.layer, text.region.entity)?;
        }
    }
    Ok(())
}

fn containing_bubble(bubbles: &[DetectedRegion], bounds: [f32; 4]) -> Option<&DetectedRegion> {
    bubbles
        .iter()
        .filter(|bubble| containment(bubble.bounds, bounds) >= 0.5)
        .min_by(|left, right| area(left.bounds).total_cmp(&area(right.bounds)))
}

fn take_previous_text(previous: &mut Vec<PreviousText>, bounds: [f32; 4]) -> Option<PreviousText> {
    let (index, overlap) = previous
        .iter()
        .enumerate()
        .map(|(index, previous)| (index, overlap_over_smaller(previous.bounds, bounds)))
        .max_by(|left, right| left.1.total_cmp(&right.1))?;
    (overlap >= 0.5).then(|| previous.swap_remove(index))
}

fn geometry_bounds(geometry: &Geometry) -> Option<[f32; 4]> {
    let first = geometry.points.first()?;
    let (mut left, mut top, mut right, mut bottom) = (first.x, first.y, first.x, first.y);
    for point in &geometry.points[1..] {
        left = left.min(point.x);
        top = top.min(point.y);
        right = right.max(point.x);
        bottom = bottom.max(point.y);
    }
    Some([left as f32, top as f32, right as f32, bottom as f32])
}

fn write_text_role(
    edit: &mut koharu_scene::Edit,
    entity: EntityId,
    role: &str,
    generation: &Generation,
) -> Result<()> {
    edit.set(
        entity,
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
// RF-DETR provides foreground pixels rather than line quadrilaterals, so
// projection-profile sharpness supplies the line axis and the mask projection
// supplies its cross span. Whole-block PCA is deliberately avoided because a
// tall multiline horizontal block otherwise looks vertical.
fn infer_typography(
    image: &RgbImage,
    detection: &KoharuLayoutDetection,
) -> Option<InferredTypography> {
    let mask = &detection.mask;
    let width = image.width().min(mask.width);
    let height = image.height().min(mask.height);
    let [left, top, right, bottom] = mask_window(detection.bbox, width, height)?;
    let mut points = Vec::new();
    let mut foreground = Vec::new();
    let mut background = Vec::new();
    for y in top..bottom {
        let row = y as usize * mask.width as usize;
        for x in left..right {
            let color = image.get_pixel(x, y).0;
            if mask.pixels.get(row + x as usize).copied().unwrap_or(0) == 0 {
                background.push(color);
                continue;
            }
            points.push(MaskPoint {
                x: f64::from(x) + 0.5,
                y: f64::from(y) + 0.5,
            });
            foreground.push(color);
        }
    }
    if points.is_empty() {
        return None;
    }

    let (angle_degrees, vertical) = mask_angle(&points, detection.bbox);
    let font_size = mask_font_size(&points, angle_degrees, vertical)?;
    let color = infer_text_color(&foreground, &background);
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
    let mut horizontal = (f64::NEG_INFINITY, 0.0);
    let mut vertical = (f64::NEG_INFINITY, 0.0);
    for step in -ANGLE_SEARCH_HALF_STEPS..=ANGLE_SEARCH_HALF_STEPS {
        let angle_degrees = f64::from(step) * ANGLE_SEARCH_STEP_DEGREES;
        let (sin, cos) = angle_degrees.to_radians().sin_cos();
        let horizontal_score = projection_score(points, -sin, cos);
        if horizontal_score > horizontal.0 {
            horizontal = (horizontal_score, angle_degrees);
        }
        let vertical_score = projection_score(points, cos, sin);
        if vertical_score > vertical.0 {
            vertical = (vertical_score, angle_degrees);
        }
    }
    let maximum_score = horizontal.0.max(vertical.0);
    let scores_are_close = (horizontal.0 - vertical.0).abs() <= maximum_score * 0.02;
    let is_vertical = if scores_are_close {
        bottom - top > right - left
    } else {
        vertical.0 > horizontal.0
    };
    let mut angle = if is_vertical {
        vertical.1
    } else {
        horizontal.1
    } as f32;
    if angle.abs() < ANGLE_SNAP_DEGREES {
        angle = 0.0;
    }
    (angle, is_vertical)
}

fn projection_score(points: &[MaskPoint], axis_x: f64, axis_y: f64) -> f64 {
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    for point in points {
        let projection = point.x * axis_x + point.y * axis_y;
        minimum = minimum.min(projection);
        maximum = maximum.max(projection);
    }
    let origin = minimum.floor();
    let length = (maximum.ceil() - origin).max(0.0) as usize + 2;
    let mut profile = vec![0.0; length];
    for point in points {
        let projection = point.x * axis_x + point.y * axis_y - origin;
        let index = projection.floor() as usize;
        let fraction = projection - index as f64;
        profile[index] += 1.0 - fraction;
        profile[index + 1] += fraction;
    }
    profile.iter().map(|value| value * value).sum::<f64>() / points.len() as f64
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

// BallonsTranslator normally receives foreground colors predicted per OCR line:
// https://github.com/dmMaze/BallonsTranslator/blob/4bcc635c19f6c63a902872cf77b3d554e14ed1b7/ballontranslator/modules/ocr/mit48px.py#L202-L210
// This detector has only a segmentation mask, so the most background-distant
// mask pixels approximate the solid glyph core without antialiasing bias.
fn infer_text_color(foreground: &[[u8; 3]], background: &[[u8; 3]]) -> [u8; 3] {
    if background.is_empty() {
        return normalize_text_color(median_color(foreground));
    }
    let background = median_color(background);
    let mut foreground = foreground.to_vec();
    foreground.sort_unstable_by(|left, right| {
        color_distance_squared(*right, background).cmp(&color_distance_squared(*left, background))
    });
    let core = foreground.len().div_ceil(4).max(1);
    normalize_text_color(median_color(&foreground[..core]))
}

fn median_color(colors: &[[u8; 3]]) -> [u8; 3] {
    std::array::from_fn(|channel| {
        let mut values = colors
            .iter()
            .map(|color| color[channel])
            .collect::<Vec<_>>();
        median_channel(&mut values)
    })
}

fn color_distance_squared(left: [u8; 3], right: [u8; 3]) -> u32 {
    left.into_iter()
        .zip(right)
        .map(|(left, right)| i32::from(left) - i32::from(right))
        .map(|difference| difference.unsigned_abs().pow(2))
        .sum()
}

fn normalize_text_color(color: [u8; 3]) -> [u8; 3] {
    let minimum = color.iter().copied().min().unwrap_or_default();
    let maximum = color.iter().copied().max().unwrap_or_default();
    let luminance = color.iter().copied().map(u16::from).sum::<u16>() / 3;
    if maximum - minimum <= COLOR_SNAP_CHROMA && luminance <= COLOR_SNAP_DARK_LUMINANCE {
        [0, 0, 0]
    } else if maximum - minimum <= COLOR_SNAP_CHROMA && luminance >= COLOR_SNAP_LIGHT_LUMINANCE {
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

fn rotated_rectangle_geometry(
    [left, top, right, bottom]: [f32; 4],
    angle_degrees: f32,
) -> Geometry {
    let width = f64::from((right - left).max(1.0));
    let height = f64::from((bottom - top).max(1.0));
    let center_x = f64::from(left + right) * 0.5;
    let center_y = f64::from(top + bottom) * 0.5;
    let (sin, cos) = f64::from(angle_degrees).to_radians().sin_cos();
    Geometry {
        origin: Origin::User,
        points: [
            (-width * 0.5, -height * 0.5),
            (width * 0.5, -height * 0.5),
            (width * 0.5, height * 0.5),
            (-width * 0.5, height * 0.5),
        ]
        .map(|(x, y)| Point {
            x: center_x + x * cos - y * sin,
            y: center_y + x * sin + y * cos,
        })
        .into(),
    }
}

fn mask_geometry(mask: &KoharuLayoutMask) -> Option<Geometry> {
    let mask = GrayImage::from_raw(mask.width, mask.height, mask.pixels.clone())?;
    let mut padded = GrayImage::new(mask.width() + 2, mask.height() + 2);
    image::imageops::replace(&mut padded, &mask, 1, 1);
    let contours = find_contours_with_threshold::<i32>(&padded, 0);
    let contour = contours
        .iter()
        .filter(|contour| contour.border_type == BorderType::Outer)
        .max_by(|left, right| {
            contour_area(&left.points)
                .partial_cmp(&contour_area(&right.points))
                .unwrap_or(Ordering::Equal)
        })?;
    if contour.points.len() < 3 {
        return None;
    }

    let epsilon = (arc_length(&contour.points, true) * 0.001).max(f64::EPSILON);
    let points = approximate_polygon_dp(&contour.points, epsilon, true)
        .into_iter()
        .map(|point| Point {
            x: f64::from(point.x - 1),
            y: f64::from(point.y - 1),
        })
        .collect::<Vec<_>>();
    (points.len() >= 3).then_some(Geometry {
        origin: Origin::User,
        points,
    })
}

fn write_masks(
    input: &StageInput,
    edit: &mut koharu_scene::Edit,
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
    edit: &mut koharu_scene::Edit,
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
        "text" => TextRegion::KIND,
        "bubble" => BubbleRegion::KIND,
        "panel" => PanelRegion::KIND,
        _ => "dev.koharu.region.unknown",
    })
    .map_err(Into::into)
}

fn mask_for(detections: &[KoharuLayoutDetection], label: &str, size: ImageSize) -> GrayImage {
    let mut mask = GrayImage::new(size.width, size.height);
    for detection in detections.iter().filter(|value| {
        value.label == label
            || (label == "text"
                && value.label == "onomatopoeia"
                && detections.iter().any(|bubble| {
                    bubble.label == "bubble" && containment(bubble.bbox, value.bbox) >= 0.5
                }))
    }) {
        for (target, source) in mask.as_mut().iter_mut().zip(&detection.mask.pixels) {
            if *source != 0 {
                *target = u8::MAX;
            }
        }
    }
    mask
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
                && (intersection_over_union(existing.bbox, candidate.bbox) >= threshold
                    || overlap_over_smaller(existing.bbox, candidate.bbox)
                        >= NMS_CONTAINMENT_THRESHOLD)
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

fn overlap_over_smaller(left: [f32; 4], right: [f32; 4]) -> f32 {
    let smaller = area(left).min(area(right));
    if smaller <= 0.0 {
        0.0
    } else {
        intersection_area(left, right) / smaller
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
        ImageSize, infer_typography, layout_order, mask_for, mask_geometry,
        non_maximum_suppression, normalize_text_color,
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

    #[test]
    fn bubble_geometry_follows_the_instance_mask_polygon() {
        let width = 9;
        let height = 9;
        let mut pixels = vec![0; width * height];
        for y in 0..height {
            for x in 0..width {
                if x.abs_diff(4) + y.abs_diff(4) <= 4 {
                    pixels[y * width + x] = u8::MAX;
                }
            }
        }

        let geometry = mask_geometry(&KoharuLayoutMask {
            width: width as u32,
            height: height as u32,
            pixels,
        })
        .unwrap();

        assert!(geometry.points.len() >= 4);
        assert!(
            geometry
                .points
                .iter()
                .all(|point| !(point.x == 0.0 && point.y == 0.0))
        );
        assert!(
            geometry
                .points
                .iter()
                .any(|point| point.x == 4.0 && point.y == 0.0)
        );
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
            detection("text", 0.5, [20.0, 20.0, 80.0, 80.0]),
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
    fn text_mask_includes_only_onomatopoeia_contained_by_a_bubble() {
        let detection = |label: &str, bbox: [f32; 4], pixels: [u8; 4]| KoharuLayoutDetection {
            label_id: 0,
            label: label.to_owned(),
            score: 1.0,
            bbox,
            area: pixels.iter().filter(|value| **value != 0).count() as u32,
            mask: KoharuLayoutMask {
                width: 4,
                height: 1,
                pixels: pixels.to_vec(),
            },
        };
        let detections = vec![
            detection("bubble", [0.0, 0.0, 3.0, 1.0], [0, 0, 0, 0]),
            detection("onomatopoeia", [0.0, 0.0, 1.0, 1.0], [255, 0, 0, 0]),
            detection("text", [1.0, 0.0, 2.0, 1.0], [0, 255, 0, 0]),
            detection("onomatopoeia", [3.0, 0.0, 4.0, 1.0], [0, 0, 0, 255]),
        ];

        let mask = mask_for(
            &detections,
            "text",
            ImageSize {
                width: 4,
                height: 1,
            },
        );

        assert_eq!(mask.as_raw(), &[255, 255, 0, 0]);
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
    fn tall_multiline_mask_uses_text_lines_instead_of_block_aspect() {
        let width = 96;
        let height = 96;
        let angle_degrees = 8.0_f64;
        let (sin, cos) = angle_degrees.to_radians().sin_cos();
        let mut image = RgbImage::from_pixel(width, height, Rgb([240, 240, 240]));
        let mut pixels = vec![0; width as usize * height as usize];
        for y in 0..height {
            for x in 0..width {
                let dx = f64::from(x) + 0.5 - f64::from(width) * 0.5;
                let dy = f64::from(y) + 0.5 - f64::from(height) * 0.5;
                let local_x = dx * cos + dy * sin;
                let local_y = -dx * sin + dy * cos;
                let inside = [-24.0, 0.0, 24.0]
                    .into_iter()
                    .any(|line_y| local_x.abs() <= 15.0 && (local_y - line_y).abs() <= 2.0);
                if inside {
                    pixels[y as usize * width as usize + x as usize] = u8::MAX;
                    image.put_pixel(x, y, Rgb([8, 8, 8]));
                }
            }
        }
        let detection = KoharuLayoutDetection {
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
        };

        let inferred = infer_typography(&image, &detection).unwrap();

        assert_eq!(inferred.writing_mode, WritingMode::Horizontal);
        assert!((inferred.angle_degrees - 8.0).abs() < 1.0);
    }

    #[test]
    fn antialiased_dark_text_uses_the_high_contrast_core() {
        let (mut image, detection) = masked_text(52.0, 12.0, 0.0, [96, 94, 92]);
        for (index, &mask) in detection.mask.pixels.iter().enumerate() {
            if mask != 0 && index.is_multiple_of(3) {
                let x = index as u32 % image.width();
                let y = index as u32 / image.width();
                image.put_pixel(x, y, Rgb([8, 9, 7]));
            }
        }

        let inferred = infer_typography(&image, &detection).unwrap();

        assert_eq!(inferred.color, [0, 0, 0]);
    }

    #[test]
    fn near_neutral_extremes_snap_to_full_black_or_white() {
        assert_eq!(normalize_text_color([20, 31, 24]), [0, 0, 0]);
        assert_eq!(normalize_text_color([230, 240, 250]), [255, 255, 255]);
        assert_eq!(normalize_text_color([33, 33, 33]), [0, 0, 0]);
        assert_eq!(normalize_text_color([205, 210, 216]), [255, 255, 255]);
        assert_eq!(normalize_text_color([40, 70, 40]), [40, 70, 40]);
    }
}
