use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow, bail, ensure};
use async_trait::async_trait;
use image::DynamicImage;
use koharu_ml::comic_onomatopoeia::{
    ComicOnomatopoeiaDetector, ComicOnomatopoeiaRecognizer, Detection, Recognition,
};
use koharu_scene::{
    Command, ElementChange, ElementKind, Frame, ModelPrediction, PageId, RegionKind, SourceText,
    TextAnalysis, TextBlock, TextDirection, TextRole,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{Artifact, Context, Processor};

const DETECTOR_ID: &str = "COO/MTSv3";
const RECOGNIZER_ID: &str = "COO/TRBA+2D";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct ComicOnomatopoeiaConfig {
    pub detection_threshold: f32,
    pub recognition_threshold: f32,
    pub dedup_iou: f32,
}

impl Default for ComicOnomatopoeiaConfig {
    fn default() -> Self {
        Self {
            detection_threshold: 0.5,
            recognition_threshold: 0.5,
            dedup_iou: 0.3,
        }
    }
}

struct Models {
    detector: ComicOnomatopoeiaDetector,
    recognizer: ComicOnomatopoeiaRecognizer,
}

pub(super) struct ComicOnomatopoeiaProcessor {
    models: Arc<Mutex<Models>>,
    config: ComicOnomatopoeiaConfig,
}

impl ComicOnomatopoeiaProcessor {
    pub(super) async fn load(
        device: koharu_ml::Device,
        config: &ComicOnomatopoeiaConfig,
    ) -> Result<Self> {
        ensure!(
            [
                config.detection_threshold,
                config.recognition_threshold,
                config.dedup_iou
            ]
            .into_iter()
            .all(|threshold| (0.0..=1.0).contains(&threshold)),
            "COO thresholds must be between zero and one"
        );
        Ok(Self {
            models: Arc::new(Mutex::new(Models {
                detector: ComicOnomatopoeiaDetector::load(device.clone()).await?,
                recognizer: ComicOnomatopoeiaRecognizer::load(device).await?,
            })),
            config: config.clone(),
        })
    }
}

#[async_trait]
impl Processor for ComicOnomatopoeiaProcessor {
    fn name(&self) -> &'static str {
        "ComicOnomatopoeia"
    }

    fn inputs(&self) -> &'static [Artifact] {
        &[Artifact::SourceImage, Artifact::TextRegion]
    }

    fn outputs(&self) -> &'static [Artifact] {
        &[Artifact::CooRegion, Artifact::CooText]
    }

    async fn run(&mut self, context: &Context) -> Result<koharu_scene::Commands> {
        let inputs = context
            .pages()
            .iter()
            .map(|page| page_input(context, page.id))
            .collect::<Result<Vec<_>>>()?;
        let config = self.config.clone();
        let models = self.models.clone();
        let outputs = tokio::task::spawn_blocking(move || {
            let models = models
                .lock()
                .map_err(|_| anyhow!("comic onomatopoeia model lock is poisoned"))?;
            inputs
                .into_iter()
                .map(|input| {
                    let candidates = models
                        .detector
                        .inference(&input.image)?
                        .into_iter()
                        .map(|detection| {
                            let crop = crop_detection(&input.image, &detection)?;
                            let recognition = models.recognizer.inference(&crop)?;
                            Ok(Candidate {
                                detection,
                                recognition,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Ok((input, candidates))
                })
                .collect::<Result<Vec<_>>>()
        })
        .await??;

        let mut commands = context.commands();
        for (input, candidates) in outputs {
            let page = context.page(input.page).expect("captured page");
            for element in &page.elements {
                if element.text().is_some_and(|text| {
                    text.predictions
                        .iter()
                        .any(|prediction| prediction.model == DETECTOR_ID)
                }) && context.includes_element(input.page, element.id, element.frame)
                {
                    commands.push(Command::DeleteElement {
                        page: input.page,
                        element: element.id,
                    });
                }
            }

            let existing = page
                .texts()
                .filter(|(element, text)| {
                    text.role == TextRole::Onomatopoeia
                        && !text
                            .predictions
                            .iter()
                            .any(|prediction| prediction.model == DETECTOR_ID)
                        && context.includes_element(page.id, element.id, element.frame)
                })
                .map(|(element, text)| (element.id, element.frame, TextAnalysis::from(text)))
                .collect::<Vec<_>>();
            let panels = page
                .elements
                .iter()
                .filter_map(|element| match &element.kind {
                    ElementKind::Region(region) if region.kind == RegionKind::Panel => {
                        Some((element.id, element.frame))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();

            let mut accepted = candidates
                .into_iter()
                .filter(|candidate| {
                    candidate.detection.score >= config.detection_threshold
                        && candidate.recognition.confidence >= config.recognition_threshold
                })
                .collect::<Vec<_>>();
            accepted.sort_by(|left, right| {
                manga_position(left.detection.bounding_box, right.detection.bounding_box)
            });
            let mut matched_existing = HashSet::new();
            for (order, candidate) in accepted.into_iter().enumerate() {
                let frame = detection_frame(&candidate.detection, input.area);
                let source = SourceText {
                    text: candidate.recognition.text.clone(),
                    language: Some("ja".to_owned()),
                    direction: if frame.height >= frame.width * 1.15 {
                        TextDirection::Vertical
                    } else {
                        TextDirection::Horizontal
                    },
                    confidence: Some(candidate.recognition.confidence.clamp(0.0, 1.0)),
                    lines: Vec::new(),
                };
                if let Some((element, _, mut metadata)) = existing
                    .iter()
                    .filter_map(|(element, existing_frame, metadata)| {
                        let overlap = iou(frame_box(frame), frame_box(*existing_frame));
                        (overlap >= config.dedup_iou && !matched_existing.contains(element))
                            .then_some((*element, overlap, metadata.clone()))
                    })
                    .max_by(|left, right| left.1.total_cmp(&right.1))
                {
                    matched_existing.insert(element);
                    metadata.polygon = candidate
                        .detection
                        .polygon
                        .iter()
                        .map(|[x, y]| [x + input.area.x as f32, y + input.area.y as f32])
                        .collect();
                    metadata.predictions.extend([
                        ModelPrediction::new(DETECTOR_ID, candidate.detection.score),
                        ModelPrediction::new(
                            RECOGNIZER_ID,
                            candidate.recognition.confidence.clamp(0.0, 1.0),
                        ),
                    ]);
                    commands.push(Command::EditElement {
                        page: input.page,
                        element,
                        edit: ElementChange::Analysis(metadata),
                    });
                    commands.push(Command::EditElement {
                        page: input.page,
                        element,
                        edit: ElementChange::Source(Some(source)),
                    });
                    commands.push(Command::EditElement {
                        page: input.page,
                        element,
                        edit: ElementChange::Translation(None),
                    });
                    continue;
                }

                let panel = best_container(frame, panels.iter().map(|(_, frame)| *frame))
                    .and_then(|index| panels.get(index).map(|(id, _)| *id));
                let polygon = candidate
                    .detection
                    .polygon
                    .into_iter()
                    .map(|[x, y]| [x + input.area.x as f32, y + input.area.y as f32])
                    .collect();
                commands.add_text_block(
                    input.page,
                    frame,
                    TextBlock {
                        source: Some(source),
                        role: TextRole::Onomatopoeia,
                        panel,
                        reading_order: Some(order as u32),
                        polygon,
                        predictions: vec![
                            ModelPrediction::new(DETECTOR_ID, candidate.detection.score),
                            ModelPrediction::new(
                                RECOGNIZER_ID,
                                candidate.recognition.confidence.clamp(0.0, 1.0),
                            ),
                        ],
                        ..TextBlock::default()
                    },
                );
            }
        }
        Ok(commands)
    }
}

struct Candidate {
    detection: Detection,
    recognition: Recognition,
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
    let image = if area.width == source.width() && area.height == source.height() {
        source
    } else {
        Arc::new(source.crop_imm(area.x, area.y, area.width, area.height))
    };
    Ok(PageInput { page, image, area })
}

// COO's TRBA evaluation uses an axis-aligned crop around the expanded MTSv3 polygon.
// https://github.com/ku21fan/COO-Comic-Onomatopoeia/blob/d8028f015b8ce99a4dd798427342f97087529357/COO-data/data_for_TRBA.ipynb
fn crop_detection(image: &DynamicImage, detection: &Detection) -> Result<DynamicImage> {
    let bounds = detection.polygon.iter().fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |mut bounds, point| {
            bounds[0] = bounds[0].min(point[0]);
            bounds[1] = bounds[1].min(point[1]);
            bounds[2] = bounds[2].max(point[0]);
            bounds[3] = bounds[3].max(point[1]);
            bounds
        },
    );
    let left = bounds[0].floor().clamp(0.0, image.width() as f32) as u32;
    let top = bounds[1].floor().clamp(0.0, image.height() as f32) as u32;
    let right = bounds[2].ceil().clamp(0.0, image.width() as f32) as u32;
    let bottom = bounds[3].ceil().clamp(0.0, image.height() as f32) as u32;
    ensure!(
        right > left && bottom > top,
        "MTSv3 produced an empty recognition crop"
    );
    Ok(image.crop_imm(left, top, right - left, bottom - top))
}

fn detection_frame(detection: &Detection, area: PixelArea) -> Frame {
    let [x1, y1, x2, y2] = detection.bounding_box;
    Frame::new(
        x1 + area.x as f32,
        y1 + area.y as f32,
        (x2 - x1).max(1.0),
        (y2 - y1).max(1.0),
    )
}

fn best_container(frame: Frame, containers: impl Iterator<Item = Frame>) -> Option<usize> {
    let target = frame_box(frame);
    containers
        .enumerate()
        .filter_map(|(index, container)| {
            let overlap =
                intersection_area(target, frame_box(container)) / box_area(target).max(1.0);
            (overlap >= 0.2).then_some((index, overlap))
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(index, _)| index)
}

fn manga_position(left: [f32; 4], right: [f32; 4]) -> std::cmp::Ordering {
    left[1]
        .total_cmp(&right[1])
        .then_with(|| right[0].total_cmp(&left[0]))
}

fn frame_box(frame: Frame) -> [f32; 4] {
    [
        frame.x,
        frame.y,
        frame.x + frame.width,
        frame.y + frame.height,
    ]
}

fn iou(left: [f32; 4], right: [f32; 4]) -> f32 {
    let intersection = intersection_area(left, right);
    intersection / (box_area(left) + box_area(right) - intersection).max(1.0)
}

fn intersection_area(left: [f32; 4], right: [f32; 4]) -> f32 {
    (left[2].min(right[2]) - left[0].max(right[0])).max(0.0)
        * (left[3].min(right[3]) - left[1].max(right[1])).max(0.0)
}

fn box_area(value: [f32; 4]) -> f32 {
    (value[2] - value[0]).max(0.0) * (value[3] - value[1]).max(0.0)
}
