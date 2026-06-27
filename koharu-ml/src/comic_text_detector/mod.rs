mod dbnet;
mod postprocess;
mod unet;
mod yolo_v5;

use std::cmp;

use anyhow::{Context, bail};
use burn::{
    module::{Module, ModuleMapper, Param},
    store::{ModuleSnapshot, PyTorchToBurnAdapter, SafetensorsStore},
    tensor::{
        DType, Device, DeviceKind, FloatDType, Tensor, TensorData,
        activation::sigmoid,
        module::{interpolate, max_pool2d},
        ops::{InterpolateMode, InterpolateOptions},
    },
};
use image::{DynamicImage, GenericImageView, GrayImage, imageops::FilterType};
use koharu_runtime::RuntimeManager;
use tracing::instrument;

use crate::types::TextRegion;

pub use postprocess::{
    ComicTextDetection, Quad, crop_text_block_bbox, expanded_text_block_crop_bounds,
    extract_text_block_regions, refine_segmentation_mask,
};

const HF_REPO: &str = "mayocream/comic-text-detector";
const CONFIDENCE_THRESHOLD: f32 = 0.4;
const NMS_THRESHOLD: f32 = 0.35;
const DBNET_BINARIZE_K: f64 = 50.0;
const BINARY_THRESHOLD: u8 = 60;
const DILATION_RADIUS: u32 = 3;
const HOLE_CLOSE_RADIUS: u32 = 10;
const BBOX_DILATION: f32 = 1.0;
const GPU_DETECT_SIZE: u32 = 1024;
const CPU_DETECT_SIZE: u32 = 640;

koharu_runtime::declare_hf_model_package!(
    id: "model:comic-text-detector:yolo-v5",
    repo: HF_REPO,
    file: "yolo-v5.safetensors",
    bootstrap: false,
    order: 110,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:comic-text-detector:unet",
    repo: HF_REPO,
    file: "unet.safetensors",
    bootstrap: false,
    order: 111,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:comic-text-detector:dbnet",
    repo: HF_REPO,
    file: "dbnet.safetensors",
    bootstrap: false,
    order: 112,
);

pub struct ComicTextDetector {
    yolo: yolo_v5::YoloV5,
    unet: unet::UNet,
    dbnet: Option<dbnet::DbNet>,
    device: Device,
    dtype: DType,
    image_size: u32,
}

impl ComicTextDetector {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> anyhow::Result<Self> {
        Self::load_inner(runtime, cpu, true).await
    }

    pub async fn load_segmentation_only(
        runtime: &RuntimeManager,
        cpu: bool,
    ) -> anyhow::Result<Self> {
        Self::load_inner(runtime, cpu, false).await
    }

    async fn load_inner(
        runtime: &RuntimeManager,
        cpu: bool,
        load_dbnet: bool,
    ) -> anyhow::Result<Self> {
        let (device, dtype, module_dtype, image_size) = make_device(cpu);
        let downloads = runtime.downloads();

        let yolo_path = downloads
            .huggingface_model(HF_REPO, "yolo-v5.safetensors")
            .await?;
        let mut yolo = yolo_v5::YoloV5::new(&device);
        let mut yolo_store = SafetensorsStore::from_file(yolo_path)
            .with_from_adapter(PyTorchToBurnAdapter)
            .with_key_remapping(r"^model\.0\.", "backbone.l0.")
            .with_key_remapping(r"^model\.1\.", "backbone.l1.")
            .with_key_remapping(r"^model\.2\.", "backbone.l2.")
            .with_key_remapping(r"^model\.3\.", "backbone.l3.")
            .with_key_remapping(r"^model\.4\.", "backbone.l4.")
            .with_key_remapping(r"^model\.5\.", "backbone.l5.")
            .with_key_remapping(r"^model\.6\.", "backbone.l6.")
            .with_key_remapping(r"^model\.7\.", "backbone.l7.")
            .with_key_remapping(r"^model\.8\.", "backbone.l8.")
            .with_key_remapping(r"^model\.9\.", "backbone.l9.")
            .with_key_remapping(r"^model\.10\.", "neck.l10.")
            .with_key_remapping(r"^model\.13\.", "neck.l13.")
            .with_key_remapping(r"^model\.14\.", "neck.l14.")
            .with_key_remapping(r"^model\.17\.", "neck.l17.")
            .with_key_remapping(r"^model\.18\.", "neck.l18.")
            .with_key_remapping(r"^model\.20\.", "neck.l20.")
            .with_key_remapping(r"^model\.21\.", "neck.l21.")
            .with_key_remapping(r"^model\.23\.", "neck.l23.")
            .with_key_remapping(r"^model\.24\.", "head.")
            .skip_enum_variants(true)
            .allow_partial(false);
        let result = yolo.load_from(&mut yolo_store).context(
            "failed to mmap/load comic text detector YOLO safetensors through Burn store",
        )?;
        if !result.errors.is_empty() {
            bail!(
                "failed to load comic text detector YOLO tensors: {}",
                result
            );
        }
        if !result.missing.is_empty() {
            bail!(
                "comic text detector YOLO checkpoint is missing tensors: {}",
                result
            );
        }
        yolo = cast_module_float(yolo, module_dtype);

        let unet_path = downloads
            .huggingface_model(HF_REPO, "unet.safetensors")
            .await?;
        let mut unet = unet::UNet::new(&device);
        let mut unet_store = SafetensorsStore::from_file(unet_path)
            .with_from_adapter(PyTorchToBurnAdapter)
            .with_key_remapping(r"^(.+)\.conv\.0\.", "$1.conv.c3.")
            .with_key_remapping(r"^(.+)\.conv\.1\.", "$1.conv.deconv.")
            .with_key_remapping(r"^(.+)\.conv\.2\.", "$1.conv.bn.")
            .with_key_remapping(r"^upconv6\.0\.", "upconv6.conv.")
            .skip_enum_variants(true)
            .allow_partial(false);
        let result = unet.load_from(&mut unet_store).context(
            "failed to mmap/load comic text detector UNet safetensors through Burn store",
        )?;
        if !result.errors.is_empty() {
            bail!(
                "failed to load comic text detector UNet tensors: {}",
                result
            );
        }
        if !result.missing.is_empty() {
            bail!(
                "comic text detector UNet checkpoint is missing tensors: {}",
                result
            );
        }
        unet = cast_module_float(unet, module_dtype);

        let dbnet = if load_dbnet {
            let dbnet_path = downloads
                .huggingface_model(HF_REPO, "dbnet.safetensors")
                .await?;
            let mut dbnet = dbnet::DbNet::new(&device);
            let mut dbnet_store = SafetensorsStore::from_file(dbnet_path)
                .with_from_adapter(PyTorchToBurnAdapter)
                .with_key_remapping(r"^(.+)\.conv\.0\.", "$1.conv.c3.")
                .with_key_remapping(r"^(.+)\.conv\.1\.", "$1.conv.deconv.")
                .with_key_remapping(r"^(.+)\.conv\.2\.", "$1.conv.bn.")
                .with_key_remapping(r"^conv\.0\.", "conv.conv.")
                .with_key_remapping(r"^conv\.1\.", "conv.bn.")
                .with_key_remapping(r"^binarize\.0\.", "binarize.conv1.conv.")
                .with_key_remapping(r"^binarize\.1\.", "binarize.conv1.bn.")
                .with_key_remapping(r"^binarize\.3\.", "binarize.deconv1.")
                .with_key_remapping(r"^binarize\.4\.", "binarize.bn1.")
                .with_key_remapping(r"^binarize\.6\.", "binarize.deconv2.")
                .with_key_remapping(r"^thresh\.0\.", "thresh.conv1.conv.")
                .with_key_remapping(r"^thresh\.1\.", "thresh.conv1.bn.")
                .with_key_remapping(r"^thresh\.3\.", "thresh.deconv1.")
                .with_key_remapping(r"^thresh\.4\.", "thresh.bn1.")
                .with_key_remapping(r"^thresh\.6\.", "thresh.deconv2.")
                .skip_enum_variants(true)
                .allow_partial(false);
            let result = dbnet.load_from(&mut dbnet_store).context(
                "failed to mmap/load comic text detector DBNet safetensors through Burn store",
            )?;
            if !result.errors.is_empty() {
                bail!(
                    "failed to load comic text detector DBNet tensors: {}",
                    result
                );
            }
            if !result.missing.is_empty() {
                bail!(
                    "comic text detector DBNet checkpoint is missing tensors: {}",
                    result
                );
            }
            Some(cast_module_float(dbnet, module_dtype))
        } else {
            None
        };

        Ok(Self {
            yolo,
            unet,
            dbnet,
            device,
            dtype,
            image_size,
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(&self, image: &DynamicImage) -> anyhow::Result<ComicTextDetection> {
        let original_dimensions = image.dimensions();
        let (image_tensor, resized_dimensions) =
            preprocess(image, &self.device, self.dtype, self.image_size)?;
        let (predictions, mask, shrink_threshold) = self.forward(image_tensor)?;

        let bboxes = postprocess_yolo(predictions, original_dimensions, resized_dimensions)?;
        let shrink_map = tensor_channel_to_gray_resized(
            shrink_threshold
                .clone()
                .narrow(1, 0, 1)
                .squeeze_dims::<2>(&[0, 1]),
            original_dimensions.0,
            original_dimensions.1,
        )?;
        let threshold_map = tensor_channel_to_gray_resized(
            shrink_threshold
                .clone()
                .narrow(1, 1, 1)
                .squeeze_dims::<2>(&[0, 1]),
            original_dimensions.0,
            original_dimensions.1,
        )?;
        let mask = postprocess_mask(
            mask,
            shrink_threshold,
            original_dimensions,
            resized_dimensions,
        )?;

        Ok(ComicTextDetection {
            shrink_map,
            threshold_map,
            line_polygons: Vec::new(),
            text_blocks: bboxes_to_text_blocks(bboxes),
            mask,
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_segmentation(&self, image: &DynamicImage) -> anyhow::Result<GrayImage> {
        let original_dimensions = image.dimensions();
        let (image_tensor, resized_dimensions) =
            preprocess(image, &self.device, self.dtype, self.image_size)?;
        let mask = self.forward_mask(image_tensor);
        postprocess_unet_mask(mask, original_dimensions, resized_dimensions)
    }

    #[instrument(level = "debug", skip_all)]
    fn forward(&self, image: Tensor<4>) -> anyhow::Result<(Tensor<3>, Tensor<4>, Tensor<4>)> {
        let (predictions, features) = self.yolo.forward(image);
        let (mask, features) = self.unet.forward(
            features[0].clone(),
            features[1].clone(),
            features[2].clone(),
            features[3].clone(),
            features[4].clone(),
        );
        let dbnet = self
            .dbnet
            .as_ref()
            .context("DBNet not loaded; use ComicTextDetector::load for full detection")?;
        let shrink_thresh = dbnet.forward(
            features[0].clone(),
            features[1].clone(),
            features[2].clone(),
        );

        Ok((predictions, mask, shrink_thresh))
    }

    #[instrument(level = "debug", skip_all)]
    fn forward_mask(&self, image: Tensor<4>) -> Tensor<4> {
        let (_, features) = self.yolo.forward(image);
        let (mask, _) = self.unet.forward(
            features[0].clone(),
            features[1].clone(),
            features[2].clone(),
            features[3].clone(),
            features[4].clone(),
        );
        mask
    }
}

fn make_device(cpu: bool) -> (Device, DType, FloatDType, u32) {
    #[cfg(feature = "cuda")]
    {
        if !cpu {
            let mut device = Device::cuda(0);
            if let Err(error) = device.configure(FloatDType::BF16) {
                tracing::warn!(%error, "failed to configure Burn CUDA default dtype to BF16");
            }
            return (device, DType::BF16, FloatDType::BF16, GPU_DETECT_SIZE);
        }
    }

    let mut device = Device::wgpu(if cpu {
        DeviceKind::Cpu
    } else {
        DeviceKind::DefaultDevice
    });
    if let Err(error) = device.configure(FloatDType::F32) {
        tracing::warn!(%error, "failed to configure Burn WGPU default dtype to F32");
    }
    let image_size = if cpu {
        CPU_DETECT_SIZE
    } else {
        GPU_DETECT_SIZE
    };
    (device, DType::F32, FloatDType::F32, image_size)
}

#[instrument(level = "debug", skip_all)]
fn preprocess(
    image: &DynamicImage,
    device: &Device,
    dtype: DType,
    image_size: u32,
) -> anyhow::Result<(Tensor<4>, (u32, u32))> {
    let (orig_w, orig_h) = image.dimensions();
    let (width, height) = if orig_w >= orig_h {
        (image_size, (image_size * orig_h / orig_w).max(1))
    } else {
        ((image_size * orig_w / orig_h).max(1), image_size)
    };
    let resized = image::imageops::resize(&image.to_rgb8(), width, height, FilterType::Triangle);
    let image_size = image_size as usize;
    let plane = image_size * image_size;
    let mut data = vec![0.0_f32; 3 * plane];
    for (x, y, pixel) in resized.enumerate_pixels() {
        let index = y as usize * image_size + x as usize;
        data[index] = pixel[0] as f32 / 255.0;
        data[plane + index] = pixel[1] as f32 / 255.0;
        data[2 * plane + index] = pixel[2] as f32 / 255.0;
    }

    let mut tensor_data = TensorData::new(data, [1, 3, image_size, image_size]);
    device.staging(std::iter::once(&mut tensor_data));
    Ok((
        Tensor::from_data(tensor_data, (device, dtype)),
        (width, height),
    ))
}

#[derive(Clone, Copy, Debug)]
struct DetectionBBox {
    xmin: f32,
    xmax: f32,
    ymin: f32,
    ymax: f32,
    confidence: f32,
}

#[instrument(level = "debug", skip(predictions))]
fn postprocess_yolo(
    predictions: Tensor<3>,
    original_dimensions: (u32, u32),
    resized_dimensions: (u32, u32),
) -> anyhow::Result<Vec<DetectionBBox>> {
    let [batch, num_predictions, num_outputs] = predictions.dims();
    if batch != 1 || num_outputs < 6 {
        bail!("invalid prediction shape: [{batch}, {num_predictions}, {num_outputs}]");
    }

    let num_classes = num_outputs - 5;
    let (orig_w, orig_h) = original_dimensions;
    let (resized_w, resized_h) = resized_dimensions;
    let w_ratio = orig_w as f32 / resized_w as f32;
    let h_ratio = orig_h as f32 / resized_h as f32;

    let values = tensor_to_f32_vec(predictions)?;
    let mut bboxes: Vec<Vec<DetectionBBox>> = (0..num_classes).map(|_| Vec::new()).collect();
    for pred_idx in 0..num_predictions {
        let base = pred_idx * num_outputs;
        let pred = &values[base..base + num_outputs];
        let (class_index, class_score) = pred[5..]
            .iter()
            .copied()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((0, 0.0));
        let confidence = pred[4] * class_score;
        if confidence < CONFIDENCE_THRESHOLD {
            continue;
        }

        let xmin = ((pred[0] - pred[2] * 0.5) * w_ratio - BBOX_DILATION).clamp(0.0, orig_w as f32);
        let xmax = ((pred[0] + pred[2] * 0.5) * w_ratio + BBOX_DILATION).clamp(0.0, orig_w as f32);
        let ymin = ((pred[1] - pred[3] * 0.5) * h_ratio - BBOX_DILATION).clamp(0.0, orig_h as f32);
        let ymax = ((pred[1] + pred[3] * 0.5) * h_ratio + BBOX_DILATION).clamp(0.0, orig_h as f32);

        if xmax <= xmin || ymax <= ymin {
            continue;
        }

        bboxes[class_index].push(DetectionBBox {
            xmin,
            xmax,
            ymin,
            ymax,
            confidence,
        });
    }

    non_maximum_suppression(&mut bboxes, NMS_THRESHOLD);
    Ok(bboxes.into_iter().flatten().collect())
}

#[instrument(level = "debug", skip(mask, shrink_thresh))]
fn postprocess_mask(
    mask: Tensor<4>,
    shrink_thresh: Tensor<4>,
    original_dimensions: (u32, u32),
    resized_dimensions: (u32, u32),
) -> anyhow::Result<GrayImage> {
    let [_, _, h_db, w_db] = shrink_thresh.dims();
    let [_, _, h_unet, w_unet] = mask.dims();
    let h = cmp::min(h_db, h_unet);
    let w = cmp::min(w_db, w_unet);

    let shrink = shrink_thresh
        .clone()
        .narrow(1, 0, 1)
        .squeeze_dims::<2>(&[0, 1])
        .narrow(0, 0, h)
        .narrow(1, 0, w);
    let thresh = shrink_thresh
        .narrow(1, 1, 1)
        .squeeze_dims::<2>(&[0, 1])
        .narrow(0, 0, h)
        .narrow(1, 0, w);
    let unet_mask = mask
        .squeeze_dims::<2>(&[0, 1])
        .narrow(0, 0, h)
        .narrow(1, 0, w);

    let prob = sigmoid((shrink - thresh) * DBNET_BINARIZE_K);
    let fused = prob.max_pair(unet_mask);

    let [mask_h, mask_w] = fused.dims();
    let valid_h = mask_h.min(resized_dimensions.1 as usize);
    let valid_w = mask_w.min(resized_dimensions.0 as usize);
    let fused = fused
        .narrow(0, 0, valid_h)
        .narrow(1, 0, valid_w)
        .unsqueeze_dim::<3>(0)
        .unsqueeze_dim::<4>(0);

    let resized = interpolate(
        fused,
        [
            original_dimensions.1 as usize,
            original_dimensions.0 as usize,
        ],
        InterpolateOptions::new(InterpolateMode::Bilinear).with_align_corners(false),
    );
    let threshold = BINARY_THRESHOLD as f32 / 255.0;
    let binary = resized.greater_equal_elem(threshold).float();
    let closed = morph_close(binary, HOLE_CLOSE_RADIUS as usize);
    let dilated = dilate_tensor(closed, DILATION_RADIUS as usize);
    let mask = dilated.squeeze_dims::<2>(&[0, 1]);

    let values = tensor_to_f32_vec(mask)?;
    let data = values
        .into_iter()
        .map(|value| if value > 0.5 { 255 } else { 0 })
        .collect::<Vec<_>>();
    GrayImage::from_raw(original_dimensions.0, original_dimensions.1, data)
        .context("failed to build mask image")
}

fn postprocess_unet_mask(
    mask: Tensor<4>,
    original_dimensions: (u32, u32),
    resized_dimensions: (u32, u32),
) -> anyhow::Result<GrayImage> {
    let unet_mask = mask.squeeze_dims::<2>(&[0, 1]);
    let [mask_h, mask_w] = unet_mask.dims();
    let valid_h = mask_h.min(resized_dimensions.1 as usize);
    let valid_w = mask_w.min(resized_dimensions.0 as usize);
    let unet_mask = unet_mask.narrow(0, 0, valid_h).narrow(1, 0, valid_w);
    tensor_channel_to_gray_resized(unet_mask, original_dimensions.0, original_dimensions.1)
}

fn tensor_channel_to_gray_resized(
    tensor: Tensor<2>,
    width: u32,
    height: u32,
) -> anyhow::Result<GrayImage> {
    let [th, tw] = tensor.dims();
    let values = tensor_to_f32_vec(tensor)?;
    let pixels: Vec<u8> = values
        .iter()
        .map(|&v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    let small = GrayImage::from_raw(tw as u32, th as u32, pixels)
        .context("failed to create gray image from tensor")?;
    if tw as u32 == width && th as u32 == height {
        return Ok(small);
    }
    Ok(image::imageops::resize(
        &small,
        width,
        height,
        FilterType::Nearest,
    ))
}

fn bboxes_to_text_blocks(mut bboxes: Vec<DetectionBBox>) -> Vec<TextRegion> {
    bboxes.sort_unstable_by(|a, b| {
        let ay = a.ymin + (a.ymax - a.ymin) * 0.5;
        let by = b.ymin + (b.ymax - b.ymin) * 0.5;
        ay.partial_cmp(&by).unwrap_or(std::cmp::Ordering::Equal)
    });

    bboxes
        .into_iter()
        .map(|bbox| TextRegion {
            x: bbox.xmin,
            y: bbox.ymin,
            width: bbox.xmax - bbox.xmin,
            height: bbox.ymax - bbox.ymin,
            confidence: bbox.confidence,
            ..Default::default()
        })
        .collect()
}

fn non_maximum_suppression(bboxes: &mut [Vec<DetectionBBox>], threshold: f32) {
    for boxes in bboxes {
        boxes.sort_unstable_by(|a, b| b.confidence.total_cmp(&a.confidence));
        let mut keep = Vec::with_capacity(boxes.len());
        for bbox in boxes.drain(..) {
            if keep.iter().all(|kept| bbox_iou(&bbox, kept) <= threshold) {
                keep.push(bbox);
            }
        }
        *boxes = keep;
    }
}

fn bbox_iou(a: &DetectionBBox, b: &DetectionBBox) -> f32 {
    let inter_w = (a.xmax.min(b.xmax) - a.xmin.max(b.xmin)).max(0.0);
    let inter_h = (a.ymax.min(b.ymax) - a.ymin.max(b.ymin)).max(0.0);
    let inter = inter_w * inter_h;
    if inter <= 0.0 {
        return 0.0;
    }
    let area_a = (a.xmax - a.xmin).max(0.0) * (a.ymax - a.ymin).max(0.0);
    let area_b = (b.xmax - b.xmin).max(0.0) * (b.ymax - b.ymin).max(0.0);
    inter / (area_a + area_b - inter).max(f32::EPSILON)
}

fn dilate_tensor(mask: Tensor<4>, radius: usize) -> Tensor<4> {
    let kernel = 2 * radius + 1;
    let padded = mask.pad((radius, radius, radius, radius), 0.0);
    max_pool2d(padded, [kernel, kernel], [1, 1], [0, 0], [1, 1], false)
}

fn erode_tensor(mask: Tensor<4>, radius: usize) -> Tensor<4> {
    let ones = mask.ones_like();
    let inverted = ones.clone() - mask;
    ones - dilate_tensor(inverted, radius)
}

fn morph_close(mask: Tensor<4>, radius: usize) -> Tensor<4> {
    let dilated = dilate_tensor(mask, radius);
    erode_tensor(dilated, radius)
}

fn cast_module_float<M: Module>(module: M, dtype: FloatDType) -> M {
    struct CastMapper {
        dtype: FloatDType,
    }

    impl ModuleMapper for CastMapper {
        fn map_float<const D: usize>(&mut self, param: Param<Tensor<D>>) -> Param<Tensor<D>> {
            let (id, tensor, mapper) = param.consume();
            Param::from_mapped_value(id, tensor.cast(self.dtype), mapper)
        }
    }

    module.map(&mut CastMapper { dtype })
}

fn tensor_to_f32_vec<const D: usize>(tensor: Tensor<D>) -> anyhow::Result<Vec<f32>> {
    tensor
        .cast(FloatDType::F32)
        .into_data()
        .into_vec::<f32>()
        .context("failed to extract burn tensor data as f32")
}

pub async fn prefetch(runtime: &RuntimeManager) -> anyhow::Result<()> {
    let downloads = runtime.downloads();
    downloads
        .huggingface_model(HF_REPO, "yolo-v5.safetensors")
        .await?;
    downloads
        .huggingface_model(HF_REPO, "unet.safetensors")
        .await?;
    downloads
        .huggingface_model(HF_REPO, "dbnet.safetensors")
        .await?;
    Ok(())
}

pub async fn prefetch_segmentation(runtime: &RuntimeManager) -> anyhow::Result<()> {
    let downloads = runtime.downloads();
    downloads
        .huggingface_model(HF_REPO, "yolo-v5.safetensors")
        .await?;
    downloads
        .huggingface_model(HF_REPO, "unet.safetensors")
        .await?;
    Ok(())
}
