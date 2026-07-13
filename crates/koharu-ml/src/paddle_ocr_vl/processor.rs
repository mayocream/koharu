//! PaddleOCR-VL 1.6 prompt and image processing.
//!
//! Official llama.cpp usage:
//! https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6-GGUF/blob/511b09642bb324401f15f97cc23bc67e8f0a291d/README.md

use anyhow::{Context, Result, ensure};
use image::DynamicImage;
use koharu_llama::{model::LlamaModel, mtmd::MtmdBitmap};
use minijinja::{Environment, context};
use serde::{Deserialize, Serialize};

const CHAT_TEMPLATE_NAME: &str = "paddle_ocr_vl";
const REPEAT_MAX_UNIT_CHARS: usize = 12;
const REPEAT_MIN_REPETITIONS: usize = 4;
const REPEAT_MIN_TOTAL_CHARS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaddleOCRVLTask {
    Ocr,
    Table,
    Formula,
    Chart,
    Spotting,
    Seal,
}

impl PaddleOCRVLTask {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaddleOCRVLResult {
    pub text: String,
}

#[derive(Debug)]
pub(super) struct Processor {
    environment: Environment<'static>,
    bos_token: String,
    eos_token: String,
}

#[derive(Debug, Serialize)]
struct PromptMessage {
    role: &'static str,
    content: [PromptContent; 2],
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PromptContent {
    Image,
    Text { text: &'static str },
}

impl Processor {
    pub(super) fn new(model: &LlamaModel, chat_template: String) -> Result<Self> {
        let mut environment = Environment::new();
        environment
            .add_template_owned(CHAT_TEMPLATE_NAME, chat_template)
            .map_err(anyhow::Error::msg)
            .context("failed to parse PaddleOCR-VL chat template")?;
        Ok(Self {
            environment,
            bos_token: token_text(model, model.token_bos())
                .context("failed to decode PaddleOCR-VL BOS token")?,
            eos_token: token_text(model, model.token_eos())
                .context("failed to decode PaddleOCR-VL EOS token")?,
        })
    }

    pub(super) fn bitmap(&self, image: &DynamicImage) -> Result<MtmdBitmap> {
        ensure!(
            image.width() > 0 && image.height() > 0,
            "image dimensions must be non-zero"
        );
        let rgb = image.to_rgb8();
        let (width, height) = rgb.dimensions();
        MtmdBitmap::from_image_data(width, height, &rgb.into_raw())
            .context("failed to create MTMD bitmap from image")
    }

    pub(super) fn render_prompt(&self, task: PaddleOCRVLTask) -> Result<String> {
        let template = self
            .environment
            .get_template(CHAT_TEMPLATE_NAME)
            .map_err(anyhow::Error::msg)
            .context("PaddleOCR-VL chat template is unavailable")?;
        template
            .render(context! {
                messages => [PromptMessage {
                    role: "user",
                    content: [
                        PromptContent::Image,
                        PromptContent::Text { text: task.prompt() },
                    ],
                }],
                bos_token => self.bos_token.as_str(),
                cls_token => self.bos_token.as_str(),
                eos_token => self.eos_token.as_str(),
                add_generation_prompt => true,
            })
            .map_err(anyhow::Error::msg)
            .context("failed to render PaddleOCR-VL chat template")
    }
}

pub(super) fn repeated_suffix_start(text: &str) -> Option<usize> {
    let chars = text
        .char_indices()
        .filter(|(_, character)| !character.is_whitespace())
        .collect::<Vec<_>>();
    let length = chars.len();
    if length < REPEAT_MIN_TOTAL_CHARS {
        return None;
    }

    let max_unit = REPEAT_MAX_UNIT_CHARS.min(length / REPEAT_MIN_REPETITIONS);
    for unit_length in 1..=max_unit {
        let unit = &chars[length - unit_length..];
        let mut repetitions = 1;
        while length >= unit_length * (repetitions + 1) {
            let start = length - unit_length * (repetitions + 1);
            if chars[start..start + unit_length]
                .iter()
                .map(|(_, character)| *character)
                .eq(unit.iter().map(|(_, character)| *character))
            {
                repetitions += 1;
            } else {
                break;
            }
        }
        if repetitions >= REPEAT_MIN_REPETITIONS
            && repetitions * unit_length >= REPEAT_MIN_TOTAL_CHARS
        {
            return Some(chars[length - repetitions * unit_length].0);
        }
    }
    None
}

fn token_text(model: &LlamaModel, token: koharu_llama::token::LlamaToken) -> Result<String> {
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    model
        .token_to_piece(token, &mut decoder, true, None)
        .map_err(Into::into)
}
