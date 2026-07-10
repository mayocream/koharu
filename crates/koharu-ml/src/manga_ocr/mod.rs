mod bert;
mod model;
mod tokenizer;

use anyhow::{Context, Result};
use candle_core::{DType, Device, Tensor};
use image::GenericImageView;
use koharu_runtime::RuntimeManager;
use tokenizers::Tokenizer;
use tracing::instrument;

use model::{PreprocessorConfig, VisionEncoderDecoder, VisionEncoderDecoderConfig};
use tokenizer::load_tokenizer;

use crate::{device, loading};

const HF_REPO: &str = "mayocream/manga-ocr";

koharu_runtime::declare_hf_model_package!(id: "model:manga-ocr:config", repo: HF_REPO, file: "config.json", bootstrap: false, order: 200);
koharu_runtime::declare_hf_model_package!(id: "model:manga-ocr:preprocessor", repo: HF_REPO, file: "preprocessor_config.json", bootstrap: false, order: 201);
koharu_runtime::declare_hf_model_package!(id: "model:manga-ocr:vocab", repo: HF_REPO, file: "vocab.txt", bootstrap: false, order: 202);
koharu_runtime::declare_hf_model_package!(id: "model:manga-ocr:special-tokens", repo: HF_REPO, file: "special_tokens_map.json", bootstrap: false, order: 203);
koharu_runtime::declare_hf_model_package!(id: "model:manga-ocr:weights", repo: HF_REPO, file: "model.safetensors", bootstrap: false, order: 204);

pub struct MangaOcr {
    model: VisionEncoderDecoder,
    tokenizer: Tokenizer,
    preprocessor: PreprocessorConfig,
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
        let device = device(cpu)?;
        let dtype = loading::model_dtype(&device);
        let hf = runtime.downloads();
        let config_path = hf.huggingface_model(HF_REPO, "config.json").await?;
        let preprocessor_path = hf
            .huggingface_model(HF_REPO, "preprocessor_config.json")
            .await?;
        let vocab_path = hf.huggingface_model(HF_REPO, "vocab.txt").await?;
        let special_tokens_path = hf
            .huggingface_model(HF_REPO, "special_tokens_map.json")
            .await?;

        let config: VisionEncoderDecoderConfig =
            loading::read_json(&config_path).context("failed to parse model config")?;
        let preprocessor: PreprocessorConfig = loading::read_json(&preprocessor_path)
            .context("failed to parse preprocessor config")?;
        let tokenizer = load_tokenizer(None, &vocab_path, &special_tokens_path)?;
        let model_device = device.clone();
        let weights = hf.huggingface_model(HF_REPO, "model.safetensors").await?;
        let model = loading::load_mmaped_safetensors_path_with_dtype(
            &weights,
            &device,
            dtype,
            move |vb| VisionEncoderDecoder::from_config(config, vb, model_device.clone()),
        )?;

        Ok(Self {
            model,
            tokenizer,
            preprocessor,
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
            image_size: self.preprocessor.size,
            image_mean: &self.preprocessor.image_mean,
            image_std: &self.preprocessor.image_std,
            do_resize: self.preprocessor.do_resize,
            do_normalize: self.preprocessor.do_normalize,
            device: &self.device,
            dtype: self.dtype,
        };
        let pixel_values = preprocess_images(images, &options)?;
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

#[instrument(level = "debug", skip_all)]
fn preprocess_images(
    images: &[image::DynamicImage],
    options: &ImagePreprocessOptions<'_>,
) -> Result<Tensor> {
    let mut batch = Vec::with_capacity(images.len());
    for image in images {
        let processed = preprocess_single_image(image, options)?;
        batch.push(processed);
    }

    Ok(Tensor::cat(&batch, 0)?)
}

#[instrument(level = "debug", skip_all)]
fn preprocess_single_image(
    image: &image::DynamicImage,
    options: &ImagePreprocessOptions<'_>,
) -> Result<Tensor> {
    let (orig_w, orig_h) = image.dimensions();
    let (width, height) = if options.do_resize {
        (options.image_size as usize, options.image_size as usize)
    } else {
        (orig_w as usize, orig_h as usize)
    };

    let tensor = Tensor::from_vec(
        image.grayscale().to_rgb8().into_raw(),
        (1, orig_h as usize, orig_w as usize, 3),
        options.device,
    )?
    .permute((0, 3, 1, 2))?
    .to_dtype(DType::F32)?;

    let tensor = if options.do_resize {
        tensor.interpolate2d(height, width)?
    } else {
        tensor
    };

    let tensor = (tensor * (1.0 / 255.0))?;
    let tensor = if options.do_normalize {
        let std = [
            if options.image_std[0] == 0.0 {
                1.0
            } else {
                options.image_std[0]
            },
            if options.image_std[1] == 0.0 {
                1.0
            } else {
                options.image_std[1]
            },
            if options.image_std[2] == 0.0 {
                1.0
            } else {
                options.image_std[2]
            },
        ];
        let mean_t = Tensor::from_slice(options.image_mean, (1, 3, 1, 1), options.device)?;
        let std_t = Tensor::from_slice(&std, (1, 3, 1, 1), options.device)?;
        tensor.broadcast_sub(&mean_t)?.broadcast_div(&std_t)?
    } else {
        tensor
    };

    Ok(tensor.to_dtype(options.dtype)?)
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
