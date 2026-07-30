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
    detections.sort_by(detection_order);

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
            .filter(|(_, candidate)| contains(*candidate, bounds))
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

fn contains(container: [f32; 4], value: [f32; 4]) -> bool {
    container[0] <= value[0]
        && container[1] <= value[1]
        && container[2] >= value[2]
        && container[3] >= value[3]
}

fn area(bounds: [f32; 4]) -> f32 {
    (bounds[2] - bounds[0]).max(0.0) * (bounds[3] - bounds[1]).max(0.0)
}
