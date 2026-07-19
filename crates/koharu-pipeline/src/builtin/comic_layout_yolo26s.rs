use std::{
    collections::HashSet,
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow, bail, ensure};
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use koharu_ml::comic_layout_yolo26s::{
    ComicLayoutYolo26sInstance, ComicLayoutYolo26sInstances, ComicLayoutYolo26sSegmenter,
};
use koharu_scene::{
    Command, ElementId, ElementKind, Frame, ModelPrediction, PageAsset, PageId, Region, RegionKind,
    SourceText, TextAnalysis, TextBlock, TextDirection, TextRole,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Artifact, Context, Processor};

const MODEL_ID: &str = "mayocream/comic-layout-yolo26s";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct ComicLayoutYolo26sConfig {
    pub confidence: f32,
    /// Add the model's generic text instances as editable free text. Keep this
    /// off when a stronger text detector is configured alongside the model.
    pub text_regions: bool,
    pub text_masks: bool,
}

impl Default for ComicLayoutYolo26sConfig {
    fn default() -> Self {
        Self {
            confidence: 0.25,
            text_regions: false,
            text_masks: true,
        }
    }
}

pub(super) struct ComicLayoutYolo26sProcessor {
    model: Arc<Mutex<ComicLayoutYolo26sSegmenter>>,
    config: ComicLayoutYolo26sConfig,
}

impl ComicLayoutYolo26sProcessor {
    pub(super) async fn load(
        device: koharu_ml::Device,
        config: &ComicLayoutYolo26sConfig,
    ) -> Result<Self> {
        ensure!(
            (0.0..=1.0).contains(&config.confidence),
            "YOLO26 confidence must be between zero and one"
        );
        Ok(Self {
            model: Arc::new(Mutex::new(ComicLayoutYolo26sSegmenter::load(device).await?)),
            config: config.clone(),
        })
    }
}

#[async_trait]
impl Processor for ComicLayoutYolo26sProcessor {
    fn name(&self) -> &'static str {
        "ComicLayoutYolo26s"
    }

    fn inputs(&self) -> &'static [Artifact] {
        &[Artifact::SourceImage]
    }

    fn outputs(&self) -> &'static [Artifact] {
        &[
            Artifact::PanelRegion,
            Artifact::BubbleRegion,
            Artifact::TextRegion,
            Artifact::LayoutTextMask,
            Artifact::BubbleMask,
        ]
    }

    async fn run(&mut self, context: &Context) -> Result<koharu_scene::Commands> {
        let inputs = context
            .pages()
            .iter()
            .map(|page| page_input(context, page.id))
            .collect::<Result<Vec<_>>>()?;
        let confidence = self.config.confidence;
        let model = self.model.clone();
        let outputs = tokio::task::spawn_blocking(move || {
            let model = model
                .lock()
                .map_err(|_| anyhow!("comic-layout-yolo26s model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let output = model.inference_with_threshold(&input.image, confidence)?;
                    Ok((input, output))
                })
                .collect::<Result<Vec<_>>>()
        })
        .await??;

        let mut commands = context.commands();
        for (input, output) in outputs {
            let page = context.page(input.page).expect("captured page");
            for element in &page.elements {
                let predicted = match &element.kind {
                    ElementKind::Text(text) => &text.predictions,
                    ElementKind::Region(region) => &region.predictions,
                    ElementKind::Image(_) => continue,
                };
                if predicted
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

            let mut analysis = analyze(output, input.area, &self.config);
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
                let bubble = (text.role != TextRole::Onomatopoeia)
                    .then(|| {
                        best_container(
                            element.frame,
                            analysis.bubbles.iter().map(|region| region.frame),
                        )
                    })
                    .flatten()
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

            for text in analysis.texts {
                let panel = best_container(
                    text.frame,
                    analysis.panels.iter().map(|region| region.frame),
                )
                .and_then(|index| panel_ids.get(index).copied());
                let bubble = (text.role == TextRole::Dialogue)
                    .then(|| {
                        best_container(
                            text.frame,
                            analysis.bubbles.iter().map(|region| region.frame),
                        )
                    })
                    .flatten()
                    .and_then(|index| bubble_ids.get(index).copied());
                let mut block = TextBlock {
                    role: text.role,
                    panel,
                    bubble,
                    reading_order: Some(text.order),
                    predictions: vec![ModelPrediction::new(MODEL_ID, text.score)],
                    ..TextBlock::default()
                };
                block.source = Some(SourceText {
                    text: String::new(),
                    language: None,
                    direction: if text.frame.height >= text.frame.width * 1.15 {
                        TextDirection::Vertical
                    } else {
                        TextDirection::Horizontal
                    },
                    confidence: None,
                    lines: Vec::new(),
                });
                commands.add_text_block(input.page, text.frame, block);
            }

            let layout_mask = patch_mask(
                context,
                input.page,
                PageAsset::LayoutTextMask,
                input.area,
                analysis.layout_text_mask,
            )?;
            let bubble_mask = patch_mask(
                context,
                input.page,
                PageAsset::BubbleMask,
                input.area,
                analysis.bubble_mask,
            )?;
            commands.set_asset(
                input.page,
                PageAsset::LayoutTextMask,
                Some(encode(layout_mask)?),
            )?;
            commands.set_asset(
                input.page,
                PageAsset::BubbleMask,
                Some(encode(bubble_mask)?),
            )?;
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
    layout_text_mask: GrayImage,
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
    role: TextRole,
    score: f32,
    order: u32,
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

fn analyze(
    output: ComicLayoutYolo26sInstances,
    area: PixelArea,
    config: &ComicLayoutYolo26sConfig,
) -> Analysis {
    let mut panels = output
        .instances
        .iter()
        .filter(|instance| instance.label == "frame")
        .collect::<Vec<_>>();
    panels.sort_by(|left, right| manga_position(left.bbox, right.bbox));
    let panels = panels
        .into_iter()
        .enumerate()
        .map(|(order, instance)| DetectedRegion {
            frame: offset_frame(instance.bbox, area),
            score: instance.score,
            order: order as u32,
        })
        .collect::<Vec<_>>();

    let mut bubble_instances = output
        .instances
        .iter()
        .filter(|instance| instance.label == "balloon")
        .collect::<Vec<_>>();
    bubble_instances.sort_by_key(|instance| std::cmp::Reverse(instance.area));
    let mut bubble_mask = GrayImage::new(output.image_width, output.image_height);
    let mut bubbles = bubble_instances
        .into_iter()
        .take(255)
        .enumerate()
        .map(|(index, instance)| {
            let mask_id = (index + 1) as u8;
            paint_instance(&mut bubble_mask, instance, mask_id);
            DetectedBubble {
                frame: offset_frame(instance.bbox, area),
                score: instance.score,
                order: 0,
                mask_id,
            }
        })
        .collect::<Vec<_>>();
    bubbles.sort_by(|left, right| manga_position(frame_box(left.frame), frame_box(right.frame)));
    for (order, bubble) in bubbles.iter_mut().enumerate() {
        bubble.order = order as u32;
    }

    let mut layout_text_mask = GrayImage::new(output.image_width, output.image_height);
    let mut texts = Vec::new();
    for instance in &output.instances {
        let role = match instance.label.as_str() {
            "text" if config.text_regions => Some(TextRole::FreeText),
            _ => None,
        };
        if config.text_masks && instance.label == "text" {
            paint_instance(&mut layout_text_mask, instance, u8::MAX);
        }
        if let Some(role) = role {
            texts.push(DetectedText {
                frame: offset_frame(instance.bbox, area),
                role,
                score: instance.score,
                order: 0,
            });
        }
    }
    texts.sort_by(|left, right| manga_position(frame_box(left.frame), frame_box(right.frame)));
    for (order, text) in texts.iter_mut().enumerate() {
        text.order = order as u32;
    }

    Analysis {
        panels,
        bubbles,
        texts,
        layout_text_mask,
        bubble_mask,
    }
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

fn best_container(fr: Frame, containers: impl Iterator<Item = Frame>) -> Option<usize> {
    let target = frame_box(fr);
    containers
        .enumerate()
        .filter_map(|(index, container)| {
            let intersection = intersection_area(target, frame_box(container));
            (intersection > 0.0).then_some((index, intersection / box_area(target).max(1.0)))
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .and_then(|(index, overlap)| (overlap >= 0.2).then_some(index))
}

fn paint_instance(mask: &mut GrayImage, instance: &ComicLayoutYolo26sInstance, value: u8) {
    let max_x = instance
        .mask
        .width
        .min(mask.width().saturating_sub(instance.mask.x));
    let max_y = instance
        .mask
        .height
        .min(mask.height().saturating_sub(instance.mask.y));
    for y in 0..max_y {
        for x in 0..max_x {
            if instance.mask.pixels[y as usize * instance.mask.width as usize + x as usize] != 0 {
                mask.put_pixel(instance.mask.x + x, instance.mask.y + y, Luma([value]));
            }
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

fn encode(mask: GrayImage) -> Result<Arc<[u8]>> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(mask).write_to(&mut bytes, ImageFormat::Png)?;
    Ok(Arc::from(bytes.into_inner()))
}
