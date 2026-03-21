use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use candle_core::{D, DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::paddleocr_vl::{Config as PaddleOcrVlConfig, PaddleOCRVLModel};
use image::{DynamicImage, RgbImage, imageops::FilterType};
use serde::{Deserialize, Serialize};
use tokenizers::Tokenizer;
use tracing::instrument;

use crate::{define_models, device, loading};

const DEFAULT_MAX_NEW_TOKENS: usize = 512;
const SPOTTING_UPSCALE_THRESHOLD: u32 = 1500;

define_models! {
    ConfigJson => ("PaddlePaddle/PaddleOCR-VL-1.5", "config.json"),
    PreprocessorConfigJson => ("PaddlePaddle/PaddleOCR-VL-1.5", "preprocessor_config.json"),
    TokenizerJson => ("PaddlePaddle/PaddleOCR-VL-1.5", "tokenizer.json"),
    Model => ("PaddlePaddle/PaddleOCR-VL-1.5", "model.safetensors"),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaddleOcrVlTask {
    Ocr,
    Table,
    Formula,
    Chart,
    Spotting,
    Seal,
}

impl PaddleOcrVlTask {
    fn prompt(self) -> &'static str {
        match self {
            Self::Ocr => "OCR:",
            Self::Table => "Table Recognition:",
            Self::Formula => "Formula Recognition:",
            Self::Chart => "Chart Recognition:",
            Self::Spotting => "Spotting:",
            Self::Seal => "Seal Recognition:",
        }
    }

    fn max_pixels(self, preprocessor: &PaddleOcrVlPreprocessorConfig) -> usize {
        if matches!(self, Self::Spotting) {
            2048 * preprocessor.factor().pow(2)
        } else {
            preprocessor.max_pixels
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaddleOcrVlOutput {
    pub task: PaddleOcrVlTask,
    pub text: String,
    pub token_ids: Vec<u32>,
    pub original_width: u32,
    pub original_height: u32,
    pub processed_width: u32,
    pub processed_height: u32,
    pub grid_thw: [u32; 3],
    pub num_image_tokens: usize,
    pub upscaled_for_spotting: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaddleOcrVlPreprocessorConfig {
    #[serde(default = "default_true")]
    pub do_convert_rgb: bool,
    #[serde(default = "default_true")]
    pub do_normalize: bool,
    #[serde(default = "default_true")]
    pub do_rescale: bool,
    #[serde(default = "default_true")]
    pub do_resize: bool,
    #[serde(default = "default_image_mean")]
    pub image_mean: [f32; 3],
    #[serde(default = "default_image_std")]
    pub image_std: [f32; 3],
    #[serde(default = "default_max_pixels")]
    pub max_pixels: usize,
    #[serde(default = "default_merge_size")]
    pub merge_size: usize,
    #[serde(default = "default_min_pixels")]
    pub min_pixels: usize,
    #[serde(default = "default_patch_size")]
    pub patch_size: usize,
    #[serde(default = "default_rescale_factor")]
    pub rescale_factor: f32,
    #[serde(default = "default_temporal_patch_size")]
    pub temporal_patch_size: usize,
}

impl PaddleOcrVlPreprocessorConfig {
    const fn factor(&self) -> usize {
        self.patch_size * self.merge_size
    }
}

fn default_true() -> bool {
    true
}

const fn default_patch_size() -> usize {
    14
}

const fn default_merge_size() -> usize {
    2
}

const fn default_temporal_patch_size() -> usize {
    1
}

const fn default_min_pixels() -> usize {
    112_896
}

const fn default_max_pixels() -> usize {
    1_003_520
}

const fn default_rescale_factor() -> f32 {
    1.0 / 255.0
}

const fn default_image_mean() -> [f32; 3] {
    [0.5, 0.5, 0.5]
}

const fn default_image_std() -> [f32; 3] {
    [0.5, 0.5, 0.5]
}

struct ModelFiles {
    config: PathBuf,
    preprocessor: PathBuf,
    tokenizer: PathBuf,
    weights: PathBuf,
}

struct PreparedImage {
    pixel_values: Tensor,
    grid_thw: Tensor,
    processed_width: u32,
    processed_height: u32,
    num_image_tokens: usize,
    upscaled_for_spotting: bool,
}

pub struct PaddleOcrVl {
    model: PaddleOCRVLModel,
    tokenizer: Tokenizer,
    config: PaddleOcrVlConfig,
    preprocessor: PaddleOcrVlPreprocessorConfig,
    device: Device,
    dtype: DType,
    eos_token_id: u32,
}

impl PaddleOcrVl {
    pub async fn load(cpu: bool) -> Result<Self> {
        let files = ModelFiles {
            config: loading::resolve_manifest_path(Manifest::ConfigJson.get()).await?,
            preprocessor: loading::resolve_manifest_path(Manifest::PreprocessorConfigJson.get())
                .await?,
            tokenizer: loading::resolve_manifest_path(Manifest::TokenizerJson.get()).await?,
            weights: loading::resolve_manifest_path(Manifest::Model.get()).await?,
        };
        Self::load_from_files(files, cpu)
    }

    pub fn load_from_dir(dir: impl AsRef<Path>, cpu: bool) -> Result<Self> {
        let dir = dir.as_ref();
        let weights = if dir.join("model.safetensors").exists() {
            dir.join("model.safetensors")
        } else {
            dir.join("pytorch_model.bin")
        };
        Self::load_from_files(
            ModelFiles {
                config: dir.join("config.json"),
                preprocessor: dir.join("preprocessor_config.json"),
                tokenizer: dir.join("tokenizer.json"),
                weights,
            },
            cpu,
        )
    }

    fn load_from_files(files: ModelFiles, cpu: bool) -> Result<Self> {
        let device = device(cpu)?;
        let dtype = if device.is_cuda() {
            DType::BF16
        } else {
            DType::F32
        };
        let config: PaddleOcrVlConfig =
            loading::read_json(&files.config).context("failed to parse model config")?;
        let preprocessor: PaddleOcrVlPreprocessorConfig =
            loading::read_json(&files.preprocessor)
                .context("failed to parse preprocessor config")?;
        let tokenizer = Tokenizer::from_file(&files.tokenizer).map_err(anyhow::Error::msg)?;
        let eos_token_id = resolve_eos_token_id(&tokenizer);
        let vb = if files.weights.extension().is_some_and(|ext| ext == "bin") {
            VarBuilder::from_pth(&files.weights, dtype, &device)?
        } else {
            unsafe {
                VarBuilder::from_mmaped_safetensors(&[files.weights.as_path()], dtype, &device)?
            }
        };
        let model = PaddleOCRVLModel::new(&config, vb)?;
        Ok(Self {
            model,
            tokenizer,
            config,
            preprocessor,
            device,
            dtype,
            eos_token_id,
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(
        &mut self,
        image: &DynamicImage,
        task: PaddleOcrVlTask,
    ) -> Result<PaddleOcrVlOutput> {
        self.inference_with_max_new_tokens(image, task, DEFAULT_MAX_NEW_TOKENS)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_with_max_new_tokens(
        &mut self,
        image: &DynamicImage,
        task: PaddleOcrVlTask,
        max_new_tokens: usize,
    ) -> Result<PaddleOcrVlOutput> {
        let (original_width, original_height) = (image.width(), image.height());
        let prepared = preprocess_image(image, &self.preprocessor, task, self.dtype, &self.device)?;
        let input_ids = build_input_tokens(
            &self.tokenizer,
            task,
            prepared.num_image_tokens,
            self.config.image_token_id,
            self.config.vision_start_token_id,
            self.config.vision_end_token_id,
            &self.device,
        )?;
        let generated = self.model.generate(
            &input_ids,
            &prepared.pixel_values,
            &prepared.grid_thw,
            max_new_tokens,
            self.eos_token_id,
        )?;
        self.build_output(task, original_width, original_height, &prepared, generated)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_images(
        &mut self,
        images: &[DynamicImage],
        task: PaddleOcrVlTask,
        max_new_tokens: usize,
    ) -> Result<Vec<PaddleOcrVlOutput>> {
        if images.is_empty() {
            return Ok(Vec::new());
        }

        let mut prepared_images = Vec::with_capacity(images.len());
        let mut original_sizes = Vec::with_capacity(images.len());
        let mut groups: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();

        for (index, image) in images.iter().enumerate() {
            let prepared =
                preprocess_image(image, &self.preprocessor, task, self.dtype, &self.device)?;
            groups
                .entry((prepared.processed_width, prepared.processed_height))
                .or_default()
                .push(index);
            prepared_images.push(prepared);
            original_sizes.push((image.width(), image.height()));
        }

        let mut outputs = vec![None; images.len()];
        for indices in groups.into_values() {
            let first = prepared_images
                .get(*indices.first().context("empty batch group")?)
                .context("missing prepared image for batch group")?;
            let input_ids = build_batched_input_tokens(
                &self.tokenizer,
                task,
                first.num_image_tokens,
                &self.config,
                indices.len(),
                &self.device,
            )?;
            let pixel_values = cat_batch(
                indices
                    .iter()
                    .map(|&index| &prepared_images[index].pixel_values),
            )?;
            let grid_thw = cat_batch(
                indices
                    .iter()
                    .map(|&index| &prepared_images[index].grid_thw),
            )?;
            let generated_batch =
                self.generate_batch(&input_ids, &pixel_values, &grid_thw, max_new_tokens)?;

            for (batch_index, &image_index) in indices.iter().enumerate() {
                let (original_width, original_height) = original_sizes[image_index];
                outputs[image_index] = Some(
                    self.build_output(
                        task,
                        original_width,
                        original_height,
                        &prepared_images[image_index],
                        generated_batch
                            .get(batch_index)
                            .cloned()
                            .context("missing generated sequence for batch item")?,
                    )?,
                );
            }
        }

        outputs
            .into_iter()
            .map(|output| output.context("missing batch inference output"))
            .collect()
    }

    fn generate_batch(
        &mut self,
        input_ids: &Tensor,
        pixel_values: &Tensor,
        grid_thw: &Tensor,
        max_new_tokens: usize,
    ) -> Result<Vec<Vec<u32>>> {
        let batch_size = input_ids.dim(0)?;
        let mut generated = vec![Vec::new(); batch_size];
        if max_new_tokens == 0 {
            return Ok(generated);
        }

        self.model.clear_kv_cache();

        let mut current_ids = input_ids.clone();
        let logits = self
            .model
            .forward(&current_ids, Some(pixel_values), Some(grid_thw), 0)?;
        let mut next_tokens = logits
            .argmax(D::Minus1)?
            .to_dtype(DType::U32)?
            .to_vec1::<u32>()?;
        let mut finished = vec![false; batch_size];
        for (index, token) in next_tokens.iter().copied().enumerate() {
            generated[index].push(token);
            if token == self.eos_token_id {
                finished[index] = true;
            }
        }
        if finished.iter().all(|&done| done) || max_new_tokens == 1 {
            return Ok(generated);
        }

        let mut seqlen_offset = current_ids.dim(1)?;
        for (token, done) in next_tokens.iter_mut().zip(&finished) {
            if *done {
                *token = self.eos_token_id;
            }
        }
        current_ids = Tensor::new(next_tokens.as_slice(), &self.device)?.unsqueeze(1)?;

        for _ in 1..max_new_tokens {
            let logits = self
                .model
                .forward(&current_ids, None, None, seqlen_offset)?;
            let mut next_tokens = logits
                .argmax(D::Minus1)?
                .to_dtype(DType::U32)?
                .to_vec1::<u32>()?;

            for (index, token) in next_tokens.iter_mut().enumerate() {
                if finished[index] {
                    *token = self.eos_token_id;
                    continue;
                }
                generated[index].push(*token);
                if *token == self.eos_token_id {
                    finished[index] = true;
                }
            }

            if finished.iter().all(|&done| done) {
                break;
            }

            seqlen_offset += 1;
            current_ids = Tensor::new(next_tokens.as_slice(), &self.device)?.unsqueeze(1)?;
        }

        Ok(generated)
    }

    fn build_output(
        &self,
        task: PaddleOcrVlTask,
        original_width: u32,
        original_height: u32,
        prepared: &PreparedImage,
        generated: Vec<u32>,
    ) -> Result<PaddleOcrVlOutput> {
        let token_ids = generated
            .into_iter()
            .take_while(|&token| token != self.eos_token_id)
            .collect::<Vec<_>>();
        let text = self
            .tokenizer
            .decode(&token_ids, true)
            .map_err(anyhow::Error::msg)?
            .trim()
            .to_string();
        let grid_thw = prepared.grid_thw.to_vec2::<u32>()?;
        let grid_thw = grid_thw
            .first()
            .cloned()
            .context("missing image grid in preprocessed output")?;
        Ok(PaddleOcrVlOutput {
            task,
            text,
            token_ids,
            original_width,
            original_height,
            processed_width: prepared.processed_width,
            processed_height: prepared.processed_height,
            grid_thw: [grid_thw[0], grid_thw[1], grid_thw[2]],
            num_image_tokens: prepared.num_image_tokens,
            upscaled_for_spotting: prepared.upscaled_for_spotting,
        })
    }
}

fn resolve_eos_token_id(tokenizer: &Tokenizer) -> u32 {
    tokenizer
        .token_to_id("</s>")
        .or_else(|| tokenizer.token_to_id("<|end_of_sentence|>"))
        .or_else(|| tokenizer.token_to_id("<|endoftext|>"))
        .unwrap_or(2)
}

fn smart_resize(
    height: usize,
    width: usize,
    factor: usize,
    min_pixels: usize,
    max_pixels: usize,
) -> Result<(usize, usize)> {
    let mut height = height;
    let mut width = width;
    if height < factor {
        width = (width * factor + height / 2) / height;
        height = factor;
    }
    if width < factor {
        height = (height * factor + width / 2) / width;
        width = factor;
    }
    if (height.max(width) as f64 / height.min(width) as f64) > 200.0 {
        bail!("absolute aspect ratio must be smaller than 200");
    }
    let mut resized_height = ((height + factor / 2) / factor) * factor;
    let mut resized_width = ((width + factor / 2) / factor) * factor;
    let total_pixels = resized_height * resized_width;
    if total_pixels > max_pixels {
        let beta = ((height * width) as f64 / max_pixels as f64).sqrt();
        resized_height = ((height as f64 / beta / factor as f64).floor() as usize) * factor;
        resized_width = ((width as f64 / beta / factor as f64).floor() as usize) * factor;
    } else if total_pixels < min_pixels {
        let beta = (min_pixels as f64 / (height * width) as f64).sqrt();
        resized_height = ((height as f64 * beta / factor as f64).ceil() as usize) * factor;
        resized_width = ((width as f64 * beta / factor as f64).ceil() as usize) * factor;
    }
    Ok((resized_height, resized_width))
}

fn preprocess_image(
    image: &DynamicImage,
    preprocessor: &PaddleOcrVlPreprocessorConfig,
    task: PaddleOcrVlTask,
    dtype: DType,
    device: &Device,
) -> Result<PreparedImage> {
    if preprocessor.temporal_patch_size != 1 {
        bail!(
            "temporal_patch_size {} is not supported for image inference",
            preprocessor.temporal_patch_size
        );
    }

    let mut image = if preprocessor.do_convert_rgb {
        DynamicImage::ImageRgb8(image.to_rgb8())
    } else {
        image.clone()
    };
    let mut upscaled_for_spotting = false;
    if matches!(task, PaddleOcrVlTask::Spotting)
        && image.width() < SPOTTING_UPSCALE_THRESHOLD
        && image.height() < SPOTTING_UPSCALE_THRESHOLD
    {
        image = DynamicImage::ImageRgb8(image::imageops::resize(
            &image.to_rgb8(),
            image.width() * 2,
            image.height() * 2,
            FilterType::Lanczos3,
        ));
        upscaled_for_spotting = true;
    }

    let resized = if preprocessor.do_resize {
        let (new_height, new_width) = smart_resize(
            image.height() as usize,
            image.width() as usize,
            preprocessor.factor(),
            preprocessor.min_pixels,
            task.max_pixels(preprocessor),
        )?;
        DynamicImage::ImageRgb8(image::imageops::resize(
            &image.to_rgb8(),
            new_width as u32,
            new_height as u32,
            FilterType::CatmullRom,
        ))
    } else {
        image
    };

    let rgb = resized.to_rgb8();
    let processed_width = rgb.width();
    let processed_height = rgb.height();
    let pixel_values = rgb_to_tensor(&rgb, preprocessor, dtype, device)?;
    let grid_h = processed_height as usize / preprocessor.patch_size;
    let grid_w = processed_width as usize / preprocessor.patch_size;
    let grid_thw = Tensor::new(&[[1u32, grid_h as u32, grid_w as u32]], device)?;
    let num_image_tokens = (grid_h / preprocessor.merge_size) * (grid_w / preprocessor.merge_size);

    Ok(PreparedImage {
        pixel_values,
        grid_thw,
        processed_width,
        processed_height,
        num_image_tokens,
        upscaled_for_spotting,
    })
}

fn rgb_to_tensor(
    image: &RgbImage,
    preprocessor: &PaddleOcrVlPreprocessorConfig,
    dtype: DType,
    device: &Device,
) -> Result<Tensor> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let mut values = vec![0f32; 3 * height * width];
    for c in 0..3 {
        let mean = preprocessor.image_mean[c];
        let std = if preprocessor.image_std[c] == 0.0 {
            1.0
        } else {
            preprocessor.image_std[c]
        };
        for y in 0..height {
            for x in 0..width {
                let pixel = image.get_pixel(x as u32, y as u32);
                let mut value = pixel[c] as f32;
                if preprocessor.do_rescale {
                    value *= preprocessor.rescale_factor;
                }
                if preprocessor.do_normalize {
                    value = (value - mean) / std;
                }
                values[c * height * width + y * width + x] = value;
            }
        }
    }
    Ok(Tensor::from_vec(values, (1, 3, height, width), device)?.to_dtype(dtype)?)
}

fn build_input_tokens(
    tokenizer: &Tokenizer,
    task: PaddleOcrVlTask,
    num_image_tokens: usize,
    image_token_id: u32,
    vision_start_token_id: u32,
    vision_end_token_id: u32,
    device: &Device,
) -> Result<Tensor> {
    let bos_token_id = tokenizer.token_to_id("<|begin_of_sentence|>").unwrap_or(1);
    let user_encoding = tokenizer
        .encode("User: ", false)
        .map_err(anyhow::Error::msg)?;
    let task_encoding = tokenizer
        .encode(task.prompt(), false)
        .map_err(anyhow::Error::msg)?;
    let assistant_encoding = tokenizer
        .encode("\nAssistant: ", false)
        .map_err(anyhow::Error::msg)?;

    let mut input_ids = vec![bos_token_id];
    input_ids.extend(user_encoding.get_ids());
    input_ids.push(vision_start_token_id);
    input_ids.extend(std::iter::repeat_n(image_token_id, num_image_tokens));
    input_ids.push(vision_end_token_id);
    input_ids.extend(task_encoding.get_ids());
    input_ids.extend(assistant_encoding.get_ids());

    Ok(Tensor::new(input_ids.as_slice(), device)?.unsqueeze(0)?)
}

fn build_batched_input_tokens(
    tokenizer: &Tokenizer,
    task: PaddleOcrVlTask,
    num_image_tokens: usize,
    config: &PaddleOcrVlConfig,
    batch_size: usize,
    device: &Device,
) -> Result<Tensor> {
    let input_ids = build_input_tokens(
        tokenizer,
        task,
        num_image_tokens,
        config.image_token_id,
        config.vision_start_token_id,
        config.vision_end_token_id,
        device,
    )?
    .squeeze(0)?
    .to_vec1::<u32>()?;

    let mut batched_ids = Vec::with_capacity(batch_size * input_ids.len());
    for _ in 0..batch_size {
        batched_ids.extend_from_slice(&input_ids);
    }
    Ok(Tensor::new(batched_ids.as_slice(), device)?.reshape((batch_size, input_ids.len()))?)
}

fn cat_batch<'a>(tensors: impl IntoIterator<Item = &'a Tensor>) -> Result<Tensor> {
    let tensors = tensors.into_iter().collect::<Vec<_>>();
    Tensor::cat(&tensors, 0).map_err(Into::into)
}
