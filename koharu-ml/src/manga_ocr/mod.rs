mod bert;
mod model;
mod tokenizer;

use std::path::Path;

use anyhow::{Context, Result, bail};
use burn::{
    module::{Module, ModuleMapper, Param},
    store::{ModuleSnapshot, PyTorchToBurnAdapter, SafetensorsStore},
    tensor::{DType, Device, DeviceKind, FloatDType, Tensor, TensorData},
};
use image::{
    DynamicImage,
    imageops::{self, FilterType},
};
use koharu_runtime::RuntimeManager;
use tokenizers::Tokenizer;
use tracing::instrument;

use model::VisionEncoderDecoder;
use tokenizer::load_tokenizer;

const HF_REPO: &str = "mayocream/manga-ocr";
const SAFETENSORS_FILENAME: &str = "model.safetensors";
const VOCAB_FILENAME: &str = "vocab.txt";
const IMAGE_SIZE: u32 = 224;
const IMAGE_MEAN: [f32; 3] = [0.5, 0.5, 0.5];
const IMAGE_STD: [f32; 3] = [0.5, 0.5, 0.5];

koharu_runtime::declare_hf_model_package!(
    id: "model:manga-ocr:vocab",
    repo: HF_REPO,
    file: VOCAB_FILENAME,
    bootstrap: false,
    order: 202,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:manga-ocr:weights",
    repo: HF_REPO,
    file: SAFETENSORS_FILENAME,
    bootstrap: false,
    order: 204,
);

pub struct MangaOcr {
    model: VisionEncoderDecoder,
    tokenizer: Tokenizer,
    device: Device,
    dtype: DType,
}

struct ImagePreprocessOptions<'a> {
    image_size: u32,
    image_mean: &'a [f32; 3],
    image_std: &'a [f32; 3],
    do_resize: bool,
    do_normalize: bool,
    device: &'a Device,
    dtype: DType,
}

impl MangaOcr {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let (device, dtype, module_dtype) = make_device(cpu);
        let hf = runtime.downloads();
        let vocab_path = hf.huggingface_model(HF_REPO, VOCAB_FILENAME).await?;
        let tokenizer = load_tokenizer(None, &vocab_path)?;
        let weights_path = hf.huggingface_model(HF_REPO, SAFETENSORS_FILENAME).await?;
        let model = load_model(&weights_path, &device, module_dtype)?;

        Ok(Self {
            model,
            tokenizer,
            device,
            dtype,
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(&self, images: &[image::DynamicImage]) -> Result<Vec<String>> {
        if images.is_empty() {
            return Ok(Vec::new());
        }

        let options = ImagePreprocessOptions {
            image_size: IMAGE_SIZE,
            image_mean: &IMAGE_MEAN,
            image_std: &IMAGE_STD,
            do_resize: true,
            do_normalize: true,
            device: &self.device,
            dtype: self.dtype,
        };
        let pixel_values = preprocess_images(images, &options)?;
        let token_ids = self.forward(pixel_values)?;
        let texts = token_ids
            .into_iter()
            .map(|ids| {
                let text = self.tokenizer.decode(&ids, true).unwrap_or_default();
                post_process(&text)
            })
            .collect();
        Ok(texts)
    }

    #[instrument(level = "debug", skip_all)]
    fn forward(&self, pixel_values: Tensor<4>) -> Result<Vec<Vec<u32>>> {
        self.model.forward(pixel_values)
    }
}

#[instrument(level = "debug", skip_all)]
fn preprocess_images(
    images: &[DynamicImage],
    options: &ImagePreprocessOptions<'_>,
) -> Result<Tensor<4>> {
    let image_size = options.image_size as usize;
    let plane = image_size * image_size;
    let mut data = Vec::with_capacity(images.len() * 3 * plane);
    for image in images {
        data.extend(preprocess_single_image(image, options)?);
    }

    let mut tensor_data = TensorData::new(data, [images.len(), 3, image_size, image_size]);
    options.device.staging(std::iter::once(&mut tensor_data));
    Ok(Tensor::from_data(
        tensor_data,
        (options.device, options.dtype),
    ))
}

#[instrument(level = "debug", skip_all)]
fn preprocess_single_image(
    image: &DynamicImage,
    options: &ImagePreprocessOptions<'_>,
) -> Result<Vec<f32>> {
    let rgb = image.grayscale().to_rgb8();
    let (orig_w, orig_h) = rgb.dimensions();
    let target = options.image_size;
    let rgb = if options.do_resize && (orig_w != target || orig_h != target) {
        imageops::resize(&rgb, target, target, FilterType::Triangle)
    } else {
        rgb
    };
    let (width, height) = rgb.dimensions();
    let width = width as usize;
    let height = height as usize;
    let plane = width * height;
    let raw = rgb.into_raw();
    let mut data = vec![0.0_f32; 3 * plane];
    let std = [
        options.image_std[0].max(f32::EPSILON),
        options.image_std[1].max(f32::EPSILON),
        options.image_std[2].max(f32::EPSILON),
    ];
    for (index, pixel) in raw.chunks_exact(3).enumerate() {
        for channel in 0..3 {
            let mut value = pixel[channel] as f32 / 255.0;
            if options.do_normalize {
                value = (value - options.image_mean[channel]) / std[channel];
            }
            data[channel * plane + index] = value;
        }
    }
    Ok(data)
}

fn load_model(
    path: &Path,
    device: &Device,
    module_dtype: FloatDType,
) -> Result<VisionEncoderDecoder> {
    let mut model = VisionEncoderDecoder::new(device);
    let mut store = SafetensorsStore::from_file(path)
        .with_from_adapter(PyTorchToBurnAdapter)
        .with_key_remapping(r"\.LayerNorm\.", ".layer_norm.")
        .with_key_remapping(r"\.attention\.self\.", ".attention.self_attention.")
        .with_key_remapping(
            r"\.crossattention\.self\.",
            ".crossattention.self_attention.",
        )
        .skip_enum_variants(true)
        .allow_partial(false);
    let result = model
        .load_from(&mut store)
        .context("failed to mmap/load Manga OCR safetensors through Burn store")?;
    if !result.errors.is_empty() {
        bail!("failed to load Manga OCR tensors: {}", result);
    }
    if !result.missing.is_empty() {
        bail!("Manga OCR checkpoint is missing tensors: {}", result);
    }
    Ok(cast_module_float(model, module_dtype))
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

#[instrument(level = "debug", skip_all)]
fn post_process(text: &str) -> String {
    let mut clean = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    clean = clean.replace('\u{2026}', "...");
    clean = collapse_dots(&clean);
    halfwidth_to_fullwidth(&clean)
}

fn collapse_dots(text: &str) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    for ch in text.chars() {
        if ch == '.' || ch == '\u{30fb}' {
            count += 1;
        } else {
            if count > 0 {
                for _ in 0..count {
                    out.push('.');
                }
                count = 0;
            }
            out.push(ch);
        }
    }
    if count > 0 {
        for _ in 0..count {
            out.push('.');
        }
    }
    out
}

fn halfwidth_to_fullwidth(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '!'..='~' => char::from_u32(ch as u32 + 0xFEE0).unwrap_or(ch),
            ' ' => '\u{3000}',
            _ => ch,
        })
        .collect()
}
