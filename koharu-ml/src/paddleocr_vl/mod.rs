use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use candle_core::{D, DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::paddleocr_vl::{Config as PaddleOcrVlConfig, PaddleOCRVLModel};
use image::{DynamicImage, RgbImage, imageops::FilterType};
use serde::{Deserialize, Serialize};
use tokenizers::Tokenizer;
use tracing::instrument;

use crate::{define_models, device, loading};

const DEFAULT_MAX_NEW_TOKENS: usize = 128;
const SPOTTING_UPSCALE_THRESHOLD: u32 = 1500;
const OCR_MAX_UPSCALE_AREA_RATIO: usize = 4;
const OCR_MIN_PIXEL_FLOOR_TILES: usize = 32;
const OCR_BATCH_MAX_TOKEN_INFLATION_NUM: usize = 9;
const OCR_BATCH_MAX_TOKEN_INFLATION_DEN: usize = 5;
const REPETITION_SINGLE_TOKEN_LIMIT: usize = 8;
const REPETITION_PATTERN_LIMITS: &[(usize, usize)] = &[(2, 6), (4, 4), (6, 4)];
const LOW_DIVERSITY_WINDOW: usize = 48;
const LOW_DIVERSITY_MAX_UNIQUE_TOKENS: usize = 10;

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

    fn min_pixels(
        self,
        preprocessor: &PaddleOcrVlPreprocessorConfig,
        image_width: usize,
        image_height: usize,
    ) -> usize {
        if !matches!(self, Self::Ocr) {
            return preprocessor.min_pixels;
        }

        let image_pixels = image_width.saturating_mul(image_height);
        let floor = preprocessor.factor().pow(2) * OCR_MIN_PIXEL_FLOOR_TILES;
        let capped = image_pixels.saturating_mul(OCR_MAX_UPSCALE_AREA_RATIO);
        preprocessor.min_pixels.min(capped.max(floor))
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

struct BatchGroup {
    indices: Vec<usize>,
    bucket_width: u32,
    bucket_height: u32,
    bucket_num_image_tokens: usize,
    min_original_num_image_tokens: usize,
}

pub struct PaddleOcrVl {
    model: PaddleOCRVLModel,
    tokenizer: Tokenizer,
    config: PaddleOcrVlConfig,
    preprocessor: PaddleOcrVlPreprocessorConfig,
    device: Device,
    dtype: DType,
    mean: Tensor,
    std: Tensor,
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
        let mean =
            Tensor::from_slice(&preprocessor.image_mean, (1, 3, 1, 1), &device)?.to_dtype(dtype)?;
        let std = Tensor::from_slice(
            &preprocessor
                .image_std
                .map(|value| if value == 0.0 { 1.0 } else { value }),
            (1, 3, 1, 1),
            &device,
        )?
        .to_dtype(dtype)?;
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
            mean,
            std,
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
        let prepared = preprocess_image(
            image,
            &self.preprocessor,
            task,
            self.dtype,
            &self.device,
            &self.mean,
            &self.std,
        )?;
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

        let started = Instant::now();
        let mut prepared_images = Vec::with_capacity(images.len());
        let mut original_sizes = Vec::with_capacity(images.len());

        let preprocess_started = Instant::now();
        for image in images {
            let prepared = preprocess_image(
                image,
                &self.preprocessor,
                task,
                self.dtype,
                &self.device,
                &self.mean,
                &self.std,
            )?;
            prepared_images.push(prepared);
            original_sizes.push((image.width(), image.height()));
        }
        let preprocess_elapsed = preprocess_started.elapsed();

        let groups = build_batch_groups(&prepared_images, &self.preprocessor, task);

        let mut outputs = vec![None; images.len()];
        let generation_started = Instant::now();
        let group_count = groups.len();
        let max_batch_size = groups
            .iter()
            .map(|group| group.indices.len())
            .max()
            .unwrap_or(0);
        let max_bucket_image_tokens = groups
            .iter()
            .map(|group| group.bucket_num_image_tokens)
            .max()
            .unwrap_or(0);
        let total_bucket_image_tokens = groups
            .iter()
            .map(|group| group.bucket_num_image_tokens * group.indices.len())
            .sum::<usize>();
        for group in groups {
            group.indices.first().context("empty batch group")?;
            let input_ids = build_batched_input_tokens(
                &self.tokenizer,
                task,
                group.bucket_num_image_tokens,
                &self.config,
                group.indices.len(),
                &self.device,
            )?;
            let pixel_values = cat_batch(
                pad_pixel_batch(
                    group
                        .indices
                        .iter()
                        .map(|&index| &prepared_images[index].pixel_values),
                    group.bucket_height,
                    group.bucket_width,
                )?
                .iter()
                .collect::<Vec<_>>(),
            )?;
            let bucket_grid_thw = grid_thw_tensor(
                group.bucket_height,
                group.bucket_width,
                &self.preprocessor,
                &self.device,
            )?;
            let grid_thw = cat_batch(std::iter::repeat_n(&bucket_grid_thw, group.indices.len()))?;
            let generated_batch =
                self.generate_batch(&input_ids, &pixel_values, &grid_thw, max_new_tokens)?;

            for (batch_index, &image_index) in group.indices.iter().enumerate() {
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
        let generation_elapsed = generation_started.elapsed();

        let decode_started = Instant::now();
        let outputs = outputs
            .into_iter()
            .map(|output| output.context("missing batch inference output"))
            .collect::<Result<Vec<_>>>()?;
        tracing::info!(
            images = images.len(),
            group_count,
            max_batch_size,
            max_bucket_image_tokens,
            total_bucket_image_tokens,
            preprocess_ms = preprocess_elapsed.as_millis(),
            generation_ms = generation_elapsed.as_millis(),
            decode_ms = decode_started.elapsed().as_millis(),
            total_ms = started.elapsed().as_millis(),
            "paddleocr-vl timings"
        );
        Ok(outputs)
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
        let next_tokens_tensor = logits.argmax(D::Minus1)?.to_dtype(DType::U32)?;
        let next_tokens = next_tokens_tensor.to_vec1::<u32>()?;
        let mut finished = vec![false; batch_size];
        for (index, token) in next_tokens.iter().copied().enumerate() {
            generated[index].push(token);
            if token == self.eos_token_id || should_stop_on_repetition(&generated[index]) {
                finished[index] = true;
            }
        }
        if finished.iter().all(|&done| done) || max_new_tokens == 1 {
            return Ok(generated);
        }

        let mut seqlen_offset = current_ids.dim(1)?;
        current_ids = next_tokens_tensor.unsqueeze(1)?;

        for _ in 1..max_new_tokens {
            let logits = self
                .model
                .forward(&current_ids, None, None, seqlen_offset)?;
            let next_tokens_tensor = logits.argmax(D::Minus1)?.to_dtype(DType::U32)?;
            let next_tokens = next_tokens_tensor.to_vec1::<u32>()?;

            for (index, token) in next_tokens.iter().copied().enumerate() {
                if finished[index] {
                    continue;
                }
                generated[index].push(token);
                if token == self.eos_token_id || should_stop_on_repetition(&generated[index]) {
                    finished[index] = true;
                }
            }

            if finished.iter().all(|&done| done) {
                break;
            }

            seqlen_offset += 1;
            current_ids = next_tokens_tensor.unsqueeze(1)?;
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
    mean: &Tensor,
    std: &Tensor,
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
            task.min_pixels(
                preprocessor,
                image.width() as usize,
                image.height() as usize,
            ),
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
    let pixel_values = rgb_to_tensor(&rgb, preprocessor, dtype, device, mean, std)?;
    let grid_thw = grid_thw_tensor(processed_height, processed_width, preprocessor, device)?;
    let num_image_tokens = bucket_num_image_tokens(processed_height, processed_width, preprocessor);

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
    mean: &Tensor,
    std: &Tensor,
) -> Result<Tensor> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let tensor = Tensor::from_vec(
        image.clone().into_raw(),
        (1, height, width, 3),
        &Device::Cpu,
    )?
    .to_device(device)?
    .permute((0, 3, 1, 2))?
    .to_dtype(dtype)?;
    let tensor = if preprocessor.do_rescale {
        tensor.affine(preprocessor.rescale_factor as f64, 0.0)?
    } else {
        tensor
    };
    if preprocessor.do_normalize {
        Ok(tensor.broadcast_sub(mean)?.broadcast_div(std)?)
    } else {
        Ok(tensor)
    }
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

fn build_batch_groups(
    prepared_images: &[PreparedImage],
    preprocessor: &PaddleOcrVlPreprocessorConfig,
    task: PaddleOcrVlTask,
) -> Vec<BatchGroup> {
    if !matches!(task, PaddleOcrVlTask::Ocr) {
        let mut groups: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
        for (index, prepared) in prepared_images.iter().enumerate() {
            groups
                .entry((prepared.processed_width, prepared.processed_height))
                .or_default()
                .push(index);
        }
        return groups
            .into_iter()
            .map(|((bucket_width, bucket_height), indices)| BatchGroup {
                min_original_num_image_tokens: bucket_num_image_tokens(
                    bucket_height,
                    bucket_width,
                    preprocessor,
                ),
                bucket_num_image_tokens: bucket_num_image_tokens(
                    bucket_height,
                    bucket_width,
                    preprocessor,
                ),
                indices,
                bucket_width,
                bucket_height,
            })
            .collect();
    }

    let mut sorted_indices = (0..prepared_images.len()).collect::<Vec<_>>();
    sorted_indices.sort_by(|&lhs, &rhs| {
        let lhs_prepared = &prepared_images[lhs];
        let rhs_prepared = &prepared_images[rhs];
        lhs_prepared
            .num_image_tokens
            .cmp(&rhs_prepared.num_image_tokens)
            .then_with(|| {
                lhs_prepared
                    .processed_height
                    .cmp(&rhs_prepared.processed_height)
            })
            .then_with(|| {
                lhs_prepared
                    .processed_width
                    .cmp(&rhs_prepared.processed_width)
            })
    });

    let max_pixels = task.max_pixels(preprocessor);
    let mut groups = Vec::<BatchGroup>::new();
    for index in sorted_indices {
        let prepared = &prepared_images[index];
        let mut best_group_index = None;
        let mut best_group_tokens = usize::MAX;

        for (group_index, group) in groups.iter().enumerate() {
            let candidate_width = group.bucket_width.max(prepared.processed_width);
            let candidate_height = group.bucket_height.max(prepared.processed_height);
            let candidate_pixels =
                (candidate_width as usize).saturating_mul(candidate_height as usize);
            if candidate_pixels > max_pixels {
                continue;
            }

            let candidate_tokens =
                bucket_num_image_tokens(candidate_height, candidate_width, preprocessor);
            let candidate_min_tokens = group
                .min_original_num_image_tokens
                .min(prepared.num_image_tokens);
            if candidate_tokens.saturating_mul(OCR_BATCH_MAX_TOKEN_INFLATION_DEN)
                > candidate_min_tokens.saturating_mul(OCR_BATCH_MAX_TOKEN_INFLATION_NUM)
            {
                continue;
            }

            if candidate_tokens < best_group_tokens {
                best_group_tokens = candidate_tokens;
                best_group_index = Some(group_index);
            }
        }

        if let Some(group_index) = best_group_index {
            let group = &mut groups[group_index];
            group.bucket_width = group.bucket_width.max(prepared.processed_width);
            group.bucket_height = group.bucket_height.max(prepared.processed_height);
            group.bucket_num_image_tokens =
                bucket_num_image_tokens(group.bucket_height, group.bucket_width, preprocessor);
            group.min_original_num_image_tokens = group
                .min_original_num_image_tokens
                .min(prepared.num_image_tokens);
            group.indices.push(index);
        } else {
            groups.push(BatchGroup {
                indices: vec![index],
                bucket_width: prepared.processed_width,
                bucket_height: prepared.processed_height,
                bucket_num_image_tokens: prepared.num_image_tokens,
                min_original_num_image_tokens: prepared.num_image_tokens,
            });
        }
    }

    groups
}

fn pad_pixel_batch<'a>(
    tensors: impl IntoIterator<Item = &'a Tensor>,
    bucket_height: u32,
    bucket_width: u32,
) -> Result<Vec<Tensor>> {
    let bucket_height = bucket_height as usize;
    let bucket_width = bucket_width as usize;
    tensors
        .into_iter()
        .map(|tensor| {
            let (_, _, height, width) = tensor.dims4()?;
            let mut padded = tensor.clone();
            if bucket_height > height {
                padded = padded.pad_with_same(2, 0, bucket_height - height)?;
            }
            if bucket_width > width {
                padded = padded.pad_with_same(3, 0, bucket_width - width)?;
            }
            Ok(padded)
        })
        .collect()
}

fn bucket_num_image_tokens(
    processed_height: u32,
    processed_width: u32,
    preprocessor: &PaddleOcrVlPreprocessorConfig,
) -> usize {
    let grid_h = processed_height as usize / preprocessor.patch_size;
    let grid_w = processed_width as usize / preprocessor.patch_size;
    (grid_h / preprocessor.merge_size) * (grid_w / preprocessor.merge_size)
}

fn grid_thw_tensor(
    processed_height: u32,
    processed_width: u32,
    preprocessor: &PaddleOcrVlPreprocessorConfig,
    device: &Device,
) -> Result<Tensor> {
    let grid_h = processed_height as usize / preprocessor.patch_size;
    let grid_w = processed_width as usize / preprocessor.patch_size;
    Tensor::new(&[[1u32, grid_h as u32, grid_w as u32]], device).map_err(Into::into)
}

fn should_stop_on_repetition(tokens: &[u32]) -> bool {
    if tokens.len() >= REPETITION_SINGLE_TOKEN_LIMIT {
        let last = tokens[tokens.len() - 1];
        if tokens[tokens.len() - REPETITION_SINGLE_TOKEN_LIMIT..]
            .iter()
            .all(|&token| token == last)
        {
            return true;
        }
    }

    if REPETITION_PATTERN_LIMITS
        .iter()
        .any(|&(pattern_len, repeats)| repeated_suffix(tokens, pattern_len, repeats))
    {
        return true;
    }

    if tokens.len() < LOW_DIVERSITY_WINDOW {
        return false;
    }

    let mut unique = Vec::with_capacity(LOW_DIVERSITY_MAX_UNIQUE_TOKENS + 1);
    for &token in &tokens[tokens.len() - LOW_DIVERSITY_WINDOW..] {
        if unique.contains(&token) {
            continue;
        }
        unique.push(token);
        if unique.len() > LOW_DIVERSITY_MAX_UNIQUE_TOKENS {
            return false;
        }
    }

    true
}

fn repeated_suffix(tokens: &[u32], pattern_len: usize, repeats: usize) -> bool {
    let total = pattern_len.saturating_mul(repeats);
    if pattern_len == 0 || repeats < 2 || tokens.len() < total {
        return false;
    }

    let pattern_start = tokens.len() - pattern_len;
    let pattern = &tokens[pattern_start..];
    (2..=repeats).all(|repeat_index| {
        let start = tokens.len() - repeat_index * pattern_len;
        &tokens[start..start + pattern_len] == pattern
    })
}

#[cfg(test)]
mod tests {
    use super::{
        PaddleOcrVlPreprocessorConfig, PaddleOcrVlTask, PreparedImage, bucket_num_image_tokens,
        build_batch_groups, grid_thw_tensor, should_stop_on_repetition, smart_resize,
    };
    use candle_core::{DType, Device, Tensor};

    fn preprocessor() -> PaddleOcrVlPreprocessorConfig {
        PaddleOcrVlPreprocessorConfig {
            do_convert_rgb: true,
            do_normalize: true,
            do_rescale: true,
            do_resize: true,
            image_mean: [0.5, 0.5, 0.5],
            image_std: [0.5, 0.5, 0.5],
            max_pixels: 1_003_520,
            merge_size: 2,
            min_pixels: 112_896,
            patch_size: 14,
            rescale_factor: 1.0 / 255.0,
            temporal_patch_size: 1,
        }
    }

    #[test]
    fn ocr_min_pixels_caps_small_crop_upscale() {
        let preprocessor = preprocessor();
        let min_pixels = PaddleOcrVlTask::Ocr.min_pixels(&preprocessor, 270, 48);
        assert_eq!(min_pixels, 51_840);
    }

    #[test]
    fn smart_resize_honors_capped_ocr_min_pixels() -> anyhow::Result<()> {
        let preprocessor = preprocessor();
        let min_pixels = PaddleOcrVlTask::Ocr.min_pixels(&preprocessor, 270, 48);
        let (height, width) = smart_resize(
            48,
            270,
            preprocessor.factor(),
            min_pixels,
            preprocessor.max_pixels,
        )?;
        assert_eq!((height, width), (112, 560));
        Ok(())
    }

    #[test]
    fn repetition_guard_detects_repeated_suffix_patterns() {
        assert!(should_stop_on_repetition(&[
            1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2
        ]));
        assert!(should_stop_on_repetition(&[9, 9, 9, 9, 9, 9, 9, 9]));
        assert!(!should_stop_on_repetition(&[1, 2, 3, 1, 2, 4, 1, 2]));
    }

    #[test]
    fn repetition_guard_detects_low_diversity_long_outputs() {
        let mut tokens = Vec::new();
        for _ in 0..12 {
            tokens.extend_from_slice(&[1, 2, 3, 4]);
        }
        assert!(should_stop_on_repetition(&tokens));
    }

    fn prepared_image(
        width: u32,
        height: u32,
        preprocessor: &PaddleOcrVlPreprocessorConfig,
    ) -> anyhow::Result<PreparedImage> {
        let device = Device::Cpu;
        let pixel_values =
            Tensor::zeros((1, 3, height as usize, width as usize), DType::F32, &device)?;
        let grid_thw = grid_thw_tensor(height, width, preprocessor, &device)?;
        Ok(PreparedImage {
            pixel_values,
            grid_thw,
            processed_width: width,
            processed_height: height,
            num_image_tokens: bucket_num_image_tokens(height, width, preprocessor),
            upscaled_for_spotting: false,
        })
    }

    #[test]
    fn ocr_batch_groups_merge_compatible_shapes_into_padded_buckets() -> anyhow::Result<()> {
        let preprocessor = preprocessor();
        let prepared_images = [
            prepared_image(112, 252, &preprocessor)?,
            prepared_image(112, 252, &preprocessor)?,
            prepared_image(84, 336, &preprocessor)?,
            prepared_image(140, 196, &preprocessor)?,
            prepared_image(252, 392, &preprocessor)?,
            prepared_image(280, 448, &preprocessor)?,
            prepared_image(224, 560, &preprocessor)?,
            prepared_image(476, 252, &preprocessor)?,
        ];

        let groups = build_batch_groups(&prepared_images, &preprocessor, PaddleOcrVlTask::Ocr);
        assert_eq!(groups.len(), 3);
        assert_eq!(
            groups.iter().map(|group| group.indices.len()).max(),
            Some(4)
        );
        assert!(
            groups
                .iter()
                .any(|group| group.indices.len() == 4 && group.bucket_num_image_tokens == 60)
        );
        assert!(
            groups
                .iter()
                .any(|group| group.indices.len() == 3 && group.bucket_num_image_tokens == 200)
        );
        assert_eq!(
            groups
                .iter()
                .map(|group| group.bucket_num_image_tokens * group.indices.len())
                .sum::<usize>(),
            993
        );
        Ok(())
    }

    #[test]
    fn non_ocr_batch_groups_keep_exact_processed_shapes() -> anyhow::Result<()> {
        let preprocessor = preprocessor();
        let prepared_images = [
            prepared_image(112, 252, &preprocessor)?,
            prepared_image(84, 336, &preprocessor)?,
        ];

        let groups = build_batch_groups(&prepared_images, &preprocessor, PaddleOcrVlTask::Table);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].bucket_width, 84);
        assert_eq!(groups[0].bucket_height, 336);
        assert_eq!(groups[1].bucket_width, 112);
        assert_eq!(groups[1].bucket_height, 252);
        Ok(())
    }
}
