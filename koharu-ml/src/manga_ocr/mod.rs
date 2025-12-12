mod bert;
mod model;
mod tokenizer;

use std::path::Path;

use anyhow::{Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use image::GenericImageView;
use serde::de::DeserializeOwned;
use tokenizers::Tokenizer;
use tracing::instrument;

use model::{PreprocessorConfig, VisionEncoderDecoder, VisionEncoderDecoderConfig};
use tokenizer::load_tokenizer;

use crate::define_models;

define_models! {
    Config => ("mayocream/manga-ocr", "config.json"),
    PreprocessorConfig => ("mayocream/manga-ocr", "preprocessor_config.json"),
    Vocab => ("mayocream/manga-ocr", "vocab.txt"),
    SpecialTokensMap => ("mayocream/manga-ocr", "special_tokens_map.json"),
    Model => ("mayocream/manga-ocr", "model.safetensors"),
}

pub struct MangaOcr {
    model: VisionEncoderDecoder,
    tokenizer: Tokenizer,
    preprocessor: PreprocessorConfig,
    device: Device,
}

impl MangaOcr {
    pub async fn load(device: Device) -> Result<Self> {
        let config_path = Manifest::Config.get().await?;
        let preprocessor_path = Manifest::PreprocessorConfig.get().await?;
        let vocab_path = Manifest::Vocab.get().await?;
        let special_tokens_path = Manifest::SpecialTokensMap.get().await?;
        let weights_path = Manifest::Model.get().await?;

        let config: VisionEncoderDecoderConfig =
            load_json(&config_path).context("failed to parse model config")?;
        let preprocessor: PreprocessorConfig =
            load_json(&preprocessor_path).context("failed to parse preprocessor config")?;
        let tokenizer = load_tokenizer(None, &vocab_path, &special_tokens_path)?;
        let vb =
            unsafe { VarBuilder::from_mmaped_safetensors(&[weights_path], DType::F32, &device)? };
        let model = VisionEncoderDecoder::from_config(config, vb, device.clone())?;

        Ok(Self {
            model,
            tokenizer,
            preprocessor,
            device,
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(&self, images: &[image::DynamicImage]) -> Result<Vec<String>> {
        if images.is_empty() {
            return Ok(Vec::new());
        }

        let pixel_values = preprocess_images(
            images,
            self.preprocessor.size,
            &self.preprocessor.image_mean,
            &self.preprocessor.image_std,
            self.preprocessor.do_resize,
            self.preprocessor.do_normalize,
            &self.device,
        )?;
        let token_ids = self.forward(&pixel_values)?;
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
    fn forward(&self, pixel_values: &Tensor) -> Result<Vec<Vec<u32>>> {
        self.model.forward(pixel_values)
    }
}

fn load_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str(&data)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(parsed)
}

#[instrument(level = "debug", skip_all)]
fn preprocess_images(
    images: &[image::DynamicImage],
    image_size: u32,
    image_mean: &[f32; 3],
    image_std: &[f32; 3],
    do_resize: bool,
    do_normalize: bool,
    device: &Device,
) -> Result<Tensor> {
    let mut batch = Vec::with_capacity(images.len());
    for image in images {
        let processed = preprocess_single_image(
            image,
            image_size,
            image_mean,
            image_std,
            do_resize,
            do_normalize,
            device,
        )?;
        batch.push(processed);
    }

    Ok(Tensor::cat(&batch, 0)?)
}

#[instrument(level = "debug", skip_all)]
fn preprocess_single_image(
    image: &image::DynamicImage,
    image_size: u32,
    image_mean: &[f32; 3],
    image_std: &[f32; 3],
    do_resize: bool,
    do_normalize: bool,
    device: &Device,
) -> Result<Tensor> {
    let (orig_w, orig_h) = image.dimensions();
    let (width, height) = if do_resize {
        (image_size as usize, image_size as usize)
    } else {
        (orig_w as usize, orig_h as usize)
    };

    let tensor = Tensor::from_vec(
        image.grayscale().to_rgb8().into_raw(),
        (1, orig_h as usize, orig_w as usize, 3),
        device,
    )?
    .permute((0, 3, 1, 2))?
    .to_dtype(DType::F32)?;

    let tensor = if do_resize {
        tensor.interpolate2d(height, width)?
    } else {
        tensor
    };

    let tensor = (tensor * (1.0 / 255.0))?;
    let tensor = if do_normalize {
        let std = [
            if image_std[0] == 0.0 {
                1.0
            } else {
                image_std[0]
            },
            if image_std[1] == 0.0 {
                1.0
            } else {
                image_std[1]
            },
            if image_std[2] == 0.0 {
                1.0
            } else {
                image_std[2]
            },
        ];
        let mean_t = Tensor::from_slice(image_mean, (1, 3, 1, 1), device)?;
        let std_t = Tensor::from_slice(&std, (1, 3, 1, 1), device)?;
        tensor.broadcast_sub(&mean_t)?.broadcast_div(&std_t)?
    } else {
        tensor
    };

    Ok(tensor)
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
