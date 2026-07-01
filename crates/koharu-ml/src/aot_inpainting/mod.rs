mod model;

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result, bail};
use candle_core::{DType, Device, Tensor};
use image::{DynamicImage, GenericImageView, GrayImage, RgbImage};
use koharu_runtime::RuntimeManager;
use serde::Deserialize;
use tracing::instrument;

use crate::{
    device,
    inpainting::{
        HdStrategyConfig, InpaintForward, apply_bubble_fill, binarize_mask, extract_alpha,
        restore_alpha_channel, run_inpaint,
    },
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
    dtype: DType,
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
        let dtype = loading::model_dtype(&device);
        let config = loading::read_json::<AotInpaintingConfig>(config_path.as_ref())
            .with_context(|| format!("failed to parse {}", config_path.as_ref().display()))?;
        config.validate()?;
        let model = loading::load_mmaped_safetensors_path_with_dtype(
            weights_path.as_ref(),
            &device,
            dtype,
            |vb| AotGenerator::load(&vb, &config.spec()),
        )?;

        Ok(Self {
            model,
            config,
            device,
            dtype,
        })
    }

    /// Default strategy: Resize, using the model's shipped `default_max_side`
    /// as the resize limit. Matches pre-refactor behaviour.
    pub fn default_config(&self) -> HdStrategyConfig {
        HdStrategyConfig::aot_default(
            self.config.default_max_side,
            self.config.pad_multiple as u32,
        )
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        bubble_mask: &DynamicImage,
    ) -> Result<DynamicImage> {
        self.inference_with_config(image, mask, bubble_mask, &self.default_config())
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_with_config(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        bubble_mask: &DynamicImage,
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
        let forward = AotForward { aot: self };
        let output_rgb = run_inpaint(&forward, &image_rgb, &binary_mask, Some(&bubble_mask), cfg)?;

        tracing::info!(
            width = image.width(),
            height = image.height(),
            resize_limit = cfg.resize_limit,
            total_ms = started.elapsed().as_millis(),
            "aot inpainting timings"
        );

        if image.color().has_alpha() {
            let alpha = extract_alpha(&image.to_rgba8());
            let rgba = restore_alpha_channel(&output_rgb, &alpha, &binary_mask);
            Ok(DynamicImage::ImageRgba8(rgba))
        } else {
            Ok(DynamicImage::ImageRgb8(output_rgb))
        }
    }

    /// Raw model forward on a pre-padded RGB image + mask. Input spatial dims
    /// must already be multiples of `pad_multiple` — the HD-strategy dispatcher
    /// handles this.
    fn forward_rgb(&self, image: &RgbImage, mask: &GrayImage) -> Result<RgbImage> {
        let (w, h) = image.dimensions();
        let image_tensor = (Tensor::from_vec(
            image.clone().into_raw(),
            (1, h as usize, w as usize, 3),
            &self.device,
        )?
        .permute((0, 3, 1, 2))?
        .to_dtype(self.dtype)?
            / 127.5)?;
        let image_tensor = (image_tensor - 1.0)?;

        let mask_tensor = Tensor::from_vec(
            mask.clone().into_raw(),
            (1, h as usize, w as usize, 1),
            &self.device,
        )?
        .permute((0, 3, 1, 2))?
        .to_dtype(self.dtype)?;
        let mask_tensor = (mask_tensor / 255.0)?;
        let mask_inv = (Tensor::ones_like(&mask_tensor)? - &mask_tensor)?;
        let mask_inv_rgb = mask_inv.broadcast_as((1, 3, h as usize, w as usize))?;
        let masked_image = (&image_tensor * &mask_inv_rgb)?;

        let output = self.model.forward(&masked_image, &mask_tensor)?;
        self.postprocess(&output)
    }

    fn postprocess(&self, output: &Tensor) -> Result<RgbImage> {
        let output = output
            .to_dtype(DType::F32)?
            .to_device(&Device::Cpu)?
            .squeeze(0)?;
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
        RgbImage::from_raw(width as u32, height as u32, raw)
            .ok_or_else(|| anyhow::anyhow!("failed to create image buffer from model output"))
    }
}

struct AotForward<'a> {
    aot: &'a AotInpainting,
}

impl InpaintForward for AotForward<'_> {
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
                "aot bubble fill fast path"
            );
            (filled.image, filled.remaining_mask)
        } else {
            (image.clone(), mask.clone())
        };

        if mask.pixels().all(|p| p.0[0] == 0) {
            return Ok(image);
        }
        self.aot.forward_rgb(&image, &mask)
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
