mod model;

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::ops::sigmoid;
use image::{DynamicImage, imageops::FilterType};
use koharu_runtime::RuntimeManager;

use crate::{device, loading, probability_map::ProbabilityMap};

const REPO: &str = "mayocream/manga-text-segmentation-2025";
const SAFETENSORS_FILENAME: &str = "model.safetensors";
const IMAGENET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const IMAGENET_STD: [f32; 3] = [0.229, 0.224, 0.225];
const GPU_MAX_PIXELS: u64 = 1_536 * 1_536;
const CPU_MAX_PIXELS: u64 = 1_280 * 1_280;

#[derive(Debug)]
pub struct MangaTextSegmentation {
    model: model::MangaTextSegmentationModel,
    device: Device,
    dtype: DType,
    mean: Tensor,
    std: Tensor,
}

struct PreparedInput {
    pixel_values: Tensor,
    original_width: u32,
    original_height: u32,
    resized_width: u32,
    resized_height: u32,
}

impl MangaTextSegmentation {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let safetensors = resolve_safetensors_path(runtime).await?;
        Self::load_from_path(&safetensors, cpu)
    }

    pub fn load_from_path(path: impl AsRef<Path>, cpu: bool) -> Result<Self> {
        let device = device(cpu)?;
        let dtype = loading::model_dtype(&device);
        let model = loading::load_mmaped_safetensors_path_with_dtype(
            path.as_ref(),
            &device,
            dtype,
            model::MangaTextSegmentationModel::load,
        )?;
        let mean = Tensor::from_slice(&IMAGENET_MEAN, (1, 3, 1, 1), &device)?.to_dtype(dtype)?;
        let std = Tensor::from_slice(&IMAGENET_STD, (1, 3, 1, 1), &device)?.to_dtype(dtype)?;
        Ok(Self {
            model,
            device,
            dtype,
            mean,
            std,
        })
    }

    pub fn inference(&self, image: &DynamicImage) -> Result<ProbabilityMap> {
        let started = Instant::now();
        let preprocess_started = Instant::now();
        let prepared = self.preprocess(image)?;
        let preprocess_elapsed = preprocess_started.elapsed();

        let forward_started = Instant::now();
        let logits = self.model.forward(&prepared.pixel_values)?;
        let forward_elapsed = forward_started.elapsed();

        let postprocess_started = Instant::now();
        let probabilities = sigmoid(&logits)?.i((
            0,
            0,
            0..prepared.resized_height as usize,
            0..prepared.resized_width as usize,
        ))?;
        let probabilities = if prepared.resized_width != prepared.original_width
            || prepared.resized_height != prepared.original_height
        {
            probabilities
                .unsqueeze(0)?
                .unsqueeze(0)?
                .interpolate2d(
                    prepared.original_height as usize,
                    prepared.original_width as usize,
                )?
                .squeeze(0)?
                .squeeze(0)?
        } else {
            probabilities
        }
        .to_dtype(DType::F32)?
        .to_device(&Device::Cpu)?;
        let values = probabilities.flatten_all()?.to_vec1::<f32>()?;
        tracing::info!(
            original_width = prepared.original_width,
            original_height = prepared.original_height,
            resized_width = prepared.resized_width,
            resized_height = prepared.resized_height,
            preprocess_ms = preprocess_elapsed.as_millis(),
            forward_ms = forward_elapsed.as_millis(),
            postprocess_ms = postprocess_started.elapsed().as_millis(),
            total_ms = started.elapsed().as_millis(),
            "manga text segmentation timings"
        );
        Ok(ProbabilityMap {
            width: prepared.original_width,
            height: prepared.original_height,
            values,
        })
    }

    fn preprocess(&self, image: &DynamicImage) -> Result<PreparedInput> {
        let rgb = image.to_rgb8();
        let (original_width, original_height) = rgb.dimensions();
        let (resized_width, resized_height) = scaled_dimensions(
            original_width,
            original_height,
            if self.device.is_cuda() {
                GPU_MAX_PIXELS
            } else {
                CPU_MAX_PIXELS
            },
        );
        let rgb = if resized_width == original_width && resized_height == original_height {
            rgb
        } else {
            image::imageops::resize(&rgb, resized_width, resized_height, FilterType::Triangle)
        };
        let pad_h = (32 - resized_height % 32) % 32;
        let pad_w = (32 - resized_width % 32) % 32;

        let tensor = Tensor::from_vec(
            rgb.into_raw(),
            (1, resized_height as usize, resized_width as usize, 3),
            &self.device,
        )?
        .permute((0, 3, 1, 2))?
        .to_dtype(self.dtype)?;
        let tensor = (tensor * (1.0 / 255.0))?
            .broadcast_sub(&self.mean)?
            .broadcast_div(&self.std)?;
        let tensor = tensor
            .pad_with_zeros(2, 0, pad_h as usize)?
            .pad_with_zeros(3, 0, pad_w as usize)?;

        Ok(PreparedInput {
            pixel_values: tensor,
            original_width,
            original_height,
            resized_width,
            resized_height,
        })
    }
}

pub async fn prefetch(runtime: &RuntimeManager) -> Result<()> {
    let _ = resolve_safetensors_path(runtime).await?;
    Ok(())
}

async fn resolve_safetensors_path(runtime: &RuntimeManager) -> Result<PathBuf> {
    runtime
        .downloads()
        .huggingface_model(REPO, SAFETENSORS_FILENAME)
        .await
        .with_context(|| format!("failed to download {SAFETENSORS_FILENAME} from {REPO}"))
}

fn scaled_dimensions(width: u32, height: u32, max_pixels: u64) -> (u32, u32) {
    let area = u64::from(width) * u64::from(height);
    if area <= max_pixels || max_pixels == 0 {
        return (width.max(1), height.max(1));
    }

    let scale = (max_pixels as f64 / area as f64).sqrt();
    let mut scaled_width = ((width as f64 * scale).floor() as u32).clamp(1, width.max(1));
    let mut scaled_height = ((height as f64 * scale).floor() as u32).clamp(1, height.max(1));
    while u64::from(scaled_width) * u64::from(scaled_height) > max_pixels {
        if scaled_width >= scaled_height && scaled_width > 1 {
            scaled_width -= 1;
        } else if scaled_height > 1 {
            scaled_height -= 1;
        } else {
            break;
        }
    }
    (scaled_width, scaled_height)
}

#[cfg(test)]
mod tests {
    use super::scaled_dimensions;

    #[test]
    fn scaled_dimensions_leave_small_inputs_unchanged() {
        assert_eq!(scaled_dimensions(800, 1200, 2_000_000), (800, 1200));
    }

    #[test]
    fn scaled_dimensions_reduce_large_inputs_to_budget() {
        let (width, height) = scaled_dimensions(3000, 4000, 2_000_000);
        assert!(u64::from(width) * u64::from(height) <= 2_000_000);
        assert!(width < 3000);
        assert!(height < 4000);
    }
}
