// Pipeline adapter for the commit-pinned Koharu Layout RF-DETR implementation:
// https://huggingface.co/mayocream/koharu-layout-rfdetr-seg-2xl-1152/tree/aed55fdb8ca953c6bec33cf6ed6dd52a9b72bfa2

use std::{
    collections::HashSet,
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow, bail, ensure};
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use imageproc::{
    distance_transform::Norm,
    morphology::dilate,
    region_labelling::{Connectivity, connected_components},
};
use koharu_ml::koharu_layout_rfdetr_seg_2xl::{
    KoharuLayoutDetection, KoharuLayoutDetections, KoharuLayoutRFDetrSeg2XL, KoharuLayoutThresholds,
};
use koharu_scene::{
    Command, ElementId, ElementKind, Frame, ModelPrediction, PageAsset, PageId, Region, RegionKind,
    SourceText, TextAnalysis, TextBlock, TextDirection, TextRole,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Context, Processor};

const MODEL_ID: &str = "mayocream/koharu-layout-rfdetr-seg-2xl-1152";
const TEXT_MASK_REFERENCE_LONG_SIDE: u32 = 1024;
const TEXT_MASK_DILATION_RADIUS: u32 = 6;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct KoharuLayoutRFDetrSeg2XLConfig {
    pub text_threshold: Option<f32>,
    pub bubble_threshold: Option<f32>,
    pub panel_threshold: Option<f32>,
}

pub(super) struct KoharuLayoutRFDetrSeg2XLProcessor {
    model: Arc<Mutex<KoharuLayoutRFDetrSeg2XL>>,
    thresholds: KoharuLayoutThresholds,
}

impl KoharuLayoutRFDetrSeg2XLProcessor {
    pub(super) async fn load(
        device: koharu_ml::Device,
        config: &KoharuLayoutRFDetrSeg2XLConfig,
    ) -> Result<Self> {
        for (class, threshold) in [
            ("text", config.text_threshold),
            ("bubble", config.bubble_threshold),
            ("panel", config.panel_threshold),
        ] {
            if let Some(threshold) = threshold {
                ensure!(
                    (0.0..=1.0).contains(&threshold),
                    "{class} confidence threshold must be between 0 and 1"
                );
            }
        }
        let model = KoharuLayoutRFDetrSeg2XL::load(device).await?;
        let mut thresholds = model.recommended_thresholds();
        if let Some(threshold) = config.text_threshold {
            thresholds.text = threshold;
        }
        if let Some(threshold) = config.bubble_threshold {
            thresholds.bubble = threshold;
        }
        if let Some(threshold) = config.panel_threshold {
            thresholds.panel = threshold;
        }
        Ok(Self {
            model: Arc::new(Mutex::new(model)),
            thresholds,
        })
    }
}

#[async_trait]
impl Processor for KoharuLayoutRFDetrSeg2XLProcessor {
    async fn run(&mut self, context: &Context) -> Result<koharu_scene::Commands> {
        let inputs = context
            .pages()
            .iter()
            .map(|page| page_input(context, page.id))
            .collect::<Result<Vec<_>>>()?;
        let thresholds = self.thresholds;
        let model = self.model.clone();
        let outputs = tokio::task::spawn_blocking(move || {
            let model = model
                .lock()
                .map_err(|_| anyhow!("Koharu Layout RF-DETR model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let output = model.inference_with_thresholds(&input.image, thresholds)?;
                    Ok((input, output))
                })
                .collect::<Result<Vec<_>>>()
        })
        .await??;

        let mut commands = context.commands();
        for (input, output) in outputs {
            let page = context.page(input.page).expect("captured page");
            for element in &page.elements {
                let predictions = match &element.kind {
                    ElementKind::Text(text) => &text.predictions,
                    ElementKind::Region(region) => &region.predictions,
                    ElementKind::Image(_) => continue,
                };
                if predictions
                    .iter()
                    .any(|prediction| prediction.model == MODEL_ID)
                    && context.includes_element(input.page, element.id, element.frame)
                {
                    commands.push(Command::DeleteElement {
                        page: input.page,
                        element: element.id,
                    });
                }
            }

            let mut analysis = analyze(output, input.area);
            remap_bubble_mask_ids(&mut analysis, page, context)?;
            for panel in &analysis.panels {
                commands.add_region(
                    input.page,
                    panel.frame,
                    Region {
                        kind: RegionKind::Panel,
                        polygon: Vec::new(),
                        mask_id: None,
                        reading_order: Some(panel.order),
                        predictions: vec![ModelPrediction::new(MODEL_ID, panel.score)],
                    },
                );
            }
            let panel_ids = inserted_region_ids(&commands, input.page, RegionKind::Panel);

            for bubble in &analysis.bubbles {
                commands.add_region(
                    input.page,
                    bubble.frame,
                    Region {
                        kind: RegionKind::Bubble,
                        polygon: Vec::new(),
                        mask_id: Some(bubble.mask_id),
                        reading_order: Some(bubble.order),
                        predictions: vec![ModelPrediction::new(MODEL_ID, bubble.score)],
                    },
                );
            }
            let bubble_ids = inserted_region_ids(&commands, input.page, RegionKind::Bubble);

            let mut existing_texts = page
                .texts()
                .filter(|(element, text)| {
                    !text
                        .predictions
                        .iter()
                        .any(|prediction| prediction.model == MODEL_ID)
                        && context.includes_element(page.id, element.id, element.frame)
                })
                .collect::<Vec<_>>();
            existing_texts.sort_by(|(left, _), (right, _)| {
                manga_position(frame_box(left.frame), frame_box(right.frame))
            });
            for (order, (element, text)) in existing_texts.into_iter().enumerate() {
                let panel = best_container(
                    element.frame,
                    analysis.panels.iter().map(|region| region.frame),
                )
                .and_then(|index| panel_ids.get(index).copied());
                let bubble = best_container(
                    element.frame,
                    analysis.bubbles.iter().map(|region| region.frame),
                )
                .and_then(|index| bubble_ids.get(index).copied());
                let mut metadata = TextAnalysis::from(text);
                metadata.panel = panel;
                metadata.bubble = bubble;
                metadata.reading_order = Some(order as u32);
                if bubble.is_some() && metadata.role == TextRole::FreeText {
                    metadata.role = TextRole::Dialogue;
                }
                commands.push(Command::EditElement {
                    page: input.page,
                    element: element.id,
                    edit: koharu_scene::ElementChange::Analysis(metadata),
                });
            }

            for text in &analysis.texts {
                let panel = best_container(
                    text.frame,
                    analysis.panels.iter().map(|region| region.frame),
                )
                .and_then(|index| panel_ids.get(index).copied());
                let bubble = best_container(
                    text.frame,
                    analysis.bubbles.iter().map(|region| region.frame),
                )
                .and_then(|index| bubble_ids.get(index).copied());
                let role = if bubble.is_some() {
                    TextRole::Dialogue
                } else {
                    TextRole::FreeText
                };
                let block = TextBlock {
                    role,
                    panel,
                    bubble,
                    reading_order: Some(text.order),
                    source: Some(SourceText {
                        text: String::new(),
                        language: None,
                        direction: if text.frame.height >= text.frame.width * 1.15 {
                            TextDirection::Vertical
                        } else {
                            TextDirection::Horizontal
                        },
                        confidence: None,
                        lines: Vec::new(),
                    }),
                    predictions: vec![ModelPrediction::new(MODEL_ID, text.score)],
                    ..TextBlock::default()
                };
                commands.add_text_block(input.page, text.frame, block);
            }

            for (asset, mask) in [
                (PageAsset::TextMask, analysis.text_mask),
                (PageAsset::BubbleMask, analysis.bubble_mask),
            ] {
                let mask = patch_mask(context, input.page, asset, input.area, mask)?;
                commands.set_asset(input.page, asset, Some(encode(mask)?))?;
            }
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

fn page_input(context: &Context, page: PageId) -> Result<PageInput> {
    let source = context.source(page)?;
    let area = if let Some(region) = context.region(page) {
        let x = (region.x.floor().max(0.0) as u32).min(source.width());
        let y = (region.y.floor().max(0.0) as u32).min(source.height());
        let right = ((region.x + region.width).ceil().max(0.0) as u32).min(source.width());
        let bottom = ((region.y + region.height).ceil().max(0.0) as u32).min(source.height());
        if right <= x || bottom <= y {
            bail!("pipeline region does not overlap page {page}");
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
    Ok(PageInput { page, image, area })
}

struct Analysis {
    panels: Vec<DetectedRegion>,
    bubbles: Vec<DetectedBubble>,
    texts: Vec<DetectedText>,
    text_mask: GrayImage,
    bubble_mask: GrayImage,
}

struct DetectedRegion {
    frame: Frame,
    score: f32,
    order: u32,
}

struct DetectedBubble {
    frame: Frame,
    score: f32,
    order: u32,
    mask_id: u8,
}

struct DetectedText {
    frame: Frame,
    score: f32,
    order: u32,
}

fn analyze(output: KoharuLayoutDetections, area: PixelArea) -> Analysis {
    let mut panels = output
        .detections
        .iter()
        .filter(|detection| detection.label == "panel")
        .map(|detection| DetectedRegion {
            frame: offset_frame(detection.bbox, area),
            score: detection.score,
            order: 0,
        })
        .collect::<Vec<_>>();
    panels.sort_by(|left, right| manga_position(frame_box(left.frame), frame_box(right.frame)));
    for (order, panel) in panels.iter_mut().enumerate() {
        panel.order = order as u32;
    }

    let mut bubble_detections = output
        .detections
        .iter()
        .filter(|detection| detection.label == "bubble")
        .collect::<Vec<_>>();
    bubble_detections.sort_by_key(|detection| std::cmp::Reverse(detection.area));
    let mut bubble_mask = GrayImage::new(output.image_width, output.image_height);
    let mut bubbles = bubble_detections
        .into_iter()
        .take(255)
        .enumerate()
        .map(|(index, detection)| {
            let mask_id = (index + 1) as u8;
            paint_instance(&mut bubble_mask, detection, mask_id);
            DetectedBubble {
                frame: offset_frame(detection.bbox, area),
                score: detection.score,
                order: 0,
                mask_id,
            }
        })
        .collect::<Vec<_>>();
    bubbles.sort_by(|left, right| manga_position(frame_box(left.frame), frame_box(right.frame)));
    for (order, bubble) in bubbles.iter_mut().enumerate() {
        bubble.order = order as u32;
    }

    let mut text_mask = GrayImage::new(output.image_width, output.image_height);
    let mut texts = Vec::new();
    for detection in output
        .detections
        .iter()
        .filter(|detection| detection.label == "text")
    {
        paint_instance(&mut text_mask, detection, u8::MAX);
        texts.push(DetectedText {
            frame: offset_frame(detection.bbox, area),
            score: detection.score,
            order: 0,
        });
    }
    let mut texts = merge_duplicate_texts(texts);
    texts.sort_by(|left, right| manga_position(frame_box(left.frame), frame_box(right.frame)));
    for (order, text) in texts.iter_mut().enumerate() {
        text.order = order as u32;
    }
    postprocess_text_mask(&mut text_mask);

    Analysis {
        panels,
        bubbles,
        texts,
        text_mask,
        bubble_mask,
    }
}

// Dilate by 6 px at a 1024 px long side, then fill background components that
// do not touch the image border.
fn postprocess_text_mask(mask: &mut GrayImage) {
    if mask.width() == 0 || mask.height() == 0 {
        return;
    }
    let radius = text_mask_dilation_radius(mask.width(), mask.height());
    *mask = dilate(mask, Norm::L2, radius);
    fill_holes(mask);
}

fn text_mask_dilation_radius(width: u32, height: u32) -> u8 {
    (f64::from(TEXT_MASK_DILATION_RADIUS) * f64::from(width.max(height))
        / f64::from(TEXT_MASK_REFERENCE_LONG_SIDE))
    .round_ties_even()
    .clamp(1.0, f64::from(u8::MAX)) as u8
}

fn fill_holes(mask: &mut GrayImage) {
    let components = connected_components(mask, Connectivity::Eight, Luma([u8::MAX]));
    let mut exterior = HashSet::new();
    for x in 0..mask.width() {
        exterior.insert(components.get_pixel(x, 0)[0]);
        exterior.insert(components.get_pixel(x, mask.height() - 1)[0]);
    }
    for y in 0..mask.height() {
        exterior.insert(components.get_pixel(0, y)[0]);
        exterior.insert(components.get_pixel(mask.width() - 1, y)[0]);
    }
    for (pixel, component) in mask.pixels_mut().zip(components.pixels()) {
        if pixel[0] == 0 && !exterior.contains(&component[0]) {
            pixel[0] = u8::MAX;
        }
    }
}

fn remap_bubble_mask_ids(
    analysis: &mut Analysis,
    page: &koharu_scene::Page,
    context: &Context,
) -> Result<()> {
    let mut used = page
        .elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementKind::Region(region)
                if region.kind == RegionKind::Bubble
                    && !(region
                        .predictions
                        .iter()
                        .any(|prediction| prediction.model == MODEL_ID)
                        && context.includes_element(page.id, element.id, element.frame)) =>
            {
                region.mask_id
            }
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut remap = [0_u8; 256];
    for bubble in &mut analysis.bubbles {
        let old = bubble.mask_id;
        let mask_id = (1..=u8::MAX)
            .find(|mask_id| used.insert(*mask_id))
            .ok_or_else(|| anyhow!("bubble mask has no free instance label"))?;
        bubble.mask_id = mask_id;
        remap[usize::from(old)] = mask_id;
    }
    for pixel in analysis.bubble_mask.as_mut() {
        *pixel = remap[usize::from(*pixel)];
    }
    Ok(())
}

fn inserted_region_ids(
    commands: &koharu_scene::Commands,
    page: PageId,
    kind: RegionKind,
) -> Vec<ElementId> {
    commands
        .as_slice()
        .iter()
        .filter_map(|command| match command {
            Command::InsertElement {
                page: inserted_page,
                element,
                ..
            } if *inserted_page == page => match &element.kind {
                ElementKind::Region(region) if region.kind == kind => Some(element.id),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn paint_instance(mask: &mut GrayImage, detection: &KoharuLayoutDetection, value: u8) {
    for (pixel, &source) in mask.as_mut().iter_mut().zip(&detection.mask.pixels) {
        if source != 0 {
            *pixel = value;
        }
    }
}

fn patch_mask(
    context: &Context,
    page: PageId,
    asset: PageAsset,
    area: PixelArea,
    local: GrayImage,
) -> Result<GrayImage> {
    let captured = context.page(page).expect("captured page");
    if area.x == 0
        && area.y == 0
        && area.width == captured.size.width
        && area.height == captured.size.height
    {
        return Ok(local);
    }
    let mut full = context
        .asset(page, asset)?
        .map(|image| image.to_luma8())
        .unwrap_or_else(|| GrayImage::new(captured.size.width, captured.size.height));
    image::imageops::replace(&mut full, &local, i64::from(area.x), i64::from(area.y));
    Ok(full)
}

fn best_container(frame: Frame, containers: impl Iterator<Item = Frame>) -> Option<usize> {
    let target = frame_box(frame);
    containers
        .enumerate()
        .filter_map(|(index, container)| {
            let intersection = intersection_area(target, frame_box(container));
            (intersection > 0.0).then_some((index, intersection / box_area(target).max(1.0)))
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .and_then(|(index, overlap)| (overlap >= 0.2).then_some(index))
}

fn offset_frame(bbox: [f32; 4], area: PixelArea) -> Frame {
    Frame::new(
        bbox[0] + area.x as f32,
        bbox[1] + area.y as f32,
        (bbox[2] - bbox[0]).max(1.0),
        (bbox[3] - bbox[1]).max(1.0),
    )
}

fn frame_box(frame: Frame) -> [f32; 4] {
    [
        frame.x,
        frame.y,
        frame.x + frame.width,
        frame.y + frame.height,
    ]
}

fn manga_position(left: [f32; 4], right: [f32; 4]) -> std::cmp::Ordering {
    left[1]
        .total_cmp(&right[1])
        .then_with(|| right[0].total_cmp(&left[0]))
}

fn intersection_area(left: [f32; 4], right: [f32; 4]) -> f32 {
    (left[2].min(right[2]) - left[0].max(right[0])).max(0.0)
        * (left[3].min(right[3]) - left[1].max(right[1])).max(0.0)
}

fn box_area(value: [f32; 4]) -> f32 {
    (value[2] - value[0]).max(0.0) * (value[3] - value[1]).max(0.0)
}

fn merge_duplicate_texts(mut texts: Vec<DetectedText>) -> Vec<DetectedText> {
    texts.sort_by(|left, right| right.score.total_cmp(&left.score));
    let mut merged: Vec<DetectedText> = Vec::with_capacity(texts.len());

    for mut text in texts {
        let mut index = 0;
        while index < merged.len() {
            if duplicate_text_frames(text.frame, merged[index].frame) {
                let duplicate = merged.swap_remove(index);
                text.frame = union_frame(text.frame, duplicate.frame);
                text.score = text.score.max(duplicate.score);
                index = 0;
            } else {
                index += 1;
            }
        }
        merged.push(text);
    }

    merged
}

fn duplicate_text_frames(left: Frame, right: Frame) -> bool {
    let left = frame_box(left);
    let right = frame_box(right);
    let intersection = intersection_area(left, right);
    if intersection == 0.0 {
        return false;
    }

    let left_area = box_area(left);
    let right_area = box_area(right);
    let iou = intersection / (left_area + right_area - intersection).max(1.0);
    let smaller_coverage = intersection / left_area.min(right_area).max(1.0);
    iou >= 0.45 || smaller_coverage >= 0.90
}

fn union_frame(left: Frame, right: Frame) -> Frame {
    let left = frame_box(left);
    let right = frame_box(right);
    let x1 = left[0].min(right[0]);
    let y1 = left[1].min(right[1]);
    let x2 = left[2].max(right[2]);
    let y2 = left[3].max(right[3]);
    Frame::new(x1, y1, x2 - x1, y2 - y1)
}

fn encode(mask: GrayImage) -> Result<Arc<[u8]>> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(mask).write_to(&mut bytes, ImageFormat::Png)?;
    Ok(Arc::from(bytes.into_inner()))
}

#[cfg(test)]
mod tests {
    use image::Luma;
    use koharu_ml::koharu_layout_rfdetr_seg_2xl::KoharuLayoutMask;

    use super::*;

    #[test]
    fn maps_supported_layout_classes_to_scene_artifacts() {
        let detection = |label: &str, bbox: [f32; 4], pixel: usize| {
            let mut pixels = vec![0; 12];
            pixels[pixel] = u8::MAX;
            KoharuLayoutDetection {
                label_id: 0,
                label: label.into(),
                score: 0.75,
                bbox,
                area: 1,
                mask: KoharuLayoutMask {
                    width: 4,
                    height: 3,
                    pixels,
                },
            }
        };
        let analysis = analyze(
            KoharuLayoutDetections {
                image_width: 4,
                image_height: 3,
                detections: vec![
                    detection("panel", [0.0, 0.0, 4.0, 3.0], 0),
                    detection("bubble", [1.0, 0.0, 3.0, 2.0], 1),
                    detection("text", [1.0, 1.0, 2.0, 2.0], 5),
                ],
            },
            PixelArea {
                x: 10,
                y: 20,
                width: 4,
                height: 3,
            },
        );

        assert_eq!(analysis.panels.len(), 1);
        assert_eq!(analysis.panels[0].frame, Frame::new(10.0, 20.0, 4.0, 3.0));
        assert_eq!(analysis.bubbles.len(), 1);
        assert_eq!(analysis.bubbles[0].mask_id, 1);
        assert_eq!(analysis.texts.len(), 1);
        assert_eq!(analysis.bubble_mask.get_pixel(1, 0), &Luma([1]));
        assert_eq!(analysis.text_mask.get_pixel(1, 1), &Luma([u8::MAX]));
        assert_eq!(analysis.text_mask.get_pixel(2, 1), &Luma([u8::MAX]));
    }

    #[test]
    fn text_mask_dilation_scales_from_the_reference_long_side() {
        assert_eq!(text_mask_dilation_radius(128, 64), 1);
        assert_eq!(text_mask_dilation_radius(1024, 512), 6);
        assert_eq!(text_mask_dilation_radius(4096, 2048), 24);
    }

    #[test]
    fn text_mask_postprocessing_dilates_and_fills_enclosed_background() {
        let mut mask = GrayImage::new(11, 11);
        for offset in 2..=8 {
            mask.put_pixel(offset, 2, Luma([u8::MAX]));
            mask.put_pixel(offset, 8, Luma([u8::MAX]));
            mask.put_pixel(2, offset, Luma([u8::MAX]));
            mask.put_pixel(8, offset, Luma([u8::MAX]));
        }

        postprocess_text_mask(&mut mask);

        assert_eq!(mask.get_pixel(1, 2), &Luma([u8::MAX]));
        assert_eq!(mask.get_pixel(5, 5), &Luma([u8::MAX]));
        assert_eq!(mask.get_pixel(0, 0), &Luma([0]));
    }

    #[test]
    fn merges_nested_text_detections_without_losing_coverage() {
        let texts = vec![
            DetectedText {
                frame: Frame::new(112.0, 231.0, 30.1, 140.2),
                score: 0.7656,
                order: 0,
            },
            DetectedText {
                frame: Frame::new(112.0, 225.7, 57.2, 147.7),
                score: 0.3867,
                order: 0,
            },
        ];

        let merged = merge_duplicate_texts(texts);

        assert_eq!(merged.len(), 1);
        let frame = merged[0].frame;
        assert!((frame.x - 112.0).abs() < 0.001);
        assert!((frame.y - 225.7).abs() < 0.001);
        assert!((frame.width - 57.2).abs() < 0.001);
        assert!((frame.height - 147.7).abs() < 0.001);
        assert_eq!(merged[0].score, 0.7656);
    }

    #[test]
    fn keeps_non_overlapping_text_detections_separate() {
        let texts = vec![
            DetectedText {
                frame: Frame::new(10.0, 20.0, 30.0, 40.0),
                score: 0.8,
                order: 0,
            },
            DetectedText {
                frame: Frame::new(45.0, 20.0, 30.0, 40.0),
                score: 0.7,
                order: 0,
            },
        ];

        assert_eq!(merge_duplicate_texts(texts).len(), 2);
    }
}
