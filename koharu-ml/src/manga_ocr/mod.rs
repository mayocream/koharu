mod bert;
mod model;
mod tokenizer;

use std::path::Path;

use anyhow::{Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use serde::de::DeserializeOwned;
use tokenizers::Tokenizer;

use crate::hf_hub;
use model::{PreprocessorConfig, VisionEncoderDecoder, VisionEncoderDecoderConfig};
use tokenizer::load_tokenizer;

pub struct MangaOcr {
    model: VisionEncoderDecoder,
    tokenizer: Tokenizer,
    preprocessor: PreprocessorConfig,
    device: Device,
}

impl MangaOcr {
    pub async fn load(device: Device) -> Result<Self> {
        let config_path = hf_hub("mayocream/manga-ocr", "config.json").await?;
        let preprocessor_path = hf_hub("mayocream/manga-ocr", "preprocessor_config.json").await?;
        let vocab_path = hf_hub("mayocream/manga-ocr", "vocab.txt").await?;
        let special_tokens_path = hf_hub("mayocream/manga-ocr", "special_tokens_map.json").await?;
        let tokenizer_json = hf_hub("mayocream/manga-ocr", "tokenizer.json").await.ok();
        let weights_path = hf_hub("mayocream/manga-ocr", "model.safetensors").await?;

        let config: VisionEncoderDecoderConfig =
            load_json(&config_path).context("failed to parse model config")?;
        let preprocessor: PreprocessorConfig =
            load_json(&preprocessor_path).context("failed to parse preprocessor config")?;
        let tokenizer =
            load_tokenizer(tokenizer_json.as_deref(), &vocab_path, &special_tokens_path)?;
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

    pub fn inference(&self, image: &image::DynamicImage) -> Result<String> {
        let pixel_values = preprocess_image(
            image,
            self.preprocessor.size,
            &self.preprocessor.image_mean,
            &self.preprocessor.image_std,
            self.preprocessor.do_resize,
            self.preprocessor.do_normalize,
            &self.device,
        )?;
        let token_ids = self.forward(&pixel_values)?;
        let text = self.tokenizer.decode(&token_ids, true).unwrap_or_default();
        Ok(post_process(&text))
    }

    fn forward(&self, pixel_values: &Tensor) -> Result<Vec<u32>> {
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

fn preprocess_image(
    image: &image::DynamicImage,
    image_size: u32,
    image_mean: &[f32; 3],
    image_std: &[f32; 3],
    do_resize: bool,
    do_normalize: bool,
    device: &Device,
) -> Result<Tensor> {
    let gray = image.grayscale().to_rgb8();
    let resized = if do_resize {
        image::imageops::resize(
            &gray,
            image_size,
            image_size,
            image::imageops::FilterType::Triangle,
        )
    } else {
        gray
    };
    let (width, height) = resized.dimensions();
    let mut data = Vec::with_capacity((3 * width * height) as usize);
    for c in 0..3 {
        for pixel in resized.pixels() {
            let std = if image_std[c] == 0.0 {
                1.0
            } else {
                image_std[c]
            };
            let value = if do_normalize {
                (pixel[c] as f32 / 255.0 - image_mean[c]) / std
            } else {
                pixel[c] as f32 / 255.0
            };
            data.push(value);
        }
    }
    Ok(Tensor::from_vec(
        data,
        (1, 3, height as usize, width as usize),
        device,
    )?)
}

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
