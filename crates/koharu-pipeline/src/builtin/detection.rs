use std::{
    cmp::Ordering,
    collections::BTreeMap,
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result, anyhow, bail};
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use imageproc::{distance_transform::Norm, morphology::dilate};
use koharu_ml::koharu_layout_rfdetr_seg_2xl::{
    KoharuLayoutDetection, KoharuLayoutDetections, KoharuLayoutRFDetrSeg2XL, KoharuLayoutThresholds,
};
use koharu_scene::{
    AssetInput, AssetMetadata, AssetRole, At, DetectionAnalysis, DetectionLabel, EntityOrigin,
    Generation, Geometry, Origin, ReadingOrder, Region, RegionKind, RelationKind, RemovePolicy,
    TextRole,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{finish, generation, producer};
use crate::{DetectionModel, NodeInput, NodeOutput, Stage};

const MODEL_ID: &str = "mayocream/koharu-layout-rfdetr-seg-2xl-1152";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct KoharuLayoutRFDetrSeg2XLConfig {
    pub text_threshold: Option<f32>,
    pub bubble_threshold: Option<f32>,
    pub panel_threshold: Option<f32>,
}

pub(super) struct Model {
    model: Arc<Mutex<KoharuLayoutRFDetrSeg2XL>>,
    thresholds: KoharuLayoutThresholds,
}

impl Model {
    pub(super) async fn load(device: koharu_ml::Device, config: &DetectionModel) -> Result<Self> {
        let DetectionModel::KoharuLayoutRFDetrSeg2XL(config) = config;
        let model = KoharuLayoutRFDetrSeg2XL::load(device).await?;
        let mut thresholds = model.recommended_thresholds();
        thresholds.text = config.text_threshold.unwrap_or(thresholds.text);
        thresholds.bubble = config.bubble_threshold.unwrap_or(thresholds.bubble);
        thresholds.panel = config.panel_threshold.unwrap_or(thresholds.panel);
        Ok(Self {
            model: Arc::new(Mutex::new(model)),
            thresholds,
        })
    }

    pub(super) async fn run(&self, input: NodeInput) -> Result<NodeOutput> {
        let mut pages = Vec::new();
        for page in input.scope.pages() {
            let image = input
                .cache
                .image(&input.scene, *page, "source")?
                .ok_or_else(|| anyhow!("page {page} has no source image"))?;
            pages.push((*page, image));
        }
        let model = self.model.clone();
        let thresholds = self.thresholds;
        let outputs = tokio::task::spawn_blocking(move || {
            let model = model
                .lock()
                .map_err(|_| anyhow!("layout model lock is poisoned"))?;
            pages
                .into_iter()
                .map(|(page, image)| {
                    Ok((page, model.inference_with_thresholds(&image, thresholds)?))
                })
                .collect::<Result<Vec<_>>>()
        })
        .await
        .context("layout detection task panicked")??;
        if input.cancellation.is_cancelled() {
            bail!("layout detection was cancelled");
        }

        let generation = generation(producer(Stage::Detection), MODEL_ID)?;
        let mut edit = input.scene.edit_as(generation.clone());
        reconcile(&input, &mut edit, &generation)?;
        for (page, output) in outputs {
            write_page(&input, &mut edit, page, output, &generation)?;
        }
        finish(edit)
    }
}

fn reconcile(
    input: &NodeInput,
    edit: &mut koharu_scene::SceneEdit,
    owner: &Generation,
) -> Result<()> {
    let mut remove = Vec::new();
    for page in input.scope.pages() {
        for entity in input.scene.descendants(*page)? {
            let id = entity.id();
            if !input.scope.contains_entity(&input.scene, id)? {
                continue;
            }
            let owned = entity
                .component::<EntityOrigin>("default")?
                .is_some_and(|origin| {
                    matches!(origin.origin, Origin::Generated(ref generation) if generation.producer == owner.producer)
                });
            if owned {
                remove.push(id);
            }
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
    input: &NodeInput,
    edit: &mut koharu_scene::SceneEdit,
    page: koharu_scene::EntityId,
    output: KoharuLayoutDetections,
    generation: &Generation,
) -> Result<()> {
    let mut detections = output.detections;
    detections.retain(|detection| {
        input.scope.region(page).is_none_or(|bounds| {
            let [left, top, right, bottom] = detection.bbox;
            left < (bounds.x + bounds.width) as f32
                && right > bounds.x as f32
                && top < (bounds.y + bounds.height) as f32
                && bottom > bounds.y as f32
        })
    });
    non_maximum_suppression(&mut detections, 0.5);
    sort_by_layout(&mut detections);

    let mut bubbles = Vec::<(koharu_scene::EntityId, [f32; 4])>::new();
    let mut texts = Vec::<(koharu_scene::EntityId, [f32; 4])>::new();
    for (order, detection) in detections.iter().enumerate() {
        let kind = region_kind(&detection.label)?;
        let entity = edit.add_entity(page, At::End)?;
        let [left, top, right, bottom] = detection.bbox;
        edit.set(
            entity,
            "default",
            &Geometry::rectangle(
                f64::from(left),
                f64::from(top),
                f64::from((right - left).max(1.0)),
                f64::from((bottom - top).max(1.0)),
            ),
        )?;
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
                    kind: kind.clone(),
                    confidence: detection.score,
                }],
            },
        )?;
        edit.set(
            entity,
            "default",
            &ReadingOrder {
                origin: Origin::Generated(generation.clone()),
                index: order as u32,
            },
        )?;
        match detection.label.as_str() {
            "bubble" => bubbles.push((entity, detection.bbox)),
            "text" => {
                edit.set(
                    entity,
                    "default",
                    &TextRole {
                        origin: Origin::Generated(generation.clone()),
                        role: "dev.koharu.text.free-text".to_owned(),
                    },
                )?;
                texts.push((entity, detection.bbox));
            }
            _ => {}
        }
    }

    let relation = RelationKind::new("dev.koharu.relation.text-region")?;
    for (text, bounds) in texts {
        if let Some((bubble, _)) = bubbles
            .iter()
            .filter(|(_, candidate)| containment(*candidate, bounds) >= 0.5)
            .min_by(|(_, left), (_, right)| area(*left).total_cmp(&area(*right)))
        {
            edit.add_relation(relation.clone(), text, *bubble)?;
            edit.set(
                text,
                "default",
                &TextRole {
                    origin: Origin::Generated(generation.clone()),
                    role: "dev.koharu.text.dialogue".to_owned(),
                },
            )?;
        }
    }

    for (role, label) in [("text-mask", "text"), ("bubble-mask", "bubble")] {
        let mut mask = mask_for(&detections, label, output.image_width, output.image_height);
        if label == "text" && mask.width() > 0 && mask.height() > 0 {
            let radius = ((mask.width().max(mask.height()) as f32 / 1024.0) * 6.0)
                .round()
                .clamp(1.0, 255.0) as u8;
            mask = dilate(&mask, Norm::L2, radius);
        }
        if let Some(bounds) = input.scope.region(page) {
            let previous = input
                .cache
                .image(&input.scene, page, role)?
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
        }
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageLuma8(mask).write_to(&mut bytes, ImageFormat::Png)?;
        edit.set_asset(
            page,
            &AssetRole::new(role)?,
            AssetInput::new(
                Arc::<[u8]>::from(bytes.into_inner()),
                "image/png",
                AssetMetadata {
                    width: Some(output.image_width),
                    height: Some(output.image_height),
                    attributes: BTreeMap::new(),
                },
            ),
        )?;
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

fn mask_for(
    detections: &[KoharuLayoutDetection],
    label: &str,
    width: u32,
    height: u32,
) -> GrayImage {
    let mut mask = GrayImage::new(width, height);
    for detection in detections.iter().filter(|value| value.label == label) {
        for (target, source) in mask.as_mut().iter_mut().zip(&detection.mask.pixels) {
            if *source != 0 {
                *target = u8::MAX;
            }
        }
    }
    mask
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
    let panels = spatial_order(
        detections,
        detections
            .iter()
            .enumerate()
            .filter_map(|(index, detection)| (detection.label == "panel").then_some(index))
            .collect(),
    );
    let bubbles = detections
        .iter()
        .enumerate()
        .filter_map(|(index, detection)| (detection.label == "bubble").then_some(index))
        .collect::<Vec<_>>();
    let texts = detections
        .iter()
        .enumerate()
        .filter_map(|(index, detection)| (detection.label == "text").then_some(index))
        .collect::<Vec<_>>();

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
    let append = |index: usize, order: &mut Vec<usize>, included: &mut [bool]| {
        if !included[index] {
            included[index] = true;
            order.push(index);
        }
    };

    for &panel in &panels {
        append(panel, &mut order, &mut included);
        let panel_bubbles = spatial_order(
            detections,
            bubbles
                .iter()
                .copied()
                .filter(|&bubble| panel_for_bubble[bubble] == Some(panel))
                .collect(),
        );
        for bubble in panel_bubbles {
            append(bubble, &mut order, &mut included);
            for text in spatial_order(
                detections,
                texts
                    .iter()
                    .copied()
                    .filter(|&text| bubble_for_text[text] == Some(bubble))
                    .collect(),
            ) {
                append(text, &mut order, &mut included);
            }
        }
        for text in spatial_order(
            detections,
            texts
                .iter()
                .copied()
                .filter(|&text| {
                    bubble_for_text[text].is_none() && panel_for_text[text] == Some(panel)
                })
                .collect(),
        ) {
            append(text, &mut order, &mut included);
        }
    }

    for bubble in spatial_order(
        detections,
        bubbles
            .iter()
            .copied()
            .filter(|&bubble| panel_for_bubble[bubble].is_none())
            .collect(),
    ) {
        append(bubble, &mut order, &mut included);
        for text in spatial_order(
            detections,
            texts
                .iter()
                .copied()
                .filter(|&text| bubble_for_text[text] == Some(bubble))
                .collect(),
        ) {
            append(text, &mut order, &mut included);
        }
    }

    for text in spatial_order(
        detections,
        texts
            .iter()
            .copied()
            .filter(|&text| bubble_for_text[text].is_none() && panel_for_text[text].is_none())
            .collect(),
    ) {
        append(text, &mut order, &mut included);
    }
    for index in spatial_order(
        detections,
        (0..detections.len())
            .filter(|&index| !included[index])
            .collect(),
    ) {
        append(index, &mut order, &mut included);
    }
    order
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
    use koharu_ml::koharu_layout_rfdetr_seg_2xl::{KoharuLayoutDetection, KoharuLayoutMask};

    use super::{layout_order, non_maximum_suppression};

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
}
