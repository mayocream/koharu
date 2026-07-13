//! llama.cpp/MTMD port of PaddleOCR-VL 1.6 inference.
//!
//! Original implementation:
//! https://github.com/mayocream/koharu/blob/10b9a38656bfb6f505268ee9dbec26a003c92349/koharu-llm/src/paddleocr_vl.rs
//! Official llama.cpp usage:
//! https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6-GGUF/blob/511b09642bb324401f15f97cc23bc67e8f0a291d/README.md

use std::{ffi::CString, num::NonZeroU32, path::Path};

use anyhow::{Context, Result, ensure};
use koharu_llama::{
    context::params::{LlamaAttentionType, LlamaContextParams},
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{LlamaModel, params::LlamaModelParams},
    mtmd::{MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputChunks, MtmdInputText},
    sampling::LlamaSampler,
    token::LlamaToken,
};

use super::processor::{Processor, repeated_suffix_start};
use crate::Backend;

const PADDLEOCR_IMAGE_MARKER: &str = "<|IMAGE_START|><|IMAGE_PLACEHOLDER|><|IMAGE_END|>";

#[derive(Debug)]
pub(super) struct Model {
    backend: &'static LlamaBackend,
    model: LlamaModel,
    mtmd: MtmdContext,
    eos_token: LlamaToken,
}

impl Model {
    pub(super) fn new(
        device: &crate::Device,
        model_path: impl AsRef<Path>,
        mmproj_path: impl AsRef<Path>,
        chat_template: String,
    ) -> Result<(Self, Processor)> {
        let backend = crate::llama_backend().context("llama.cpp backend is not initialized")?;
        let params = model_params(device)?;
        let model = LlamaModel::load_from_file(backend, model_path.as_ref(), &params);
        let model =
            model.with_context(|| format!("failed to load {}", model_path.as_ref().display()))?;
        let processor = Processor::new(&model, chat_template)?;
        let mmproj_path = mmproj_path
            .as_ref()
            .to_str()
            .with_context(|| format!("invalid mmproj path {}", mmproj_path.as_ref().display()))?;
        let mtmd = MtmdContext::init_from_file(
            mmproj_path,
            &model,
            &MtmdContextParams {
                use_gpu: device.backend != Backend::Cpu && backend.supports_gpu_offload(),
                print_timings: false,
                n_threads: std::thread::available_parallelism()
                    .map_or(1, std::num::NonZeroUsize::get)
                    .try_into()
                    .unwrap_or(i32::MAX),
                media_marker: CString::new(PADDLEOCR_IMAGE_MARKER).unwrap(),
                ..MtmdContextParams::default()
            },
        );
        let mtmd = mtmd.context("failed to initialize PaddleOCR-VL multimodal projector")?;
        ensure!(
            mtmd.support_vision(),
            "PaddleOCR-VL projector does not advertise vision support"
        );
        let eos_token = model.token_eos();
        tracing::info!(
            has_encoder = model.has_encoder(),
            has_decoder = model.has_decoder(),
            "loaded llama.cpp PaddleOCR-VL model"
        );
        Ok((
            Self {
                backend,
                model,
                mtmd,
                eos_token,
            },
            processor,
        ))
    }

    pub(super) fn forward(
        &self,
        bitmap: &MtmdBitmap,
        prompt: String,
        max_new_tokens: usize,
    ) -> Result<String> {
        let chunks = self
            .mtmd
            .tokenize(
                MtmdInputText {
                    text: prompt,
                    add_special: false,
                    parse_special: true,
                },
                &[bitmap],
            )
            .context("failed to tokenize multimodal OCR input")?;
        ensure!(
            !chunks.is_empty(),
            "multimodal tokenization produced no chunks"
        );

        let batch_tokens = max_chunk_tokens(&chunks).max(1);
        let prompt_positions =
            usize::try_from(chunks.total_positions()).context("prompt positions overflow usize")?;
        let prompt_tokens = chunks.total_tokens();
        let context_params = context_params(
            &self.mtmd,
            prompt_positions,
            prompt_tokens,
            batch_tokens,
            max_new_tokens,
        )?;
        let mut context = self
            .model
            .new_context(self.backend, context_params)
            .context("failed to create PaddleOCR-VL llama.cpp context")?;

        let past = chunks
            .eval_chunks(
                &self.mtmd,
                &context,
                0,
                0,
                i32::try_from(batch_tokens).context("batch size exceeds i32")?,
                true,
            )
            .context("failed to evaluate multimodal OCR prompt")?;

        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::penalties(-1, 1.2, 0.0, 0.0),
            LlamaSampler::greedy(),
        ]);
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut text = String::new();
        let mut output_tokens = 0;

        if max_new_tokens > 0 {
            let (mut next_token, mut position) = if self.model.has_encoder() {
                let decoder_start = self.model.decode_start_token();
                let decoder_start = if decoder_start.0 >= 0 {
                    decoder_start
                } else {
                    self.model.token_bos()
                };
                let mut batch = LlamaBatch::new(1, 1);
                batch
                    .add(decoder_start, 0, &[0], true)
                    .context("failed to build decoder start batch")?;
                context
                    .decode(&mut batch)
                    .context("failed to decode PaddleOCR-VL start token")?;
                (sampler.sample(&context, -1), 1)
            } else {
                (sampler.sample(&context, -1), past)
            };

            while output_tokens < max_new_tokens && !self.should_stop(next_token) {
                sampler.accept(next_token);
                text.push_str(
                    &self
                        .model
                        .token_to_piece(next_token, &mut decoder, true, None)
                        .context("failed to decode generated OCR token")?,
                );
                output_tokens += 1;

                if let Some(trim_at) = repeated_suffix_start(&text) {
                    text.truncate(trim_at);
                    break;
                }
                if output_tokens >= max_new_tokens {
                    break;
                }

                let mut batch = LlamaBatch::new(1, 1);
                batch
                    .add(next_token, position, &[0], true)
                    .context("failed to build generated-token batch")?;
                context
                    .decode(&mut batch)
                    .context("failed to decode generated OCR token")?;
                position += 1;
                next_token = sampler.sample(&context, -1);
            }
        }

        Ok(text.trim().to_owned())
    }

    fn should_stop(&self, token: LlamaToken) -> bool {
        token == self.eos_token || self.model.is_eog_token(token)
    }
}

fn model_params(device: &crate::Device) -> Result<LlamaModelParams> {
    if device.backend == Backend::Cpu {
        return Ok(LlamaModelParams::default().with_n_gpu_layers(0));
    }
    ensure!(
        device.index == 0,
        "PaddleOCR-VL llama.cpp currently supports accelerator device index 0"
    );
    Ok(LlamaModelParams::default().with_n_gpu_layers(1000))
}

fn context_params(
    mtmd: &MtmdContext,
    prompt_positions: usize,
    prompt_tokens: usize,
    batch_tokens: usize,
    max_new_tokens: usize,
) -> Result<LlamaContextParams> {
    let required = prompt_positions
        .saturating_add(max_new_tokens)
        .saturating_add(1)
        .max(
            prompt_tokens
                .saturating_add(max_new_tokens)
                .saturating_add(1),
        )
        .max(batch_tokens.saturating_add(1));
    let n_ctx = NonZeroU32::new(u32::try_from(required).context("context size exceeds u32")?)
        .expect("context size is non-zero");
    let n_batch = u32::try_from(batch_tokens.max(1)).context("batch size exceeds u32")?;
    let n_ubatch = if mtmd.decode_use_non_causal() {
        n_batch
    } else {
        n_batch.min(512)
    };
    let mut params = LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_n_batch(n_batch)
        .with_n_ubatch(n_ubatch);
    if mtmd.decode_use_non_causal() {
        params = params.with_attention_type(LlamaAttentionType::NonCausal);
    }
    Ok(params)
}

fn max_chunk_tokens(chunks: &MtmdInputChunks) -> usize {
    (0..chunks.len())
        .filter_map(|index| chunks.get(index))
        .map(|chunk| chunk.n_tokens())
        .max()
        .unwrap_or(0)
}
