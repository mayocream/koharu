mod model;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use image::{DynamicImage, RgbImage, imageops::FilterType};
use koharu_runtime::RuntimeManager;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use model::{Mit48pxModel, RawPrediction};

use crate::{comic_text_detector::extract_text_block_regions, device, loading, types::TextRegion};

const OCR_CHUNK_SIZE: usize = 16;
const HF_REPO: &str = "mayocream/mit48px-ocr";

koharu_runtime::declare_hf_model_package!(id: "model:mit48px-ocr:config", repo: HF_REPO, file: "config.json", bootstrap: false, order: 210);
koharu_runtime::declare_hf_model_package!(id: "model:mit48px-ocr:dictionary", repo: HF_REPO, file: "alphabet-all-v7.txt", bootstrap: false, order: 211);
koharu_runtime::declare_hf_model_package!(id: "model:mit48px-ocr:weights", repo: HF_REPO, file: "model.safetensors", bootstrap: false, order: 212);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mit48pxConfig {
    pub text_height: u32,
    pub max_width: u32,
    pub embd_dim: usize,
    pub num_heads: usize,
    pub encoder_layers: usize,
    pub decoder_layers: usize,
    pub beam_size_default: usize,
    pub max_seq_length_default: usize,
    pub pad_token_id: u32,
    pub bos_token_id: u32,
    pub eos_token_id: u32,
    pub space_token: String,
    pub dictionary_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mit48pxPrediction {
    pub text: String,
    pub confidence: f32,
    pub text_color: [u8; 3],
    pub stroke_color: [u8; 3],
    pub has_text_color: bool,
    pub has_stroke_color: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mit48pxBlockPrediction {
    pub block_index: usize,
    pub text: String,
    pub confidence: f32,
    pub text_color: [u8; 3],
    pub stroke_color: [u8; 3],
}

struct PreparedBatch {
    tensor: Tensor,
    widths: Vec<u32>,
}

struct ModelFiles {
    config: PathBuf,
    dictionary: PathBuf,
    weights: PathBuf,
}

pub struct Mit48pxOcr {
    model: Mit48pxModel,
    config: Mit48pxConfig,
    dictionary: Vec<String>,
    device: Device,
    dtype: DType,
}

impl Mit48pxOcr {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let hf = runtime.downloads();
        let files = ModelFiles {
            config: hf.huggingface_model(HF_REPO, "config.json").await?,
            dictionary: hf.huggingface_model(HF_REPO, "alphabet-all-v7.txt").await?,
            weights: hf.huggingface_model(HF_REPO, "model.safetensors").await?,
        };
        Self::load_from_files(files, cpu)
    }

    pub fn load_from_dir(dir: impl AsRef<Path>, cpu: bool) -> Result<Self> {
        let dir = dir.as_ref();
        Self::load_from_files(
            ModelFiles {
                config: dir.join("config.json"),
                dictionary: dir.join("alphabet-all-v7.txt"),
                weights: dir.join("model.safetensors"),
            },
            cpu,
        )
    }

    fn load_from_files(files: ModelFiles, cpu: bool) -> Result<Self> {
        let device = device(cpu)?;
        let dtype = loading::model_dtype(&device);
        let config: Mit48pxConfig =
            loading::read_json(&files.config).context("failed to parse mit48px config")?;
        let dictionary = read_dictionary(&files.dictionary)?;
        let data = std::fs::read(&files.weights)
            .with_context(|| format!("failed to read {}", files.weights.display()))?;
        let vb = VarBuilder::from_buffered_safetensors(data, dtype, &device)?;
        let model = Mit48pxModel::new(config.clone(), dictionary.len(), vb, device.clone())?;
        Ok(Self {
            model,
            config,
            dictionary,
            device,
            dtype,
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_regions(&self, regions: &[DynamicImage]) -> Result<Vec<Mit48pxPrediction>> {
        if regions.is_empty() {
            return Ok(Vec::new());
        }

        let mut predictions = Vec::with_capacity(regions.len());
        for chunk in regions.chunks(OCR_CHUNK_SIZE) {
            let batch = preprocess_regions(chunk, &self.config, &self.device, self.dtype)?;
            let raw = self.model.infer_batch(&batch.tensor, &batch.widths)?;
            for prediction in raw {
                predictions.push(self.decode_prediction(prediction));
            }
        }
        Ok(predictions)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_text_blocks(
        &self,
        image: &DynamicImage,
        blocks: &[TextRegion],
    ) -> Result<Vec<Mit48pxBlockPrediction>> {
        let mut regions = Vec::new();
        let mut block_indices = Vec::new();
        for (block_index, block) in blocks.iter().enumerate() {
            for region in extract_text_block_regions(image, block) {
                regions.push(region);
                block_indices.push(block_index);
            }
        }

        let line_predictions = self.inference_regions(&regions)?;
        let mut grouped = vec![Vec::<Mit48pxPrediction>::new(); blocks.len()];
        for (prediction, block_index) in line_predictions.into_iter().zip(block_indices) {
            grouped[block_index].push(prediction);
        }

        let mut outputs = Vec::with_capacity(blocks.len());
        for (block_index, lines) in grouped.into_iter().enumerate() {
            if lines.is_empty() {
                outputs.push(Mit48pxBlockPrediction {
                    block_index,
                    text: String::new(),
                    confidence: 0.0,
                    text_color: [0, 0, 0],
                    stroke_color: [0, 0, 0],
                });
                continue;
            }

            let text = lines
                .iter()
                .map(|line| normalize_ocr_text(&line.text))
                .collect::<Vec<_>>()
                .join("");
            let confidence =
                lines.iter().map(|line| line.confidence).sum::<f32>() / lines.len() as f32;
            let text_color = average_rgb(lines.iter().map(|line| line.text_color));
            let stroke_color = average_rgb(lines.iter().map(|line| line.stroke_color));

            outputs.push(Mit48pxBlockPrediction {
                block_index,
                text,
                confidence,
                text_color,
                stroke_color,
            });
        }

        Ok(outputs)
    }

    fn decode_prediction(&self, prediction: RawPrediction) -> Mit48pxPrediction {
        let mut text = String::new();
        let mut fg_sum = [0f32; 3];
        let mut bg_sum = [0f32; 3];
        let mut fg_count = 0usize;
        let mut bg_count = 0usize;
        let mut has_text_color = false;
        let mut has_stroke_color = false;

        let len = prediction
            .token_ids
            .len()
            .min(prediction.fg_colors.len())
            .min(prediction.bg_colors.len())
            .min(prediction.fg_indicators.len())
            .min(prediction.bg_indicators.len());

        for index in 0..len {
            let token_id = prediction.token_ids[index] as usize;
            let token = self
                .dictionary
                .get(token_id)
                .map(String::as_str)
                .unwrap_or("<UNK>");
            if token == "<S>" {
                continue;
            }
            if token == "</S>" {
                break;
            }

            if token == self.config.space_token {
                text.push(' ');
            } else {
                text.push_str(token);
            }

            let fg = prediction.fg_colors[index];
            let bg = prediction.bg_colors[index];
            let fg_present =
                prediction.fg_indicators[index][1] > prediction.fg_indicators[index][0];
            let bg_present =
                prediction.bg_indicators[index][1] > prediction.bg_indicators[index][0];
            if fg_present {
                has_text_color = true;
                accumulate_rgb(&mut fg_sum, fg);
                fg_count += 1;
            }
            if bg_present {
                has_stroke_color = true;
                accumulate_rgb(&mut bg_sum, bg);
                bg_count += 1;
            } else {
                accumulate_rgb(&mut bg_sum, fg);
                bg_count += 1;
            }
        }

        Mit48pxPrediction {
            text: normalize_ocr_text(&text),
            confidence: prediction.confidence,
            text_color: finish_rgb(fg_sum, fg_count),
            stroke_color: finish_rgb(bg_sum, bg_count),
            has_text_color,
            has_stroke_color,
        }
    }
}

fn normalize_ocr_text(text: &str) -> String {
    text.chars()
        .filter(|&ch| ch != '\n' && ch != '\r')
        .collect()
}

fn read_dictionary(path: &Path) -> Result<Vec<String>> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(data
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect())
}

fn preprocess_regions(
    regions: &[DynamicImage],
    config: &Mit48pxConfig,
    device: &Device,
    dtype: DType,
) -> Result<PreparedBatch> {
    let mut resized = Vec::<RgbImage>::with_capacity(regions.len());
    let mut widths = Vec::with_capacity(regions.len());
    let mut max_width = 1u32;

    for region in regions {
        let region = resize_region(region, config.text_height, config.max_width);
        max_width = max_width.max(region.width());
        widths.push(region.width());
        resized.push(region);
    }

    // The source checkpoint expects seven blank pixels before the ConvNeXt
    // backbone. That extra slack affects the backbone feature width and therefore the
    // encoder mask shape, so keep it byte-for-byte compatible instead of rounding to 4.
    let padded_width = max_width.saturating_add(7);
    let height = config.text_height as usize;
    let width = padded_width as usize;
    let mut flat = vec![-1.0f32; resized.len() * height * width * 3];

    for (batch_index, image) in resized.iter().enumerate() {
        for y in 0..image.height() as usize {
            for x in 0..image.width() as usize {
                let pixel = image.get_pixel(x as u32, y as u32).0;
                let offset = ((batch_index * height + y) * width + x) * 3;
                flat[offset] = pixel[0] as f32 / 127.5 - 1.0;
                flat[offset + 1] = pixel[1] as f32 / 127.5 - 1.0;
                flat[offset + 2] = pixel[2] as f32 / 127.5 - 1.0;
            }
        }
    }

    let tensor = Tensor::from_vec(flat, (resized.len(), height, width, 3), device)?
        .permute((0, 3, 1, 2))?
        .to_dtype(dtype)?;
    Ok(PreparedBatch { tensor, widths })
}

fn resize_region(region: &DynamicImage, text_height: u32, max_width: u32) -> RgbImage {
    let rgb = region.to_rgb8();
    let (width, height) = rgb.dimensions();
    let new_width = ((width as f32 / height.max(1) as f32) * text_height as f32)
        .round()
        .clamp(1.0, max_width as f32) as u32;
    if width == new_width && height == text_height {
        rgb
    } else {
        image::imageops::resize(&rgb, new_width, text_height, FilterType::Triangle)
    }
}

fn accumulate_rgb(sum: &mut [f32; 3], color: [f32; 3]) {
    for (dst, src) in sum.iter_mut().zip(color) {
        *dst += src * 255.0;
    }
}

fn finish_rgb(sum: [f32; 3], count: usize) -> [u8; 3] {
    if count == 0 {
        return [0, 0, 0];
    }
    let denom = count as f32;
    [
        ((sum[0] / denom).round() as i32).clamp(0, 255) as u8,
        ((sum[1] / denom).round() as i32).clamp(0, 255) as u8,
        ((sum[2] / denom).round() as i32).clamp(0, 255) as u8,
    ]
}

fn average_rgb(colors: impl Iterator<Item = [u8; 3]>) -> [u8; 3] {
    let mut sum = [0f32; 3];
    let mut count = 0usize;
    for color in colors {
        for (index, channel) in color.into_iter().enumerate() {
            sum[index] += channel as f32;
        }
        count += 1;
    }
    if count == 0 {
        return [0, 0, 0];
    }
    [
        (sum[0] / count as f32).round().clamp(0.0, 255.0) as u8,
        (sum[1] / count as f32).round().clamp(0.0, 255.0) as u8,
        (sum[2] / count as f32).round().clamp(0.0, 255.0) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use image::{DynamicImage, RgbImage};

    use super::{
        Mit48pxConfig, Mit48pxPrediction, finish_rgb, normalize_ocr_text, preprocess_regions,
    };

    fn test_config() -> Mit48pxConfig {
        Mit48pxConfig {
            text_height: 48,
            max_width: 8100,
            embd_dim: 320,
            num_heads: 4,
            encoder_layers: 4,
            decoder_layers: 5,
            beam_size_default: 5,
            max_seq_length_default: 255,
            pad_token_id: 0,
            bos_token_id: 1,
            eos_token_id: 2,
            space_token: "<SP>".to_string(),
            dictionary_file: "alphabet-all-v7.txt".to_string(),
        }
    }

    #[test]
    fn preprocessing_resizes_to_48px_and_matches_ballonstranslator_width_padding()
    -> anyhow::Result<()> {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(25, 10, image::Rgb([255, 0, 0])));
        let batch = preprocess_regions(
            &[image],
            &test_config(),
            &candle_core::Device::Cpu,
            candle_core::DType::F32,
        )?;
        assert_eq!(batch.widths, vec![120]);
        assert_eq!(batch.tensor.dims(), &[1, 3, 48, 127]);
        Ok(())
    }

    #[test]
    fn finish_rgb_clamps_to_u8_range() {
        assert_eq!(finish_rgb([300.0, 40.0, -10.0], 1), [255, 40, 0]);
    }

    #[test]
    fn block_prediction_shape_remains_serializable() -> anyhow::Result<()> {
        let prediction = Mit48pxPrediction {
            text: "abc".to_string(),
            confidence: 0.5,
            text_color: [1, 2, 3],
            stroke_color: [4, 5, 6],
            has_text_color: true,
            has_stroke_color: false,
        };
        let json = serde_json::to_string(&prediction)?;
        assert!(json.contains("\"hasTextColor\":true"));
        Ok(())
    }

    #[test]
    fn normalize_ocr_text_removes_newlines() {
        assert_eq!(normalize_ocr_text("ab\ncd\r\nef"), "abcdef");
    }

    #[test]
    #[ignore]
    fn local_model_dir_loads_and_ocrs_a_crop() -> anyhow::Result<()> {
        let model_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("target/mit48px-local");
        if !model_dir.exists() {
            anyhow::bail!("missing local mit48px assets at {}", model_dir.display());
        }

        let model = super::Mit48pxOcr::load_from_dir(&model_dir, true)?;
        let image = image::open(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("data/bluearchive_comics/1.jpg"),
        )?;
        let crop = image.crop_imm(66, 26, 270, 48);
        let output = model.inference_regions(&[crop])?;
        assert_eq!(output.len(), 1);
        assert!(!output[0].text.is_empty());
        Ok(())
    }

    #[test]
    #[ignore]
    fn local_model_matches_reference_text_on_known_crop() -> anyhow::Result<()> {
        let model_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("target/mit48px-local");
        if !model_dir.exists() {
            anyhow::bail!("missing local mit48px assets at {}", model_dir.display());
        }

        let model = super::Mit48pxOcr::load_from_dir(&model_dir, true)?;
        let image = image::open(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("data/140817417_p0.jpg"),
        )?;
        let crop = image.crop_imm(48, 232, 1172, 388);
        let output = model.inference_regions(&[crop])?;
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].text, "デカグラマトン戦闘");
        Ok(())
    }
}
