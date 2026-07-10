mod model;

use std::{collections::BTreeMap, time::Instant};

use anyhow::{Context, Result, bail};
use candle_core::{DType, Device, Tensor};
use image::{
    DynamicImage, GenericImageView, GrayImage,
    imageops::{self, FilterType},
};
use imageproc::contours::{BorderType, find_contours};
use koharu_runtime::RuntimeManager;
use serde::{Deserialize, Serialize};

use crate::{device, loading};

use self::model::{PPDocLayoutV3ForObjectDetection, PPDocLayoutV3Outputs};

const HF_REPO: &str = "PaddlePaddle/PP-DocLayoutV3_safetensors";

koharu_runtime::declare_hf_model_package!(
    id: "model:pp-doclayout-v3:config",
    repo: "PaddlePaddle/PP-DocLayoutV3_safetensors",
    file: "config.json",
    bootstrap: false,
    order: 100,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:pp-doclayout-v3:preprocessor-config",
    repo: "PaddlePaddle/PP-DocLayoutV3_safetensors",
    file: "preprocessor_config.json",
    bootstrap: false,
    order: 101,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:pp-doclayout-v3:weights",
    repo: "PaddlePaddle/PP-DocLayoutV3_safetensors",
    file: "model.safetensors",
    bootstrap: false,
    order: 102,
);

#[derive(Debug)]
pub struct PPDocLayoutV3 {
    model: PPDocLayoutV3ForObjectDetection,
    config: PPDocLayoutV3Config,
    preprocessor: PPDocLayoutV3PreprocessorConfig,
    device: Device,
    dtype: DType,
    mean: Tensor,
    std: Tensor,
}

impl PPDocLayoutV3 {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let device = device(cpu)?;
        let dtype = loading::model_dtype(&device);
        let downloads = runtime.downloads();
        let config_path = downloads.huggingface_model(HF_REPO, "config.json").await?;
        let preprocessor_path = downloads
            .huggingface_model(HF_REPO, "preprocessor_config.json")
            .await?;
        let config = loading::read_json::<PPDocLayoutV3Config>(&config_path)
            .with_context(|| format!("failed to load {}", config_path.display()))?;
        let preprocessor =
            loading::read_json::<PPDocLayoutV3PreprocessorConfig>(&preprocessor_path)
                .with_context(|| format!("failed to load {}", preprocessor_path.display()))?;
        let mean =
            Tensor::from_slice(&preprocessor.image_mean, (1, 3, 1, 1), &device)?.to_dtype(dtype)?;
        let std =
            Tensor::from_slice(&preprocessor.image_std, (1, 3, 1, 1), &device)?.to_dtype(dtype)?;
        let weights_path = downloads
            .huggingface_model(HF_REPO, "model.safetensors")
            .await?;
        let model = loading::load_mmaped_safetensors_path_with_dtype(
            &weights_path,
            &device,
            dtype,
            |vb| PPDocLayoutV3ForObjectDetection::load(vb, &config, &device),
        )?;

        Ok(Self {
            model,
            config,
            preprocessor,
            device,
            dtype,
            mean,
            std,
        })
    }

    pub fn inference(
        &self,
        images: &[DynamicImage],
        threshold: f32,
    ) -> Result<Vec<LayoutDetectionResult>> {
        self.inference_impl(images, threshold, true)
    }

    pub fn inference_fast(
        &self,
        images: &[DynamicImage],
        threshold: f32,
    ) -> Result<Vec<LayoutDetectionResult>> {
        self.inference_impl(images, threshold, false)
    }

    fn inference_impl(
        &self,
        images: &[DynamicImage],
        threshold: f32,
        include_polygons: bool,
    ) -> Result<Vec<LayoutDetectionResult>> {
        if images.is_empty() {
            return Ok(Vec::new());
        }

        let started = Instant::now();
        let preprocess_started = Instant::now();
        let pixel_values = preprocess_images(
            images,
            &self.preprocessor,
            &self.device,
            self.dtype,
            &self.mean,
            &self.std,
        )?;
        let preprocess_elapsed = preprocess_started.elapsed();

        let forward_started = Instant::now();
        let outputs = self.model.forward(&pixel_values)?;
        let forward_elapsed = forward_started.elapsed();

        let postprocess_started = Instant::now();
        post_process_outputs(
            &self.config,
            &self.preprocessor,
            &outputs,
            images,
            threshold,
            include_polygons,
        )
        .inspect(|results| {
            tracing::info!(
                images = images.len(),
                include_polygons,
                regions = results
                    .iter()
                    .map(|result| result.regions.len())
                    .sum::<usize>(),
                preprocess_ms = preprocess_elapsed.as_millis(),
                forward_ms = forward_elapsed.as_millis(),
                postprocess_ms = postprocess_started.elapsed().as_millis(),
                total_ms = started.elapsed().as_millis(),
                "pp-doclayout-v3 timings"
            );
        })
    }

    pub fn inference_one(
        &self,
        image: &DynamicImage,
        threshold: f32,
    ) -> Result<LayoutDetectionResult> {
        let mut results = self.inference(std::slice::from_ref(image), threshold)?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("missing layout result"))
    }

    pub fn inference_one_fast(
        &self,
        image: &DynamicImage,
        threshold: f32,
    ) -> Result<LayoutDetectionResult> {
        let mut results = self.inference_fast(std::slice::from_ref(image), threshold)?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("missing layout result"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutDetectionResult {
    pub image_width: u32,
    pub image_height: u32,
    pub regions: Vec<LayoutRegion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutRegion {
    pub order: usize,
    pub label_id: usize,
    pub label: String,
    pub score: f32,
    pub bbox: [f32; 4],
    pub polygon_points: Vec<[f32; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HGNetV2Config {
    #[serde(default = "default_num_channels")]
    pub num_channels: usize,
    #[serde(default = "default_hidden_act")]
    pub hidden_act: String,
    #[serde(default = "default_stem_channels")]
    pub stem_channels: Vec<usize>,
    #[serde(default = "default_stem_strides")]
    pub stem_strides: Vec<usize>,
    #[serde(default = "default_stage_in_channels")]
    pub stage_in_channels: Vec<usize>,
    #[serde(default = "default_stage_mid_channels")]
    pub stage_mid_channels: Vec<usize>,
    #[serde(default = "default_stage_out_channels")]
    pub stage_out_channels: Vec<usize>,
    #[serde(default = "default_stage_num_blocks")]
    pub stage_num_blocks: Vec<usize>,
    #[serde(default = "default_stage_downsample")]
    pub stage_downsample: Vec<bool>,
    #[serde(default = "default_stage_downsample_strides")]
    pub stage_downsample_strides: Vec<usize>,
    #[serde(default = "default_stage_light_block")]
    pub stage_light_block: Vec<bool>,
    #[serde(default = "default_stage_kernel_size")]
    pub stage_kernel_size: Vec<usize>,
    #[serde(default = "default_stage_num_layers")]
    pub stage_numb_of_layers: Vec<usize>,
    #[serde(default)]
    pub use_learnable_affine_block: bool,
}

impl Default for HGNetV2Config {
    fn default() -> Self {
        Self {
            num_channels: default_num_channels(),
            hidden_act: default_hidden_act(),
            stem_channels: default_stem_channels(),
            stem_strides: default_stem_strides(),
            stage_in_channels: default_stage_in_channels(),
            stage_mid_channels: default_stage_mid_channels(),
            stage_out_channels: default_stage_out_channels(),
            stage_num_blocks: default_stage_num_blocks(),
            stage_downsample: default_stage_downsample(),
            stage_downsample_strides: default_stage_downsample_strides(),
            stage_light_block: default_stage_light_block(),
            stage_kernel_size: default_stage_kernel_size(),
            stage_numb_of_layers: default_stage_num_layers(),
            use_learnable_affine_block: false,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PPDocLayoutV3Config {
    #[serde(default = "default_activation_dropout")]
    pub activation_dropout: f64,
    #[serde(default = "default_activation_function")]
    pub activation_function: String,
    #[serde(default)]
    pub anchor_image_size: Option<Vec<usize>>,
    #[serde(default = "default_attention_dropout")]
    pub attention_dropout: f64,
    #[serde(default)]
    pub backbone_config: HGNetV2Config,
    #[serde(default = "default_batch_norm_eps")]
    pub batch_norm_eps: f64,
    #[serde(default = "default_box_noise_scale")]
    pub box_noise_scale: f64,
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
    #[serde(default = "default_decoder_n_points")]
    pub decoder_n_points: usize,
    #[serde(default)]
    pub disable_custom_kernels: bool,
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
    #[serde(default = "default_global_pointer_head_size")]
    pub global_pointer_head_size: usize,
    #[serde(default = "default_gp_dropout_value")]
    pub gp_dropout_value: f64,
    #[serde(default = "default_hidden_expansion")]
    pub hidden_expansion: f64,
    #[serde(default)]
    pub id2label: BTreeMap<String, String>,
    #[serde(default = "default_initializer_range")]
    pub initializer_range: f64,
    #[serde(default = "default_label_noise_ratio")]
    pub label_noise_ratio: f64,
    #[serde(default = "default_layer_norm_eps")]
    pub layer_norm_eps: f64,
    #[serde(default = "default_learn_initial_query")]
    pub learn_initial_query: bool,
    #[serde(default = "default_mask_enhanced")]
    pub mask_enhanced: bool,
    #[serde(default = "default_mask_feature_channels")]
    pub mask_feature_channels: Vec<usize>,
    #[serde(default = "default_normalize_before")]
    pub normalize_before: bool,
    #[serde(default = "default_num_denoising")]
    pub num_denoising: usize,
    #[serde(default = "default_num_feature_levels")]
    pub num_feature_levels: usize,
    #[serde(default = "default_num_prototypes")]
    pub num_prototypes: usize,
    #[serde(default = "default_num_queries")]
    pub num_queries: usize,
    #[serde(default = "default_positional_encoding_temperature")]
    pub positional_encoding_temperature: usize,
    #[serde(default = "default_x4_feat_dim")]
    pub x4_feat_dim: usize,
}

impl PPDocLayoutV3Config {
    pub(crate) fn num_labels(&self) -> usize {
        self.id2label.len().max(25)
    }

    pub(crate) fn label(&self, label_id: usize) -> String {
        self.id2label
            .get(&label_id.to_string())
            .cloned()
            .unwrap_or_else(|| format!("label_{label_id}"))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PPDocLayoutV3PreprocessorConfig {
    #[serde(default = "default_true")]
    pub do_resize: bool,
    #[serde(default = "default_true")]
    pub do_rescale: bool,
    #[serde(default = "default_true")]
    pub do_normalize: bool,
    #[serde(default = "default_zero_mean")]
    pub image_mean: [f32; 3],
    #[serde(default = "default_one_std")]
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

fn preprocess_images(
    images: &[DynamicImage],
    preprocessor: &PPDocLayoutV3PreprocessorConfig,
    device: &Device,
    dtype: DType,
    mean: &Tensor,
    std: &Tensor,
) -> Result<Tensor> {
    let target_h = preprocessor.size.height;
    let target_w = preprocessor.size.width;
    let mut batch = Vec::with_capacity(images.len() * 3 * target_h * target_w);
    for image in images {
        let resized = if preprocessor.do_resize {
            image.resize_exact(target_w as u32, target_h as u32, FilterType::CatmullRom)
        } else {
            image.clone()
        };
        let rgb = resized.to_rgb8();
        batch.extend_from_slice(rgb.as_raw());
    }

    let tensor = Tensor::from_vec(batch, (images.len(), target_h, target_w, 3), &Device::Cpu)?
        .to_device(device)?
        .permute((0, 3, 1, 2))?
        .to_dtype(dtype)?;
    let tensor = if preprocessor.do_rescale {
        tensor.affine(preprocessor.rescale_factor as f64, 0.0)?
    } else {
        tensor
    };
    if preprocessor.do_normalize {
        Ok(tensor.broadcast_sub(mean)?.broadcast_div(std)?)
    } else {
        Ok(tensor)
    }
}

fn post_process_outputs(
    config: &PPDocLayoutV3Config,
    preprocessor: &PPDocLayoutV3PreprocessorConfig,
    outputs: &PPDocLayoutV3Outputs,
    images: &[DynamicImage],
    threshold: f32,
    include_polygons: bool,
) -> Result<Vec<LayoutDetectionResult>> {
    let logits = outputs
        .logits
        .to_dtype(DType::F32)?
        .to_device(&Device::Cpu)?;
    let boxes = outputs
        .pred_boxes
        .to_dtype(DType::F32)?
        .to_device(&Device::Cpu)?;
    let order_logits = outputs
        .order_logits
        .to_dtype(DType::F32)?
        .to_device(&Device::Cpu)?;

    let (batch_size, num_queries, num_classes) = logits.dims3()?;
    if batch_size != images.len() {
        bail!("batch size mismatch between model outputs and images");
    }

    let logits = logits.flatten_all()?.to_vec1::<f32>()?;
    let boxes = boxes.flatten_all()?.to_vec1::<f32>()?;
    let order_logits = order_logits.flatten_all()?.to_vec1::<f32>()?;
    let (masks, mask_h, mask_w) = if include_polygons {
        let masks = outputs
            .out_masks
            .to_dtype(DType::F32)?
            .to_device(&Device::Cpu)?;
        let (_, _, mask_h, mask_w) = masks.dims4()?;
        (Some(masks.flatten_all()?.to_vec1::<f32>()?), mask_h, mask_w)
    } else {
        (None, 0, 0)
    };

    let mut results = Vec::with_capacity(batch_size);
    for batch in 0..batch_size {
        let image = &images[batch];
        let (image_width, image_height) = image.dimensions();
        let order_seq = compute_order_sequence(
            &order_logits
                [batch * num_queries * num_queries..(batch + 1) * num_queries * num_queries],
            num_queries,
        );
        let mut scored = Vec::with_capacity(num_queries * num_classes);
        for query in 0..num_queries {
            for class_id in 0..num_classes {
                let index = ((batch * num_queries + query) * num_classes) + class_id;
                scored.push((sigmoid(logits[index]), query, class_id));
            }
        }
        scored.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
        scored.truncate(num_queries);

        let mut regions = Vec::new();
        for (score, query, class_id) in scored {
            if score < threshold {
                continue;
            }
            let box_offset = (batch * num_queries + query) * 4;
            let bbox = scale_box_to_image(
                [
                    boxes[box_offset],
                    boxes[box_offset + 1],
                    boxes[box_offset + 2],
                    boxes[box_offset + 3],
                ],
                image_width as f32,
                image_height as f32,
            );
            let polygon_points = if let Some(masks) = masks.as_ref() {
                let mask_offset = (batch * num_queries + query) * mask_h * mask_w;
                extract_polygon_points(
                    bbox,
                    &masks[mask_offset..mask_offset + mask_h * mask_w],
                    mask_w,
                    mask_h,
                    preprocessor.size.width as f32 / image_width.max(1) as f32,
                    preprocessor.size.height as f32 / image_height.max(1) as f32,
                    threshold,
                )?
            } else {
                Vec::new()
            };
            regions.push(LayoutRegion {
                order: order_seq[query],
                label_id: class_id,
                label: config.label(class_id),
                score,
                bbox,
                polygon_points,
            });
        }
        regions.sort_unstable_by_key(|region| region.order);
        for (order, region) in regions.iter_mut().enumerate() {
            region.order = order;
        }
        results.push(LayoutDetectionResult {
            image_width,
            image_height,
            regions,
        });
    }

    Ok(results)
}

fn compute_order_sequence(order_logits: &[f32], sequence_length: usize) -> Vec<usize> {
    let mut order_scores = vec![0.0f32; sequence_length * sequence_length];
    for (index, value) in order_logits.iter().copied().enumerate() {
        order_scores[index] = sigmoid(value);
    }

    let mut order_votes = vec![0.0f32; sequence_length];
    for candidate in 0..sequence_length {
        let mut vote = 0.0f32;
        for other in 0..sequence_length {
            if other < candidate {
                vote += order_scores[other * sequence_length + candidate];
            } else if other > candidate {
                vote += 1.0 - order_scores[candidate * sequence_length + other];
            }
        }
        order_votes[candidate] = vote;
    }

    let mut pointers: Vec<usize> = (0..sequence_length).collect();
    pointers.sort_unstable_by(|a, b| order_votes[*a].total_cmp(&order_votes[*b]));
    let mut order_seq = vec![0usize; sequence_length];
    for (rank, pointer) in pointers.into_iter().enumerate() {
        order_seq[pointer] = rank;
    }
    order_seq
}

fn scale_box_to_image(box_cxcywh: [f32; 4], image_width: f32, image_height: f32) -> [f32; 4] {
    let center_x = box_cxcywh[0] * image_width;
    let center_y = box_cxcywh[1] * image_height;
    let width = box_cxcywh[2] * image_width;
    let height = box_cxcywh[3] * image_height;
    let x_min = (center_x - width * 0.5).clamp(0.0, image_width);
    let y_min = (center_y - height * 0.5).clamp(0.0, image_height);
    let x_max = (center_x + width * 0.5).clamp(0.0, image_width);
    let y_max = (center_y + height * 0.5).clamp(0.0, image_height);
    [x_min, y_min, x_max, y_max]
}

fn extract_polygon_points(
    bbox: [f32; 4],
    mask: &[f32],
    mask_width: usize,
    mask_height: usize,
    scale_width: f32,
    scale_height: f32,
    threshold: f32,
) -> Result<Vec<[f32; 2]>> {
    let x_min = bbox[0].round() as i32;
    let y_min = bbox[1].round() as i32;
    let x_max = bbox[2].round() as i32;
    let y_max = bbox[3].round() as i32;
    let box_w = (x_max - x_min).max(1) as u32;
    let box_h = (y_max - y_min).max(1) as u32;
    let rect = rect_polygon(bbox);

    let x0 = ((bbox[0] * (scale_width / 4.0)).round() as i32).clamp(0, mask_width as i32) as usize;
    let x1 = ((bbox[2] * (scale_width / 4.0)).round() as i32).clamp(0, mask_width as i32) as usize;
    let y0 =
        ((bbox[1] * (scale_height / 4.0)).round() as i32).clamp(0, mask_height as i32) as usize;
    let y1 =
        ((bbox[3] * (scale_height / 4.0)).round() as i32).clamp(0, mask_height as i32) as usize;
    if x1 <= x0 || y1 <= y0 {
        return Ok(rect);
    }

    let cropped_w = x1 - x0;
    let cropped_h = y1 - y0;
    let mut cropped = Vec::with_capacity(cropped_w * cropped_h);
    for y in y0..y1 {
        for x in x0..x1 {
            let value = if mask[y * mask_width + x] >= threshold {
                255u8
            } else {
                0u8
            };
            cropped.push(value);
        }
    }
    let Some(cropped) = GrayImage::from_raw(cropped_w as u32, cropped_h as u32, cropped) else {
        return Ok(rect);
    };
    let resized = imageops::resize(&cropped, box_w, box_h, FilterType::Nearest);
    let contours = find_contours::<i32>(&resized);
    let mut best_points = Vec::new();
    let mut best_area = 0.0f32;
    for contour in contours {
        if contour.border_type != BorderType::Outer || contour.points.is_empty() {
            continue;
        }
        let points = contour
            .points
            .into_iter()
            .map(|point| [point.x as f32 + bbox[0], point.y as f32 + bbox[1]])
            .collect::<Vec<_>>();
        let area = polygon_area(&points).abs();
        if area > best_area {
            best_area = area;
            best_points = points;
        }
    }
    if best_points.len() < 4 {
        Ok(rect)
    } else {
        Ok(best_points)
    }
}

fn rect_polygon(bbox: [f32; 4]) -> Vec<[f32; 2]> {
    vec![
        [bbox[0], bbox[1]],
        [bbox[2], bbox[1]],
        [bbox[2], bbox[3]],
        [bbox[0], bbox[3]],
    ]
}

fn polygon_area(points: &[[f32; 2]]) -> f32 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0f32;
    for index in 0..points.len() {
        let next = (index + 1) % points.len();
        sum += points[index][0] * points[next][1] - points[next][0] * points[index][1];
    }
    sum * 0.5
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

const fn default_true() -> bool {
    true
}

const fn default_num_channels() -> usize {
    3
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

const fn default_batch_norm_eps() -> f64 {
    1e-5
}

const fn default_activation_dropout() -> f64 {
    0.0
}

const fn default_attention_dropout() -> f64 {
    0.0
}

const fn default_dropout() -> f64 {
    0.0
}

const fn default_box_noise_scale() -> f64 {
    1.0
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

const fn default_decoder_n_points() -> usize {
    4
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

const fn default_global_pointer_head_size() -> usize {
    64
}

const fn default_gp_dropout_value() -> f64 {
    0.1
}

const fn default_hidden_expansion() -> f64 {
    1.0
}

const fn default_initializer_range() -> f64 {
    0.01
}

const fn default_label_noise_ratio() -> f64 {
    0.5
}

const fn default_layer_norm_eps() -> f64 {
    1e-5
}

const fn default_learn_initial_query() -> bool {
    false
}

const fn default_mask_enhanced() -> bool {
    true
}

fn default_mask_feature_channels() -> Vec<usize> {
    vec![64, 64]
}

const fn default_normalize_before() -> bool {
    false
}

const fn default_num_denoising() -> usize {
    100
}

const fn default_num_feature_levels() -> usize {
    3
}

const fn default_num_prototypes() -> usize {
    32
}

const fn default_num_queries() -> usize {
    300
}

const fn default_positional_encoding_temperature() -> usize {
    10_000
}

const fn default_x4_feat_dim() -> usize {
    128
}

fn default_zero_mean() -> [f32; 3] {
    [0.0, 0.0, 0.0]
}

fn default_one_std() -> [f32; 3] {
    [1.0, 1.0, 1.0]
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
    800
}

const fn default_processor_width() -> usize {
    800
}

fn default_stem_channels() -> Vec<usize> {
    vec![3, 32, 48]
}

fn default_stem_strides() -> Vec<usize> {
    vec![2, 1, 1, 2, 1]
}

fn default_stage_in_channels() -> Vec<usize> {
    vec![48, 128, 512, 1024]
}

fn default_stage_mid_channels() -> Vec<usize> {
    vec![48, 96, 192, 384]
}

fn default_stage_out_channels() -> Vec<usize> {
    vec![128, 512, 1024, 2048]
}

fn default_stage_num_blocks() -> Vec<usize> {
    vec![1, 1, 3, 1]
}

fn default_stage_downsample() -> Vec<bool> {
    vec![false, true, true, true]
}

fn default_stage_downsample_strides() -> Vec<usize> {
    vec![2, 2, 2, 2]
}

fn default_stage_light_block() -> Vec<bool> {
    vec![false, false, true, true]
}

fn default_stage_kernel_size() -> Vec<usize> {
    vec![3, 3, 5, 5]
}

fn default_stage_num_layers() -> Vec<usize> {
    vec![6, 6, 6, 6]
}
