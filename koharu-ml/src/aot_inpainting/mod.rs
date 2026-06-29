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

use crate::inpainting::{
    HdStrategyConfig, InpaintForward, apply_bubble_fill, binarize_mask, extract_alpha,
    restore_alpha_channel, run_inpaint,
};

use self::model::AotGenerator;

const HF_REPO: &str = "mayocream/aot-inpainting";
const SAFETENSORS_FILENAME: &str = "model.safetensors";
const PAD_MULTIPLE: u32 = 8;
const DEFAULT_MAX_SIDE: u32 = 1024;

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
    device: Device,
    dtype: DType,
}

impl AotInpainting {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let weights_path = resolve_model_path(runtime).await?;
        Self::load_from_weights_path(&weights_path, cpu)
    }

    pub fn load_from_paths(
        _config_path: impl AsRef<Path>,
        weights_path: impl AsRef<Path>,
        cpu: bool,
    ) -> Result<Self> {
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

    pub fn default_config(&self) -> HdStrategyConfig {
        HdStrategyConfig::aot_default(DEFAULT_MAX_SIDE, PAD_MULTIPLE)
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

    fn forward_rgb(&self, image: &RgbImage, mask: &GrayImage) -> Result<RgbImage> {
        let (w, h) = image.dimensions();
        let (width, height) = (w as usize, h as usize);
        let plane = width * height;
        let rgb = image.as_raw();
        let luma = mask.as_raw();
        let mut image_data = vec![0.0_f32; 3 * plane];
        let mut mask_data = vec![0.0_f32; plane];

        for index in 0..plane {
            let mask_value = luma[index] as f32 / 255.0;
            mask_data[index] = mask_value;
            let inv = 1.0 - mask_value;
            for channel in 0..3 {
                let value = rgb[index * 3 + channel] as f32 / 127.5 - 1.0;
                image_data[channel * plane + index] = value * inv;
            }
        }

        let mut image_tensor_data = TensorData::new(image_data, [1, 3, height, width]);
        let mut mask_tensor_data = TensorData::new(mask_data, [1, 1, height, width]);
        self.device
            .staging([&mut image_tensor_data, &mut mask_tensor_data].into_iter());
        let image_tensor = Tensor::<4>::from_data(image_tensor_data, (&self.device, self.dtype));
        let mask_tensor = Tensor::<4>::from_data(mask_tensor_data, (&self.device, self.dtype));

        let output = self.model.forward(image_tensor, mask_tensor);
        self.postprocess(output)
    }

    fn postprocess(&self, output: Tensor<4>) -> Result<RgbImage> {
        let output = output
            .cast(FloatDType::F32)
            .squeeze_dim::<3>(0)
            .permute([1, 2, 0]);
        let [height, width, channels] = output.dims();
        if channels != 3 {
            bail!("expected 3 output channels, got {channels}");
        }

        let raw = tensor_to_f32_vec(((output + 1.0) * 127.5).clamp(0.0, 255.0))?
            .into_iter()
            .map(|value| value.round().clamp(0.0, 255.0) as u8)
            .collect::<Vec<_>>();
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

fn load_model(path: &Path, device: &Device, module_dtype: FloatDType) -> Result<AotGenerator> {
    let mut model = AotGenerator::new(device);
    let mut store = SafetensorsStore::from_file(path)
        .with_from_adapter(PyTorchToBurnAdapter)
        .with_key_remapping(r"^head\.0\.", "head0.")
        .with_key_remapping(r"^head\.2\.", "head1.")
        .with_key_remapping(r"^head\.4\.", "head2.")
        .with_key_remapping(r"^body_conv\.", "body.")
        .with_key_remapping(r"\.block(0[0-3])\.1\.", ".block$1.conv.")
        .with_key_remapping(r"\.fuse\.1\.", ".fuse.conv.")
        .with_key_remapping(r"\.gate\.1\.", ".gate.conv.")
        .with_key_remapping(r"^tail\.0\.", "tail0.")
        .with_key_remapping(r"^tail\.2\.", "tail1.")
        .with_key_remapping(r"^tail\.4\.", "up0.")
        .with_key_remapping(r"^tail\.6\.", "up1.")
        .with_key_remapping(r"^tail\.8\.", "output.")
        .skip_enum_variants(true)
        .allow_partial(false);
    let result = model
        .load_from(&mut store)
        .context("failed to mmap/load AOT inpainting safetensors through Burn store")?;
    if !result.errors.is_empty() {
        bail!("failed to load AOT inpainting tensors: {}", result);
    }
    if !result.missing.is_empty() {
        bail!("AOT inpainting checkpoint is missing tensors: {}", result);
    }

    Ok(cast_module_float(model.into_inference(), module_dtype))
}

fn make_device(cpu: bool) -> (Device, DType, FloatDType) {
    #[cfg(feature = "cuda")]
    {
        if !cpu {
            let mut device = Device::cuda(0);
            if let Err(error) = device.configure(FloatDType::BF16) {
                tracing::warn!(%error, "failed to configure Burn CUDA default dtype to BF16");
            }
            return (device, DType::BF16, FloatDType::BF16);
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
