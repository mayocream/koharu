mod model;

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result, bail};
use candle_core::{DType, Device, Tensor};
use image::{
    DynamicImage, GenericImageView, GrayImage, RgbImage,
    imageops::{FilterType, resize},
};
use koharu_runtime::RuntimeManager;
use serde::Deserialize;
use tracing::instrument;

use crate::{
    device,
    inpainting::{binarize_mask, extract_alpha, restore_alpha_channel},
    loading,
};

use self::model::{AotGenerator, AotModelSpec};

const HF_REPO: &str = "mayocream/aot-inpainting";
const CONFIG_FILENAME: &str = "config.json";
const SAFETENSORS_FILENAME: &str = "model.safetensors";

koharu_runtime::declare_hf_model_package!(
    id: "model:aot-inpainting:config",
    repo: HF_REPO,
    file: CONFIG_FILENAME,
    bootstrap: false,
    order: 131,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:aot-inpainting:weights",
    repo: HF_REPO,
    file: SAFETENSORS_FILENAME,
    bootstrap: false,
    order: 132,
);

#[derive(Debug)]
pub struct AotInpainting {
    model: AotGenerator,
    config: AotInpaintingConfig,
    device: Device,
}

#[derive(Debug, Clone)]
struct PreparedInput {
    pixel_values: Tensor,
    mask_values: Tensor,
    original_rgb: RgbImage,
    original_mask: GrayImage,
    model_width: u32,
    model_height: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct AotInpaintingConfig {
    model_type: String,
    input_channels: usize,
    output_channels: usize,
    base_channels: usize,
    num_blocks: usize,
    dilation_rates: Vec<usize>,
    pad_multiple: usize,
    default_max_side: u32,
}

impl AotInpaintingConfig {
    fn validate(&self) -> Result<()> {
        if self.model_type != "manga-image-translator-aot" {
            bail!("unsupported AOT inpainting model type {}", self.model_type);
        }
        if self.input_channels != 4 {
            bail!("expected input_channels=4, found {}", self.input_channels);
        }
        if self.output_channels != 3 {
            bail!("expected output_channels=3, found {}", self.output_channels);
        }
        if self.base_channels == 0 {
            bail!("base_channels must be positive");
        }
        if self.num_blocks == 0 {
            bail!("num_blocks must be positive");
        }
        if self.dilation_rates.is_empty() {
            bail!("dilation_rates must not be empty");
        }
        if self.pad_multiple == 0 {
            bail!("pad_multiple must be positive");
        }
        if self.default_max_side == 0 {
            bail!("default_max_side must be positive");
        }
        Ok(())
    }

    fn spec(&self) -> AotModelSpec {
        AotModelSpec {
            input_channels: self.input_channels,
            output_channels: self.output_channels,
            base_channels: self.base_channels,
            num_blocks: self.num_blocks,
            dilation_rates: self.dilation_rates.clone(),
        }
    }
}

impl AotInpainting {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let (config_path, weights_path) = resolve_model_paths(runtime).await?;
        Self::load_from_paths(&config_path, &weights_path, cpu)
    }

    pub fn load_from_paths(
        config_path: impl AsRef<Path>,
        weights_path: impl AsRef<Path>,
        cpu: bool,
    ) -> Result<Self> {
        let device = device(cpu)?;
        let config = loading::read_json::<AotInpaintingConfig>(config_path.as_ref())
            .with_context(|| format!("failed to parse {}", config_path.as_ref().display()))?;
        config.validate()?;
        let model = loading::load_mmaped_safetensors_path(weights_path.as_ref(), &device, |vb| {
            AotGenerator::load(&vb, &config.spec())
        })?;

        Ok(Self {
            model,
            config,
            device,
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(&self, image: &DynamicImage, mask: &DynamicImage) -> Result<DynamicImage> {
        self.inference_with_max_side(image, mask, self.config.default_max_side)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_with_max_side(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        max_side: u32,
    ) -> Result<DynamicImage> {
        if max_side == 0 {
            bail!("max_side must be positive");
        }
        if image.dimensions() != mask.dimensions() {
            bail!(
                "image and mask dimensions dismatch: image is {:?}, mask is {:?}",
                image.dimensions(),
                mask.dimensions()
            );
        }

        let started = Instant::now();
        let prepared = self.preprocess(image, mask, max_side)?;
        let output = self
            .model
            .forward(&prepared.pixel_values, &prepared.mask_values)?;
        let composited = self.postprocess(&output, &prepared)?;

        tracing::info!(
            width = image.width(),
            height = image.height(),
            model_width = prepared.model_width,
            model_height = prepared.model_height,
            max_side,
            total_ms = started.elapsed().as_millis(),
            "aot inpainting timings"
        );

        if image.color().has_alpha() {
            let alpha = extract_alpha(&image.to_rgba8());
            let rgba = restore_alpha_channel(&composited, &alpha, &prepared.original_mask);
            Ok(DynamicImage::ImageRgba8(rgba))
        } else {
            Ok(DynamicImage::ImageRgb8(composited))
        }
    }

    fn preprocess(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        max_side: u32,
    ) -> Result<PreparedInput> {
        let original_rgb = image.to_rgb8();
        let original_mask = binarize_mask(mask);
        let mut working_rgb = original_rgb.clone();
        let mut working_mask = original_mask.clone();

        if working_rgb.width().max(working_rgb.height()) > max_side {
            let (resized_width, resized_height) =
                resize_keep_aspect_dims(working_rgb.width(), working_rgb.height(), max_side);
            working_rgb = resize(
                &working_rgb,
                resized_width,
                resized_height,
                FilterType::Triangle,
            );
            working_mask = resize(
                &working_mask,
                resized_width,
                resized_height,
                FilterType::Triangle,
            );
        }

        let model_width = round_up_multiple(working_rgb.width(), self.config.pad_multiple as u32);
        let model_height = round_up_multiple(working_rgb.height(), self.config.pad_multiple as u32);
        if model_width != working_rgb.width() || model_height != working_rgb.height() {
            working_rgb = resize(
                &working_rgb,
                model_width,
                model_height,
                FilterType::Triangle,
            );
            working_mask = resize(
                &working_mask,
                model_width,
                model_height,
                FilterType::Triangle,
            );
        }

        let mut binary_model_mask = working_mask;
        for pixel in binary_model_mask.pixels_mut() {
            pixel.0[0] = if pixel.0[0] >= 127 { 255 } else { 0 };
        }

        let image_tensor = (Tensor::from_vec(
            working_rgb.into_raw(),
            (1, model_height as usize, model_width as usize, 3),
            &self.device,
        )?
        .permute((0, 3, 1, 2))?
        .to_dtype(DType::F32)?
            / 127.5)?;
        let image_tensor = (image_tensor - 1.0)?;

        let mask_tensor = Tensor::from_vec(
            binary_model_mask.clone().into_raw(),
            (1, model_height as usize, model_width as usize, 1),
            &self.device,
        )?
        .permute((0, 3, 1, 2))?
        .to_dtype(DType::F32)?;
        let mask_tensor = (mask_tensor / 255.0)?;
        let mask_inv = (Tensor::ones_like(&mask_tensor)? - &mask_tensor)?;
        let mask_inv_rgb =
            mask_inv.broadcast_as((1, 3, model_height as usize, model_width as usize))?;
        let masked_image = (&image_tensor * &mask_inv_rgb)?;

        Ok(PreparedInput {
            pixel_values: masked_image,
            mask_values: mask_tensor,
            original_rgb,
            original_mask,
            model_width,
            model_height,
        })
    }

    fn postprocess(&self, output: &Tensor, prepared: &PreparedInput) -> Result<RgbImage> {
        let output = output.to_device(&Device::Cpu)?.squeeze(0)?;
        let (channels, height, width) = output.dims3()?;
        if channels != 3 {
            bail!("expected 3 output channels, got {channels}");
        }

        let raw = ((output + 1.0)? * 127.5)?
            .clamp(0.0, 255.0)?
            .permute((1, 2, 0))?
            .to_dtype(DType::U8)?
            .flatten_all()?
            .to_vec1::<u8>()?;
        let predicted = RgbImage::from_raw(width as u32, height as u32, raw)
            .ok_or_else(|| anyhow::anyhow!("failed to create image buffer from model output"))?;

        let predicted = if width as u32 != prepared.original_rgb.width()
            || height as u32 != prepared.original_rgb.height()
        {
            resize(
                &predicted,
                prepared.original_rgb.width(),
                prepared.original_rgb.height(),
                FilterType::Triangle,
            )
        } else {
            predicted
        };

        Ok(composite_rgb(
            &prepared.original_rgb,
            &predicted,
            &prepared.original_mask,
        ))
    }
}

pub async fn prefetch(runtime: &RuntimeManager) -> Result<()> {
    let _ = resolve_model_paths(runtime).await?;
    Ok(())
}

async fn resolve_model_paths(runtime: &RuntimeManager) -> Result<(PathBuf, PathBuf)> {
    let downloads = runtime.downloads();
    let config = downloads
        .huggingface_model(HF_REPO, CONFIG_FILENAME)
        .await
        .with_context(|| format!("failed to download {CONFIG_FILENAME} from {HF_REPO}"))?;
    let weights = downloads
        .huggingface_model(HF_REPO, SAFETENSORS_FILENAME)
        .await
        .with_context(|| format!("failed to download {SAFETENSORS_FILENAME} from {HF_REPO}"))?;
    Ok((config, weights))
}

fn resize_keep_aspect_dims(width: u32, height: u32, max_side: u32) -> (u32, u32) {
    let ratio = max_side as f32 / width.max(height) as f32;
    (
        ((width as f32 * ratio).round() as u32).max(1),
        ((height as f32 * ratio).round() as u32).max(1),
    )
}

fn round_up_multiple(value: u32, multiple: u32) -> u32 {
    if value.is_multiple_of(multiple) {
        value
    } else {
        value + (multiple - value % multiple)
    }
}

fn composite_rgb(original: &RgbImage, predicted: &RgbImage, mask: &GrayImage) -> RgbImage {
    let mut composited = original.clone();
    for y in 0..original.height() {
        for x in 0..original.width() {
            if mask.get_pixel(x, y).0[0] > 0 {
                composited.put_pixel(x, y, *predicted.get_pixel(x, y));
            }
        }
    }
    composited
}

#[cfg(test)]
mod tests {
    use super::{resize_keep_aspect_dims, round_up_multiple};

    #[test]
    fn resize_keep_aspect_matches_upstream_rounding() {
        assert_eq!(resize_keep_aspect_dims(1600, 900, 1024), (1024, 576));
        assert_eq!(resize_keep_aspect_dims(900, 1600, 1024), (576, 1024));
    }

    #[test]
    fn round_up_multiple_expands_to_next_valid_shape() {
        assert_eq!(round_up_multiple(1024, 8), 1024);
        assert_eq!(round_up_multiple(1025, 8), 1032);
        assert_eq!(round_up_multiple(7, 8), 8);
    }
}
