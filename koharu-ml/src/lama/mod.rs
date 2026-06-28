mod model;

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result, bail};
use burn::{
    module::{Module, ModuleMapper, Param},
    store::{ModuleSnapshot, PyTorchToBurnAdapter, SafetensorsStore},
    tensor::{DType, Device, DeviceKind, FloatDType, Tensor, TensorData},
};
use image::{DynamicImage, GenericImageView, GrayImage, RgbImage};
use koharu_runtime::RuntimeManager;
use tracing::instrument;

use crate::{
    inpainting::{
        HdStrategy, HdStrategyConfig, InpaintForward, apply_bubble_fill, binarize_mask,
        extract_alpha, restore_alpha_channel, run_inpaint, run_inpaint_with_windows,
    },
    types::TextRegion,
};

const HF_REPO: &str = "mayocream/lama-manga";
const SAFETENSORS_FILENAME: &str = "lama-manga.safetensors";
const BLOCK_WINDOW_RATIO: f64 = 1.7;
const BLOCK_WINDOW_ASPECT_RATIO: f64 = 1.0;

type Xyxy = [u32; 4];

#[derive(Debug)]
pub struct Lama {
    model: model::Lama,
    device: Device,
    dtype: DType,
}

impl Lama {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let weights_path = resolve_model_path(runtime).await?;
        Self::load_from_weights_path(weights_path, cpu)
    }

    pub fn load_from_weights_path(weights_path: impl AsRef<Path>, cpu: bool) -> Result<Self> {
        let (device, dtype, module_dtype) = make_device(cpu);
        let model = load_model(weights_path.as_ref(), &device, module_dtype)?;
        Ok(Self {
            model,
            device,
            dtype,
        })
    }

    /// Run inpainting with the manga-tuned default strategy (Crop, 800/128/1280).
    #[instrument(level = "debug", skip_all)]
    pub fn inference(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        bubble_mask: &DynamicImage,
    ) -> Result<DynamicImage> {
        let cfg = HdStrategyConfig {
            strategy: HdStrategy::Resize,
            resize_limit: 800,
            ..HdStrategyConfig::lama_default()
        };
        self.inference_with_config_and_blocks(image, mask, bubble_mask, None, &cfg)
    }

    /// Run inpainting with scene text regions as crop-planning hints. LaMa
    /// uses these to build larger semantic windows than raw mask contours.
    #[instrument(level = "debug", skip_all)]
    pub fn inference_with_blocks(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        bubble_mask: &DynamicImage,
        text_blocks: &[TextRegion],
    ) -> Result<DynamicImage> {
        let cfg = HdStrategyConfig {
            strategy: HdStrategy::Resize,
            resize_limit: 800,
            ..HdStrategyConfig::lama_default()
        };
        self.inference_with_config_and_blocks(image, mask, bubble_mask, Some(text_blocks), &cfg)
    }

    /// Run inpainting with a caller-supplied [`HdStrategyConfig`]. Use this to
    /// pick a different strategy (Original / Resize) or tune the trigger /
    /// margin / resize-limit for GPUs with less VRAM.
    #[instrument(level = "debug", skip_all)]
    pub fn inference_with_config(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        bubble_mask: &DynamicImage,
        cfg: &HdStrategyConfig,
    ) -> Result<DynamicImage> {
        self.inference_with_config_and_blocks(image, mask, bubble_mask, None, cfg)
    }

    /// Variant of [`Self::inference_with_config`] that also accepts text
    /// regions for crop planning.
    #[instrument(level = "debug", skip_all)]
    pub fn inference_with_config_and_blocks(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        bubble_mask: &DynamicImage,
        text_blocks: Option<&[TextRegion]>,
        cfg: &HdStrategyConfig,
    ) -> Result<DynamicImage> {
        if image.dimensions() != mask.dimensions() || image.dimensions() != bubble_mask.dimensions()
        {
            bail!(
                "image/mask/bubble dimensions dismatch: image is {:?}, mask is {:?}, bubble is {:?}",
                image.dimensions(),
                mask.dimensions(),
                bubble_mask.dimensions()
            );
        }

        let started = Instant::now();
        let binary_mask = binarize_mask(mask);
        let bubble_mask = bubble_mask.to_luma8();
        let image_rgb = image.to_rgb8();
        let crop_windows = text_blocks
            .filter(|blocks| !blocks.is_empty())
            .map(|blocks| crop_windows_from_text_blocks(blocks, image.width(), image.height()))
            .filter(|windows| !windows.is_empty());
        let forward = LamaForward { lama: self };
        let output_rgb = if let Some(windows) = crop_windows.as_deref() {
            tracing::debug!(
                text_block_count = text_blocks.map_or(0, <[TextRegion]>::len),
                crop_window_count = windows.len(),
                "lama text-aware crop planning"
            );
            run_inpaint_with_windows(
                &forward,
                &image_rgb,
                &binary_mask,
                Some(&bubble_mask),
                cfg,
                Some(windows),
            )?
        } else {
            run_inpaint(&forward, &image_rgb, &binary_mask, Some(&bubble_mask), cfg)?
        };

        tracing::info!(
            width = image.width(),
            height = image.height(),
            resize_limit = cfg.resize_limit,
            total_ms = started.elapsed().as_millis(),
            "lama inpainting timings"
        );

        if image.color().has_alpha() {
            let alpha = extract_alpha(&image.to_rgba8());
            let output = restore_alpha_channel(&output_rgb, &alpha, &binary_mask);
            Ok(DynamicImage::ImageRgba8(output))
        } else {
            Ok(DynamicImage::ImageRgb8(output_rgb))
        }
    }

    #[instrument(level = "debug", skip_all)]
    fn forward_rgb(&self, image: &RgbImage, mask: &GrayImage) -> Result<RgbImage> {
        let (image_tensor, mask_tensor) = self.preprocess(image, mask);
        let output = self.device.memory_persistent_allocations(
            (image_tensor, mask_tensor),
            |(image_tensor, mask_tensor)| self.model.forward(image_tensor, mask_tensor),
        );
        self.postprocess(output)
    }

    #[instrument(level = "debug", skip_all)]
    fn preprocess(&self, image: &RgbImage, mask: &GrayImage) -> (Tensor<4>, Tensor<4>) {
        let (width, height) = (image.width() as usize, image.height() as usize);
        let plane = width * height;
        let rgb = image.as_raw();
        let luma = mask.as_raw();
        let mut image_data = vec![0.0_f32; 3 * plane];
        let mut mask_data = vec![0.0_f32; plane];

        for index in 0..plane {
            mask_data[index] = if luma[index] > 1 { 1.0 } else { 0.0 };
            for channel in 0..3 {
                image_data[channel * plane + index] = rgb[index * 3 + channel] as f32 / 255.0;
            }
        }

        let mut image_tensor_data = TensorData::new(image_data, [1, 3, height, width]);
        let mut mask_tensor_data = TensorData::new(mask_data, [1, 1, height, width]);
        self.device
            .staging([&mut image_tensor_data, &mut mask_tensor_data].into_iter());
        let image_tensor = Tensor::<4>::from_data(image_tensor_data, (&self.device, self.dtype));
        let mask_tensor = Tensor::<4>::from_data(mask_tensor_data, (&self.device, self.dtype));
        (image_tensor, mask_tensor)
    }

    #[instrument(level = "debug", skip_all)]
    fn postprocess(&self, output: Tensor<4>) -> Result<RgbImage> {
        let output = output
            .cast(FloatDType::F32)
            .squeeze_dim::<3>(0)
            .permute([1, 2, 0]);
        let [height, width, channels] = output.dims();
        if channels != 3 {
            bail!("expected 3 channels in output, got {channels}");
        }

        let raw = tensor_to_f32_vec((output * 255.0).clamp(0.0, 255.0))?
            .into_iter()
            .map(|value| value.round().clamp(0.0, 255.0) as u8)
            .collect::<Vec<_>>();
        RgbImage::from_raw(width as u32, height as u32, raw)
            .ok_or_else(|| anyhow::anyhow!("failed to create image buffer from model output"))
    }
}

/// [`InpaintForward`] impl used by the HD-strategy dispatcher. Applies the
/// balloon-fill fast path on a per-crop basis before falling back to the
/// model forward; flat-background speech bubbles skip the model entirely.
struct LamaForward<'a> {
    lama: &'a Lama,
}

impl InpaintForward for LamaForward<'_> {
    fn forward(
        &self,
        image: &RgbImage,
        mask: &GrayImage,
        bubble_mask: Option<&GrayImage>,
    ) -> Result<RgbImage> {
        if mask.pixels().all(|p| p.0[0] == 0) {
            return Ok(image.clone());
        }

        let (image, mask) = if let Some(bubble_mask) = bubble_mask {
            let filled = apply_bubble_fill(image, mask, bubble_mask);
            tracing::debug!(
                filled_pixels = filled.filled_pixels,
                "lama bubble fill fast path"
            );
            (filled.image, filled.remaining_mask)
        } else {
            (image.clone(), mask.clone())
        };

        if mask.pixels().all(|p| p.0[0] == 0) {
            return Ok(image);
        }
        self.lama.forward_rgb(&image, &mask)
    }
}

pub async fn prefetch(runtime: &RuntimeManager) -> Result<()> {
    let _ = resolve_model_path(runtime).await?;
    Ok(())
}

async fn resolve_model_path(runtime: &RuntimeManager) -> Result<PathBuf> {
    runtime
        .downloads()
        .huggingface_model(HF_REPO, SAFETENSORS_FILENAME)
        .await
        .with_context(|| format!("failed to download {SAFETENSORS_FILENAME} from {HF_REPO}"))
}

fn load_model(path: &Path, device: &Device, module_dtype: FloatDType) -> Result<model::Lama> {
    let mut model = model::Lama::new(device);
    let mut store = SafetensorsStore::from_file(path)
        .with_from_adapter(PyTorchToBurnAdapter)
        .with_key_remapping(r"^model\.1\.", "init.")
        .with_key_remapping(r"^model\.2\.", "down1.")
        .with_key_remapping(r"^model\.3\.", "down2.")
        .with_key_remapping(r"^model\.4\.", "down3.")
        .with_key_remapping(r"^model\.5\.", "blocks.0.")
        .with_key_remapping(r"^model\.6\.", "blocks.1.")
        .with_key_remapping(r"^model\.7\.", "blocks.2.")
        .with_key_remapping(r"^model\.8\.", "blocks.3.")
        .with_key_remapping(r"^model\.9\.", "blocks.4.")
        .with_key_remapping(r"^model\.10\.", "blocks.5.")
        .with_key_remapping(r"^model\.11\.", "blocks.6.")
        .with_key_remapping(r"^model\.12\.", "blocks.7.")
        .with_key_remapping(r"^model\.13\.", "blocks.8.")
        .with_key_remapping(r"^model\.14\.", "blocks.9.")
        .with_key_remapping(r"^model\.15\.", "blocks.10.")
        .with_key_remapping(r"^model\.16\.", "blocks.11.")
        .with_key_remapping(r"^model\.17\.", "blocks.12.")
        .with_key_remapping(r"^model\.18\.", "blocks.13.")
        .with_key_remapping(r"^model\.19\.", "blocks.14.")
        .with_key_remapping(r"^model\.20\.", "blocks.15.")
        .with_key_remapping(r"^model\.21\.", "blocks.16.")
        .with_key_remapping(r"^model\.22\.", "blocks.17.")
        .with_key_remapping(r"^model\.24\.", "up1.conv.")
        .with_key_remapping(r"^model\.25\.", "up1.bn.")
        .with_key_remapping(r"^model\.27\.", "up2.conv.")
        .with_key_remapping(r"^model\.28\.", "up2.bn.")
        .with_key_remapping(r"^model\.30\.", "up3.conv.")
        .with_key_remapping(r"^model\.31\.", "up3.bn.")
        .with_key_remapping(r"^model\.34\.", "final_conv.")
        .with_key_remapping(r"\.ffc\.(convl2l|convl2g|convg2l)\.", ".ffc.$1.conv.")
        .with_key_remapping(r"\.convg2g\.conv1\.0\.", ".convg2g.conv1.conv.")
        .with_key_remapping(r"\.convg2g\.conv1\.1\.", ".convg2g.conv1.bn.")
        .with_key_remapping(r"(^|\.)(bn_l|bn_g|bn)\.weight$", "$1$2.gamma")
        .with_key_remapping(r"(^|\.)(bn_l|bn_g|bn)\.bias$", "$1$2.beta")
        .skip_enum_variants(true)
        .allow_partial(false);
    let result = model
        .load_from(&mut store)
        .context("failed to mmap/load LaMa safetensors through Burn store")?;
    if !result.errors.is_empty() {
        bail!("failed to load LaMa tensors: {}", result);
    }
    if !result.missing.is_empty() {
        bail!("LaMa checkpoint is missing tensors: {}", result);
    }

    let model = cast_module_float(model, module_dtype);
    if std::env::var_os("KOHARU_LAMA_FOLD_BN").is_some() {
        Ok(model.fuse_batch_norms())
    } else {
        Ok(model)
    }
}

fn make_device(cpu: bool) -> (Device, DType, FloatDType) {
    #[cfg(feature = "cuda")]
    {
        if !cpu {
            let mut device = Device::cuda(0);
            if let Err(error) = device.configure(FloatDType::F16) {
                tracing::warn!(%error, "failed to configure Burn CUDA default dtype to F16");
            }
            return (device, DType::F16, FloatDType::F16);
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
    (device, DType::F32, FloatDType::F32)
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

fn tensor_to_f32_vec<const D: usize>(tensor: Tensor<D>) -> Result<Vec<f32>> {
    tensor
        .cast(FloatDType::F32)
        .into_data()
        .into_vec::<f32>()
        .context("failed to extract burn tensor data as f32")
}

fn crop_windows_from_text_blocks(text_blocks: &[TextRegion], width: u32, height: u32) -> Vec<Xyxy> {
    let mut windows = Vec::with_capacity(text_blocks.len());
    for block in text_blocks {
        let Some(block_box) = block_xyxy(block, width, height) else {
            continue;
        };
        let window = enlarge_window(
            block_box,
            width,
            height,
            BLOCK_WINDOW_RATIO,
            BLOCK_WINDOW_ASPECT_RATIO,
        );
        if window[2] > window[0] && window[3] > window[1] {
            windows.push(window);
        }
    }
    merge_overlapping_windows(windows)
}

fn block_xyxy(block: &TextRegion, width: u32, height: u32) -> Option<Xyxy> {
    let x1 = block.x.floor().max(0.0) as u32;
    let y1 = block.y.floor().max(0.0) as u32;
    let x2 = (block.x + block.width).ceil().max(block.x.floor()) as u32;
    let y2 = (block.y + block.height).ceil().max(block.y.floor()) as u32;

    let x1 = x1.min(width);
    let y1 = y1.min(height);
    let x2 = x2.min(width);
    let y2 = y2.min(height);

    if x2 <= x1 || y2 <= y1 {
        return None;
    }

    Some([x1, y1, x2, y2])
}

fn enlarge_window(rect: Xyxy, im_w: u32, im_h: u32, ratio: f64, aspect_ratio: f64) -> Xyxy {
    debug_assert!(ratio > 1.0);

    let [x1, y1, x2, y2] = rect;
    let w = f64::from(x2.saturating_sub(x1));
    let h = f64::from(y2.saturating_sub(y1));
    if w <= 0.0 || h <= 0.0 || aspect_ratio <= 0.0 {
        return [0, 0, 0, 0];
    }

    let a = aspect_ratio;
    let b = w + h * aspect_ratio;
    let c = (1.0 - ratio) * w * h;
    let discriminant = (b * b - 4.0 * a * c).max(0.0);
    let delta = ((-b + discriminant.sqrt()) / (2.0 * a) / 2.0).round();
    let mut delta_h = delta.max(0.0) as u32;
    let mut delta_w = (delta * aspect_ratio).round().max(0.0) as u32;

    delta_w = delta_w.min(x1).min(im_w.saturating_sub(x2));
    delta_h = delta_h.min(y1).min(im_h.saturating_sub(y2));

    [
        x1.saturating_sub(delta_w),
        y1.saturating_sub(delta_h),
        (x2 + delta_w).min(im_w),
        (y2 + delta_h).min(im_h),
    ]
}

fn merge_overlapping_windows(mut windows: Vec<Xyxy>) -> Vec<Xyxy> {
    windows.sort_by_key(|window| (window[0], window[1], window[2], window[3]));
    let mut merged = Vec::with_capacity(windows.len());
    for window in windows {
        merge_window_into(&mut merged, window);
    }
    merged.sort_by_key(|window| (window[0], window[1], window[2], window[3]));
    merged
}

fn merge_window_into(merged: &mut Vec<Xyxy>, mut window: Xyxy) {
    loop {
        let Some(index) = merged
            .iter()
            .position(|candidate| windows_touch_or_overlap(*candidate, window))
        else {
            merged.push(window);
            return;
        };
        window = union_xyxy(merged.swap_remove(index), window);
    }
}

fn windows_touch_or_overlap(a: Xyxy, b: Xyxy) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

fn union_xyxy(a: Xyxy, b: Xyxy) -> Xyxy {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

#[cfg(test)]
mod tests {
    use crate::inpainting::restore_alpha_channel;
    use crate::types::TextRegion;
    use image::{GrayImage, Luma, Rgb, RgbImage};

    use super::{crop_windows_from_text_blocks, enlarge_window};

    const ALPHA_RING_RADIUS: u8 = 7;

    #[test]
    fn rgba_alpha_restore_uses_surrounding_ring() {
        let image = RgbImage::from_pixel(32, 32, Rgb([20, 30, 40]));
        let mut alpha = GrayImage::from_pixel(32, 32, Luma([255]));
        let mut mask = GrayImage::new(32, 32);

        for y in 10..22 {
            for x in 10..22 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        for y in (10 - u32::from(ALPHA_RING_RADIUS))..(22 + u32::from(ALPHA_RING_RADIUS)) {
            for x in (10 - u32::from(ALPHA_RING_RADIUS))..(22 + u32::from(ALPHA_RING_RADIUS)) {
                if x < 32 && y < 32 && mask.get_pixel(x, y).0[0] == 0 {
                    alpha.put_pixel(x, y, Luma([64]));
                }
            }
        }

        let restored = restore_alpha_channel(&image, &alpha, &mask);
        assert_eq!(restored.get_pixel(15, 15).0[3], 64);
        assert_eq!(restored.get_pixel(2, 2).0[3], 255);
    }

    #[test]
    fn enlarge_window_matches_ratio_1_7_reference() {
        let enlarged = enlarge_window([10, 20, 50, 60], 200, 150, 1.7, 1.0);
        assert_eq!(enlarged, [4, 14, 56, 66]);
    }

    #[test]
    fn crop_windows_merge_overlapping_text_blocks() {
        let windows = crop_windows_from_text_blocks(
            &[
                TextRegion {
                    x: 100.0,
                    y: 100.0,
                    width: 40.0,
                    height: 40.0,
                    ..TextRegion::default()
                },
                TextRegion {
                    x: 145.0,
                    y: 105.0,
                    width: 40.0,
                    height: 40.0,
                    ..TextRegion::default()
                },
            ],
            512,
            512,
        );

        assert_eq!(windows.len(), 1);
        assert!(windows[0][0] <= 100);
        assert!(windows[0][1] <= 100);
        assert!(windows[0][2] >= 185);
        assert!(windows[0][3] >= 145);
    }
}
