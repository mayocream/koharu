mod model;

use std::{collections::BTreeMap, time::Instant};

use anyhow::{Context, Result, bail};
use image::{DynamicImage, GenericImageView, imageops::FilterType};
use koharu_runtime::RuntimeManager;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::{Device, device, loading, types::TextRegion};

use self::model::{RTDetrV2ForObjectDetection, RTDetrV2Outputs};

const HF_REPO: &str = "ogkalu/comic-text-and-bubble-detector";
const DEFAULT_CONFIDENCE_THRESHOLD: f32 = 0.3;
const DETECTOR_NAME: &str = "comic-text-bubble-detector";

koharu_runtime::declare_hf_model_package!(
    id: "model:comic-text-bubble-detector:config",
    repo: "ogkalu/comic-text-and-bubble-detector",
    file: "config.json",
    bootstrap: false,
    order: 113,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:comic-text-bubble-detector:preprocessor-config",
    repo: "ogkalu/comic-text-and-bubble-detector",
    file: "preprocessor_config.json",
    bootstrap: false,
    order: 114,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:comic-text-bubble-detector:weights",
    repo: "ogkalu/comic-text-and-bubble-detector",
    file: "model.safetensors",
    bootstrap: false,
    order: 115,
);

#[derive(Debug)]
pub struct ComicTextBubbleDetector {
    model: RTDetrV2ForObjectDetection,
    config: RTDetrV2Config,
    preprocessor: RTDetrImageProcessorConfig,
    device: Device,
    slicer: ImageSlicer,
}

impl ComicTextBubbleDetector {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let device = device(cpu)?;
        let downloads = runtime.downloads();
        let config_path = downloads.huggingface_model(HF_REPO, "config.json").await?;
        let preprocessor_path = downloads
            .huggingface_model(HF_REPO, "preprocessor_config.json")
            .await?;
        let weights_path = downloads
            .huggingface_model(HF_REPO, "model.safetensors")
            .await?;

        let config = loading::read_json::<RTDetrV2Config>(&config_path)
            .with_context(|| format!("failed to parse {}", config_path.display()))?;
        config.validate()?;
        let preprocessor = loading::read_json::<RTDetrImageProcessorConfig>(&preprocessor_path)
            .with_context(|| format!("failed to parse {}", preprocessor_path.display()))?;
        let model = loading::load_mmaped_safetensors_path(&weights_path, &device, |vb| {
            RTDetrV2ForObjectDetection::load(vb, &config)
        })?;

        Ok(Self {
            model,
            config,
            preprocessor,
            device,
            slicer: ImageSlicer::default(),
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(&self, image: &DynamicImage) -> Result<ComicTextBubbleDetection> {
        self.inference_with_threshold(image, DEFAULT_CONFIDENCE_THRESHOLD)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_with_threshold(
        &self,
        image: &DynamicImage,
        threshold: f32,
    ) -> Result<ComicTextBubbleDetection> {
        let started = Instant::now();
        let detections = self.slicer.process_slices_for_detection(image, |slice| {
            self.detect_single_image(slice, threshold)
        })?;
        let detections = merge_slice_regions(
            filter_and_fix_regions(detections, image.dimensions()),
            image.height(),
        );
        let text_blocks = detections_to_text_blocks(image.dimensions(), &detections);

        tracing::info!(
            width = image.width(),
            height = image.height(),
            detections = detections.len(),
            text_blocks = text_blocks.len(),
            total_ms = started.elapsed().as_millis(),
            "comic text bubble detector timings"
        );

        Ok(ComicTextBubbleDetection {
            image_width: image.width(),
            image_height: image.height(),
            detections,
            text_blocks,
        })
    }

    #[instrument(level = "debug", skip_all)]
    fn detect_single_image(
        &self,
        image: &DynamicImage,
        threshold: f32,
    ) -> Result<Vec<ComicTextBubbleRegion>> {
        let pixel_values = preprocess_image(image, &self.preprocessor, &self.device)?;
        let outputs = self.model.forward(&pixel_values)?;
        post_process_object_detection(&self.config, &outputs, image.dimensions(), threshold)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComicTextBubbleDetection {
    pub image_width: u32,
    pub image_height: u32,
    pub detections: Vec<ComicTextBubbleRegion>,
    pub text_blocks: Vec<TextRegion>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComicTextBubbleRegion {
    pub label_id: usize,
    pub label: String,
    pub score: f32,
    pub bbox: [f32; 4],
}

impl ComicTextBubbleRegion {
    pub fn is_bubble(&self) -> bool {
        self.label_id == 0
    }

    pub fn is_text(&self) -> bool {
        matches!(self.label_id, 1 | 2)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RTDetrV2Config {
    #[serde(default = "default_activation_dropout")]
    pub activation_dropout: f64,
    #[serde(default = "default_activation_function")]
    pub activation_function: String,
    #[serde(default)]
    pub anchor_image_size: Option<Vec<usize>>,
    #[serde(default = "default_attention_dropout")]
    pub attention_dropout: f64,
    #[serde(default)]
    pub backbone_config: RTDetrResNetConfig,
    #[serde(default = "default_batch_norm_eps")]
    pub batch_norm_eps: f64,
    #[serde(default = "default_d_model")]
    pub d_model: usize,
    #[serde(default = "default_decoder_activation_function")]
    pub decoder_activation_function: String,
    #[serde(default = "default_decoder_attention_heads")]
    pub decoder_attention_heads: usize,
    #[serde(default = "default_decoder_ffn_dim")]
    pub decoder_ffn_dim: usize,
    #[serde(default = "default_decoder_in_channels")]
    pub decoder_in_channels: Vec<usize>,
    #[serde(default = "default_decoder_layers")]
    pub decoder_layers: usize,
    #[serde(default = "default_decoder_n_levels")]
    pub decoder_n_levels: usize,
    #[serde(default = "default_decoder_n_points")]
    pub decoder_n_points: usize,
    #[serde(default = "default_decoder_offset_scale")]
    pub decoder_offset_scale: f64,
    #[serde(default = "default_decoder_method")]
    pub decoder_method: String,
    #[serde(default = "default_dropout")]
    pub dropout: f64,
    #[serde(default = "default_encode_proj_layers")]
    pub encode_proj_layers: Vec<usize>,
    #[serde(default = "default_encoder_activation_function")]
    pub encoder_activation_function: String,
    #[serde(default = "default_encoder_attention_heads")]
    pub encoder_attention_heads: usize,
    #[serde(default = "default_encoder_ffn_dim")]
    pub encoder_ffn_dim: usize,
    #[serde(default = "default_encoder_hidden_dim")]
    pub encoder_hidden_dim: usize,
    #[serde(default = "default_encoder_in_channels")]
    pub encoder_in_channels: Vec<usize>,
    #[serde(default = "default_encoder_layers")]
    pub encoder_layers: usize,
    #[serde(default = "default_feature_strides", alias = "feature_strides")]
    pub feat_strides: Vec<usize>,
    #[serde(default = "default_freeze_backbone_batch_norms")]
    pub freeze_backbone_batch_norms: bool,
    #[serde(default = "default_hidden_expansion")]
    pub hidden_expansion: f64,
    #[serde(default)]
    pub id2label: BTreeMap<String, String>,
    #[serde(default = "default_layer_norm_eps")]
    pub layer_norm_eps: f64,
    #[serde(default = "default_learn_initial_query")]
    pub learn_initial_query: bool,
    #[serde(default = "default_normalize_before")]
    pub normalize_before: bool,
    #[serde(default = "default_num_feature_levels")]
    pub num_feature_levels: usize,
    #[serde(default = "default_num_labels")]
    pub num_labels: usize,
    #[serde(default = "default_num_queries")]
    pub num_queries: usize,
    #[serde(default = "default_positional_encoding_temperature")]
    pub positional_encoding_temperature: usize,
    #[serde(default = "default_true")]
    pub use_focal_loss: bool,
    #[serde(default = "default_true")]
    pub with_box_refine: bool,
}

impl RTDetrV2Config {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.backbone_config.layer_type != "bottleneck" {
            bail!(
                "unsupported RT-DETR backbone layer_type {:?}; only bottleneck is supported",
                self.backbone_config.layer_type
            );
        }
        if self.learn_initial_query {
            bail!("learn_initial_query=true is not supported");
        }
        if !self.with_box_refine {
            bail!("with_box_refine=false is not supported");
        }
        if self.decoder_method != "default" {
            bail!(
                "unsupported RT-DETR decoder method {:?}; only default is supported",
                self.decoder_method
            );
        }
        Ok(())
    }

    pub(crate) fn num_labels(&self) -> usize {
        self.num_labels.max(self.id2label.len()).max(1)
    }

    pub(crate) fn label(&self, label_id: usize) -> String {
        self.id2label
            .get(&label_id.to_string())
            .cloned()
            .unwrap_or_else(|| format!("label_{label_id}"))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RTDetrResNetConfig {
    #[serde(default = "default_num_channels")]
    pub num_channels: usize,
    #[serde(default = "default_embedding_size")]
    pub embedding_size: usize,
    #[serde(default = "default_hidden_sizes")]
    pub hidden_sizes: Vec<usize>,
    #[serde(default = "default_depths")]
    pub depths: Vec<usize>,
    #[serde(default = "default_layer_type")]
    pub layer_type: String,
    #[serde(default = "default_hidden_act")]
    pub hidden_act: String,
    #[serde(default)]
    pub downsample_in_first_stage: bool,
    #[serde(default)]
    pub downsample_in_bottleneck: bool,
    #[serde(default = "default_out_features")]
    pub out_features: Vec<String>,
}

impl Default for RTDetrResNetConfig {
    fn default() -> Self {
        Self {
            num_channels: default_num_channels(),
            embedding_size: default_embedding_size(),
            hidden_sizes: default_hidden_sizes(),
            depths: default_depths(),
            layer_type: default_layer_type(),
            hidden_act: default_hidden_act(),
            downsample_in_first_stage: false,
            downsample_in_bottleneck: false,
            out_features: default_out_features(),
        }
    }
}

impl RTDetrResNetConfig {
    pub(crate) fn channels(&self) -> Result<Vec<usize>> {
        let mut channels = Vec::with_capacity(self.out_features.len());
        for feature in &self.out_features {
            if feature == "stem" {
                channels.push(self.embedding_size);
                continue;
            }
            let Some(stage) = feature.strip_prefix("stage") else {
                bail!("unsupported RT-DETR backbone out_feature {:?}", feature);
            };
            let stage_index = stage.parse::<usize>().with_context(|| {
                format!("failed to parse RT-DETR backbone out_feature {:?}", feature)
            })?;
            let hidden_index = stage_index
                .checked_sub(1)
                .context("stage indices must start at 1")?;
            let channels_for_stage = self
                .hidden_sizes
                .get(hidden_index)
                .copied()
                .with_context(|| format!("missing hidden size for stage {}", stage_index))?;
            channels.push(channels_for_stage);
        }
        Ok(channels)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RTDetrImageProcessorConfig {
    #[serde(default = "default_true")]
    pub do_resize: bool,
    #[serde(default = "default_true")]
    pub do_rescale: bool,
    #[serde(default)]
    pub do_normalize: bool,
    #[serde(default = "default_image_mean")]
    pub image_mean: [f32; 3],
    #[serde(default = "default_image_std")]
    pub image_std: [f32; 3],
    #[serde(default = "default_rescale_factor")]
    pub rescale_factor: f32,
    #[serde(default = "default_processor_size")]
    pub size: ProcessorSize,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ProcessorSize {
    #[serde(default = "default_processor_height")]
    pub height: usize,
    #[serde(default = "default_processor_width")]
    pub width: usize,
}

fn preprocess_image(
    image: &DynamicImage,
    preprocessor: &RTDetrImageProcessorConfig,
    device: &Device,
) -> Result<candle_core::Tensor> {
    let target_h = preprocessor.size.height;
    let target_w = preprocessor.size.width;
    let resized = if preprocessor.do_resize {
        image.resize_exact(target_w as u32, target_h as u32, FilterType::Triangle)
    } else {
        image.clone()
    };
    let rgb = resized.to_rgb8();
    let tensor =
        candle_core::Tensor::from_vec(rgb.into_raw(), (1, target_h, target_w, 3), &Device::Cpu)?
            .to_device(device)?
            .permute((0, 3, 1, 2))?
            .to_dtype(candle_core::DType::F32)?;
    let tensor = if preprocessor.do_rescale {
        tensor.affine(preprocessor.rescale_factor as f64, 0.0)?
    } else {
        tensor
    };
    if preprocessor.do_normalize {
        let mean = candle_core::Tensor::from_slice(&preprocessor.image_mean, (1, 3, 1, 1), device)?;
        let std = candle_core::Tensor::from_slice(&preprocessor.image_std, (1, 3, 1, 1), device)?;
        Ok(tensor.broadcast_sub(&mean)?.broadcast_div(&std)?)
    } else {
        Ok(tensor)
    }
}

fn post_process_object_detection(
    config: &RTDetrV2Config,
    outputs: &RTDetrV2Outputs,
    target_size: (u32, u32),
    threshold: f32,
) -> Result<Vec<ComicTextBubbleRegion>> {
    let logits = outputs.logits.to_device(&Device::Cpu)?;
    let pred_boxes = outputs.pred_boxes.to_device(&Device::Cpu)?;
    let (batch_size, num_queries, num_classes) = logits.dims3()?;
    if batch_size != 1 {
        bail!("only single-image inference is supported, got batch_size={batch_size}");
    }
    if num_classes != config.num_labels() {
        bail!(
            "model output label count mismatch: expected {}, got {}",
            config.num_labels(),
            num_classes
        );
    }

    let logits = logits.flatten_all()?.to_vec1::<f32>()?;
    let pred_boxes = pred_boxes.flatten_all()?.to_vec1::<f32>()?;
    let mut scored = Vec::with_capacity(num_queries * num_classes);
    for query_index in 0..num_queries {
        for class_id in 0..num_classes {
            let index = query_index * num_classes + class_id;
            scored.push((sigmoid_scalar(logits[index]), query_index, class_id));
        }
    }
    scored.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(num_queries);

    let (image_width, image_height) = target_size;
    let image_width = image_width as f32;
    let image_height = image_height as f32;
    let mut detections = Vec::new();
    for (score, query_index, class_id) in scored {
        if score < threshold {
            continue;
        }
        let box_offset = query_index * 4;
        let bbox = scale_box_to_image(
            [
                pred_boxes[box_offset],
                pred_boxes[box_offset + 1],
                pred_boxes[box_offset + 2],
                pred_boxes[box_offset + 3],
            ],
            image_width,
            image_height,
        );
        detections.push(ComicTextBubbleRegion {
            label_id: class_id,
            label: config.label(class_id),
            score,
            bbox,
        });
    }

    Ok(detections)
}

fn detections_to_text_blocks(
    image_dimensions: (u32, u32),
    detections: &[ComicTextBubbleRegion],
) -> Vec<TextRegion> {
    let text_boxes = merge_text_regions(
        detections
            .iter()
            .filter(|region| region.is_text())
            .cloned()
            .collect::<Vec<_>>(),
    );

    let (image_width, image_height) = image_dimensions;
    let image_width = image_width as f32;
    let image_height = image_height as f32;
    let mut blocks = Vec::with_capacity(text_boxes.len());
    for text_region in text_boxes {
        let bbox = clamp_box(text_region.bbox, image_width, image_height);
        let width = (bbox[2] - bbox[0]).max(1.0);
        let height = (bbox[3] - bbox[1]).max(1.0);
        if width <= 5.0 || height <= 5.0 {
            continue;
        }

        let block = TextRegion {
            x: bbox[0],
            y: bbox[1],
            width,
            height,
            confidence: text_region.score,
            detector: Some(DETECTOR_NAME.to_string()),
            ..Default::default()
        };
        blocks.push(block);
    }
    blocks
}

fn filter_and_fix_regions(
    regions: Vec<ComicTextBubbleRegion>,
    image_dimensions: (u32, u32),
) -> Vec<ComicTextBubbleRegion> {
    let (image_width, image_height) = image_dimensions;
    let image_width = image_width as f32;
    let image_height = image_height as f32;
    regions
        .into_iter()
        .filter_map(|mut region| {
            region.bbox = clamp_box(region.bbox, image_width, image_height);
            let width = region.bbox[2] - region.bbox[0];
            let height = region.bbox[3] - region.bbox[1];
            if width <= 5.0 || height <= 5.0 {
                None
            } else {
                Some(region)
            }
        })
        .collect()
}

fn merge_text_regions(mut regions: Vec<ComicTextBubbleRegion>) -> Vec<ComicTextBubbleRegion> {
    let mut merged = Vec::new();
    while let Some(mut candidate) = regions.pop() {
        let mut index = 0usize;
        while index < regions.len() {
            let overlaps = rectangles_overlap(&candidate.bbox, &regions[index].bbox, 0.5)
                || is_mostly_contained(&candidate.bbox, &regions[index].bbox, 0.3)
                || is_mostly_contained(&regions[index].bbox, &candidate.bbox, 0.3);
            if overlaps {
                candidate.bbox = merge_boxes(candidate.bbox, regions[index].bbox);
                candidate.score = candidate.score.max(regions[index].score);
                regions.swap_remove(index);
            } else {
                index += 1;
            }
        }
        merged.push(candidate);
    }
    merged
}

fn merge_slice_regions(
    mut regions: Vec<ComicTextBubbleRegion>,
    image_height: u32,
) -> Vec<ComicTextBubbleRegion> {
    let y_distance_threshold = image_height as f32 * 0.1;
    let mut index = 0usize;
    while index < regions.len() {
        let mut compare = index + 1;
        while compare < regions.len() {
            if regions[index].label_id != regions[compare].label_id {
                compare += 1;
                continue;
            }

            let box1 = regions[index].bbox;
            let box2 = regions[compare].bbox;
            let area1 = box_area(box1);
            let area2 = box_area(box2);
            let iou = calculate_iou(&box1, &box2);
            let (contained, contains_first) = contained_relation(box1, box2, 0.85);

            if contained {
                if !contains_first {
                    regions[index].bbox = box2;
                }
                regions[index].score = regions[index].score.max(regions[compare].score);
                regions.swap_remove(compare);
                continue;
            }

            if iou >= 0.5 {
                if area2 > area1 {
                    regions[index].bbox = box2;
                }
                regions[index].score = regions[index].score.max(regions[compare].score);
                regions.swap_remove(compare);
                continue;
            }

            let width1 = (box1[2] - box1[0]).max(1.0);
            let height1 = (box1[3] - box1[1]).max(1.0);
            let width2 = (box2[2] - box2[0]).max(1.0);
            let height2 = (box2[3] - box2[1]).max(1.0);
            let y_dist = (box1[1] - box2[3]).abs().min((box1[3] - box2[1]).abs());
            let local_y_threshold = y_distance_threshold.min(height1.max(height2) * 0.1);
            let x_overlap = (box1[2].min(box2[2]) - box1[0].max(box2[0])).max(0.0);
            let x_overlap_ratio = x_overlap / width1.min(width2);
            let size_ratio = area1.min(area2) / area1.max(area2);

            if y_dist < local_y_threshold
                && x_overlap_ratio > 0.2
                && size_ratio > 0.3
                && (box1[0] - box2[0]).abs() < 0.5 * width1.max(width2)
                && (box1[2] - box2[2]).abs() < 0.5 * width1.max(width2)
            {
                let merged_box = merge_boxes(box1, box2);
                let merged_area = box_area(merged_box);
                if merged_area <= 3.0 * area1.max(area2) {
                    regions[index].bbox = merged_box;
                    regions[index].score = regions[index].score.max(regions[compare].score);
                    regions.swap_remove(compare);
                    continue;
                }
            }

            compare += 1;
        }
        index += 1;
    }
    regions
}

fn clamp_box(bbox: [f32; 4], image_width: f32, image_height: f32) -> [f32; 4] {
    [
        bbox[0].clamp(0.0, image_width),
        bbox[1].clamp(0.0, image_height),
        bbox[2].clamp(0.0, image_width),
        bbox[3].clamp(0.0, image_height),
    ]
}

fn scale_box_to_image(box_cxcywh: [f32; 4], image_width: f32, image_height: f32) -> [f32; 4] {
    let center_x = box_cxcywh[0] * image_width;
    let center_y = box_cxcywh[1] * image_height;
    let width = box_cxcywh[2] * image_width;
    let height = box_cxcywh[3] * image_height;
    [
        (center_x - width * 0.5).clamp(0.0, image_width),
        (center_y - height * 0.5).clamp(0.0, image_height),
        (center_x + width * 0.5).clamp(0.0, image_width),
        (center_y + height * 0.5).clamp(0.0, image_height),
    ]
}

fn sigmoid_scalar(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn calculate_iou(rect1: &[f32; 4], rect2: &[f32; 4]) -> f32 {
    let x1 = rect1[0].max(rect2[0]);
    let y1 = rect1[1].max(rect2[1]);
    let x2 = rect1[2].min(rect2[2]);
    let y2 = rect1[3].min(rect2[3]);
    let intersection_area = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let union_area = box_area(*rect1) + box_area(*rect2) - intersection_area;
    if union_area <= 0.0 {
        0.0
    } else {
        intersection_area / union_area
    }
}

fn box_area(bbox: [f32; 4]) -> f32 {
    (bbox[2] - bbox[0]).max(0.0) * (bbox[3] - bbox[1]).max(0.0)
}

fn rectangles_overlap(rect1: &[f32; 4], rect2: &[f32; 4], threshold: f32) -> bool {
    calculate_iou(rect1, rect2) >= threshold
}

fn is_mostly_contained(outer: &[f32; 4], inner: &[f32; 4], threshold: f32) -> bool {
    let inner_area = box_area(*inner);
    if inner_area <= 0.0 || box_area(*outer) < inner_area {
        return false;
    }
    let x1 = outer[0].max(inner[0]);
    let y1 = outer[1].max(inner[1]);
    let x2 = outer[2].min(inner[2]);
    let y2 = outer[3].min(inner[3]);
    let overlap = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    overlap / inner_area >= threshold
}

fn contained_relation(box1: [f32; 4], box2: [f32; 4], threshold: f32) -> (bool, bool) {
    let area1 = box_area(box1);
    let area2 = box_area(box2);
    if area1 <= 0.0 || area2 <= 0.0 {
        return (false, false);
    }
    let x1 = box1[0].max(box2[0]);
    let y1 = box1[1].max(box2[1]);
    let x2 = box1[2].min(box2[2]);
    let y2 = box1[3].min(box2[3]);
    let overlap = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let containment_ratio = overlap / area1.min(area2);
    if containment_ratio < threshold {
        return (false, false);
    }
    (true, area1 >= area2)
}

fn merge_boxes(box1: [f32; 4], box2: [f32; 4]) -> [f32; 4] {
    [
        box1[0].min(box2[0]),
        box1[1].min(box2[1]),
        box1[2].max(box2[2]),
        box1[3].max(box2[3]),
    ]
}

#[derive(Debug, Clone)]
struct ImageSlicer {
    height_to_width_ratio_threshold: f32,
    target_slice_ratio: f32,
    overlap_height_ratio: f32,
    min_slice_height_ratio: f32,
}

impl Default for ImageSlicer {
    fn default() -> Self {
        Self {
            height_to_width_ratio_threshold: 3.5,
            target_slice_ratio: 3.0,
            overlap_height_ratio: 0.2,
            min_slice_height_ratio: 0.7,
        }
    }
}

impl ImageSlicer {
    fn should_slice(&self, image: &DynamicImage) -> bool {
        let (width, height) = image.dimensions();
        height as f32 / width.max(1) as f32 > self.height_to_width_ratio_threshold
    }

    fn calculate_slice_params(&self, image: &DynamicImage) -> (u32, u32, usize) {
        let (width, height) = image.dimensions();
        let slice_height = (width as f32 * self.target_slice_ratio).round().max(1.0) as u32;
        let effective_slice_height = (slice_height as f32 * (1.0 - self.overlap_height_ratio))
            .round()
            .max(1.0) as u32;
        let mut num_slices = height.div_ceil(effective_slice_height) as usize;
        if num_slices > 1 {
            let last_slice_start = (num_slices as u32 - 1) * effective_slice_height;
            let last_slice_height = height.saturating_sub(last_slice_start);
            if last_slice_height as f32 / slice_height as f32 <= self.min_slice_height_ratio {
                num_slices -= 1;
            }
        }
        (slice_height, effective_slice_height, num_slices.max(1))
    }

    fn process_slices_for_detection<F>(
        &self,
        image: &DynamicImage,
        detect_fn: F,
    ) -> Result<Vec<ComicTextBubbleRegion>>
    where
        F: Fn(&DynamicImage) -> Result<Vec<ComicTextBubbleRegion>>,
    {
        if !self.should_slice(image) {
            return detect_fn(image);
        }

        let (slice_height, effective_slice_height, num_slices) = self.calculate_slice_params(image);
        let (width, height) = image.dimensions();
        let mut detections = Vec::new();
        for slice_number in 0..num_slices {
            let start_y = slice_number as u32 * effective_slice_height;
            let end_y = if slice_number + 1 == num_slices {
                height
            } else {
                (start_y + slice_height).min(height)
            };
            let crop_height = end_y.saturating_sub(start_y).max(1);
            let cropped = image.crop_imm(0, start_y, width, crop_height);
            let mut slice_detections = detect_fn(&cropped)?;
            for detection in &mut slice_detections {
                detection.bbox[1] += start_y as f32;
                detection.bbox[3] += start_y as f32;
            }
            detections.extend(slice_detections);
        }
        Ok(detections)
    }
}

const fn default_true() -> bool {
    true
}

const fn default_num_channels() -> usize {
    3
}

const fn default_embedding_size() -> usize {
    64
}

fn default_hidden_sizes() -> Vec<usize> {
    vec![256, 512, 1024, 2048]
}

fn default_depths() -> Vec<usize> {
    vec![3, 4, 6, 3]
}

fn default_layer_type() -> String {
    "bottleneck".to_string()
}

fn default_hidden_act() -> String {
    "relu".to_string()
}

fn default_activation_function() -> String {
    "silu".to_string()
}

fn default_decoder_activation_function() -> String {
    "relu".to_string()
}

fn default_encoder_activation_function() -> String {
    "gelu".to_string()
}

const fn default_activation_dropout() -> f64 {
    0.0
}

const fn default_attention_dropout() -> f64 {
    0.0
}

const fn default_batch_norm_eps() -> f64 {
    1e-5
}

const fn default_d_model() -> usize {
    256
}

const fn default_decoder_attention_heads() -> usize {
    8
}

const fn default_decoder_ffn_dim() -> usize {
    1024
}

fn default_decoder_in_channels() -> Vec<usize> {
    vec![256, 256, 256]
}

const fn default_decoder_layers() -> usize {
    6
}

const fn default_decoder_n_levels() -> usize {
    3
}

const fn default_decoder_n_points() -> usize {
    4
}

const fn default_decoder_offset_scale() -> f64 {
    0.5
}

fn default_decoder_method() -> String {
    "default".to_string()
}

const fn default_dropout() -> f64 {
    0.0
}

fn default_encode_proj_layers() -> Vec<usize> {
    vec![2]
}

const fn default_encoder_attention_heads() -> usize {
    8
}

const fn default_encoder_ffn_dim() -> usize {
    1024
}

const fn default_encoder_hidden_dim() -> usize {
    256
}

fn default_encoder_in_channels() -> Vec<usize> {
    vec![512, 1024, 2048]
}

const fn default_encoder_layers() -> usize {
    1
}

fn default_feature_strides() -> Vec<usize> {
    vec![8, 16, 32]
}

const fn default_freeze_backbone_batch_norms() -> bool {
    true
}

const fn default_hidden_expansion() -> f64 {
    1.0
}

const fn default_layer_norm_eps() -> f64 {
    1e-5
}

const fn default_learn_initial_query() -> bool {
    false
}

const fn default_normalize_before() -> bool {
    false
}

const fn default_num_feature_levels() -> usize {
    3
}

const fn default_num_labels() -> usize {
    3
}

const fn default_num_queries() -> usize {
    300
}

const fn default_positional_encoding_temperature() -> usize {
    10_000
}

fn default_out_features() -> Vec<String> {
    vec![
        "stage2".to_string(),
        "stage3".to_string(),
        "stage4".to_string(),
    ]
}

fn default_image_mean() -> [f32; 3] {
    [0.485, 0.456, 0.406]
}

fn default_image_std() -> [f32; 3] {
    [0.229, 0.224, 0.225]
}

const fn default_rescale_factor() -> f32 {
    1.0 / 255.0
}

fn default_processor_size() -> ProcessorSize {
    ProcessorSize {
        height: default_processor_height(),
        width: default_processor_width(),
    }
}

const fn default_processor_height() -> usize {
    640
}

const fn default_processor_width() -> usize {
    640
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_slice_regions_merges_duplicates_per_label() {
        let regions = vec![
            ComicTextBubbleRegion {
                label_id: 0,
                label: "bubble".to_string(),
                score: 0.8,
                bbox: [10.0, 10.0, 50.0, 50.0],
            },
            ComicTextBubbleRegion {
                label_id: 0,
                label: "bubble".to_string(),
                score: 0.7,
                bbox: [12.0, 12.0, 48.0, 48.0],
            },
            ComicTextBubbleRegion {
                label_id: 2,
                label: "text_free".to_string(),
                score: 0.9,
                bbox: [100.0, 100.0, 140.0, 140.0],
            },
        ];
        let merged = merge_slice_regions(regions, 500);
        assert_eq!(merged.len(), 2);
    }
}
