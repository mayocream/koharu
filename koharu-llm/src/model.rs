use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Once;
use std::time::Instant;

use anyhow::{Context, Result};

use crate::prompt::PromptRenderer;
use crate::safe::context::params::LlamaContextParams;
use crate::safe::llama_backend::LlamaBackend;
use crate::safe::llama_batch::LlamaBatch;
use crate::safe::model::params::LlamaModelParams;
use crate::safe::model::{AddBos, LlamaModel};
use crate::safe::sampling::LlamaSampler;
use crate::safe::token::LlamaToken;
use crate::safe::{LogOptions, send_logs_to_tracing};
use crate::{Language, ModelId};

static LOGGING_READY: Once = Once::new();

const DEFAULT_GPU_LAYERS: u32 = 1000;
const MAX_UBATCH: u32 = 512;
const SAKURA_QWEN_CORRECT_EOS_ID: i32 = 151645;

pub struct Llm {
    model_id: ModelId,
    backend: Arc<LlamaBackend>,
    model: LlamaModel,
    prompt_renderer: PromptRenderer,
    eos_token: LlamaToken,
}

#[derive(Debug, Clone)]
pub struct GenerateOptions {
    pub max_tokens: usize,
    pub temperature: f64,
    pub top_k: Option<usize>,
    pub top_p: Option<f64>,
    pub seed: u64,
    pub split_prompt: bool,
    pub repeat_penalty: f32,
    pub repeat_last_n: usize,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            max_tokens: 1000,
            temperature: 0.1,
            top_k: None,
            top_p: None,
            seed: 299792458,
            split_prompt: false,
            repeat_penalty: 1.1,
            repeat_last_n: 64,
        }
    }
}

impl Llm {
    pub async fn load(id: ModelId, cpu: bool, backend: Arc<LlamaBackend>) -> Result<Self> {
        let model_path = id.get().await?;

        tokio::task::spawn_blocking(move || Self::load_from_path(id, cpu, model_path, backend))
            .await
            .context("failed to join llama.cpp model loading task")?
    }

    fn load_from_path(
        id: ModelId,
        cpu: bool,
        model_path: PathBuf,
        backend: Arc<LlamaBackend>,
    ) -> Result<Self> {
        crate::sys::initialize().context("failed to initialize llama.cpp runtime bindings")?;

        LOGGING_READY.call_once(|| {
            send_logs_to_tracing(LogOptions::default().with_logs_enabled(true));
        });

        let model_params = model_params(cpu, backend.as_ref());
        let model = LlamaModel::load_from_file(backend.as_ref(), &model_path, &model_params)
            .with_context(|| format!("unable to load model from `{}`", model_path.display()))?;

        let chat_template = model
            .meta_val_str("tokenizer.ggml.chat_template")
            .or_else(|_| model.meta_val_str("tokenizer.chat_template"))
            .context("missing chat template in GGUF metadata")?;

        let bos_token = token_text(&model, model.token_bos());
        let (eos_token, eos_text) = eos_token_for(id, &model);
        let prompt_renderer = PromptRenderer::new(id, chat_template, bos_token, eos_text);

        Ok(Self {
            model_id: id,
            backend,
            model,
            prompt_renderer,
            eos_token,
        })
    }

    pub fn id(&self) -> ModelId {
        self.model_id
    }

    pub fn generate(
        &mut self,
        prompt: &str,
        opts: &GenerateOptions,
        target_language: Language,
    ) -> Result<String> {
        if opts.max_tokens == 0 {
            return Ok(String::new());
        }

        let prompt = self
            .prompt_renderer
            .format_chat_prompt(prompt.to_string(), target_language)?;
        tracing::info!("Generating with prompt:\n{}", prompt);

        let prompt_tokens = self
            .model
            .str_to_token(&prompt, AddBos::Never)
            .context("failed to tokenize prompt")?;
        if prompt_tokens.is_empty() {
            anyhow::bail!("prompt produced no tokens");
        }

        let mut ctx = self
            .model
            .new_context(
                self.backend.as_ref(),
                context_params(prompt_tokens.len(), opts.max_tokens)?,
            )
            .context("unable to create llama.cpp context")?;
        let mut sampler = build_sampler(opts);
        let mut decoder = encoding_rs::UTF_8.new_decoder();

        let start_prompt_processing = Instant::now();
        let mut next_token = if opts.split_prompt {
            self.process_prompt_split(&mut ctx, &prompt_tokens, &mut sampler)?
        } else {
            self.process_prompt_batch(&mut ctx, &prompt_tokens, &mut sampler)?
        };
        let prompt_dt = start_prompt_processing.elapsed();

        tracing::info!(
            "{:4} prompt tokens processed: {:.2} token/s",
            prompt_tokens.len(),
            rate(prompt_tokens.len(), prompt_dt)
        );

        if self.should_stop(next_token) {
            tracing::warn!("Early stopping: EOS/EOG token generated at end of prompt");
            return Ok(String::new());
        }

        let start_post_prompt = Instant::now();
        let mut generated = String::new();
        let mut sampled = 0usize;
        let mut position = i32::try_from(prompt_tokens.len()).context("prompt is too long")?;
        let mut batch = LlamaBatch::new(1, 1);

        while sampled < opts.max_tokens {
            sampler.accept(next_token);
            generated.push_str(&decode_token(&self.model, next_token, &mut decoder)?);
            sampled += 1;

            if sampled >= opts.max_tokens {
                break;
            }

            batch.clear();
            batch
                .add(next_token, position, &[0], true)
                .context("failed to add generated token to llama batch")?;
            ctx.decode(&mut batch)
                .context("failed to decode generated token")?;
            position += 1;

            next_token = sampler.sample(&ctx, batch.n_tokens() - 1);
            if self.should_stop(next_token) {
                break;
            }
        }

        let gen_dt = start_post_prompt.elapsed();
        tracing::info!(
            "{:<4} tokens generated: {:.2} token/s",
            sampled,
            rate(sampled, gen_dt)
        );

        Ok(generated)
    }

    fn process_prompt_batch(
        &self,
        ctx: &mut crate::safe::context::LlamaContext<'_>,
        prompt_tokens: &[LlamaToken],
        sampler: &mut LlamaSampler,
    ) -> Result<LlamaToken> {
        let mut batch = LlamaBatch::new(prompt_tokens.len(), 1);
        batch
            .add_sequence(prompt_tokens, 0, false)
            .context("failed to build prompt batch")?;
        ctx.decode(&mut batch)
            .context("failed to process prompt batch")?;
        Ok(sampler.sample(ctx, batch.n_tokens() - 1))
    }

    fn process_prompt_split(
        &self,
        ctx: &mut crate::safe::context::LlamaContext<'_>,
        prompt_tokens: &[LlamaToken],
        sampler: &mut LlamaSampler,
    ) -> Result<LlamaToken> {
        let last_index = prompt_tokens.len() - 1;

        for (index, token) in prompt_tokens.iter().copied().enumerate() {
            let mut batch = LlamaBatch::new(1, 1);
            batch
                .add(
                    token,
                    i32::try_from(index).context("prompt is too long")?,
                    &[0],
                    index == last_index,
                )
                .context("failed to build split prompt batch")?;
            ctx.decode(&mut batch)
                .with_context(|| format!("failed to process prompt token {index}"))?;

            if index == last_index {
                return Ok(sampler.sample(ctx, batch.n_tokens() - 1));
            }
        }

        anyhow::bail!("split prompt processing did not produce a final token")
    }

    fn should_stop(&self, token: LlamaToken) -> bool {
        token == self.eos_token || self.model.is_eog_token(token)
    }
}

fn model_params(cpu: bool, backend: &LlamaBackend) -> LlamaModelParams {
    if !cpu && backend.supports_gpu_offload() {
        LlamaModelParams::default().with_n_gpu_layers(DEFAULT_GPU_LAYERS)
    } else {
        LlamaModelParams::default()
    }
}

fn context_params(prompt_tokens: usize, max_tokens: usize) -> Result<LlamaContextParams> {
    let required_ctx = prompt_tokens
        .saturating_add(max_tokens)
        .saturating_add(1)
        .max(1);
    let n_ctx = NonZeroU32::new(u32::try_from(required_ctx).context("context size exceeds u32")?)
        .expect("required context is always non-zero");
    let n_batch = u32::try_from(prompt_tokens.max(1)).context("prompt batch size exceeds u32")?;
    let n_ubatch = n_batch.min(MAX_UBATCH);

    Ok(LlamaContextParams::default()
        .with_n_ctx(Some(n_ctx))
        .with_n_batch(n_batch)
        .with_n_ubatch(n_ubatch))
}

fn build_sampler(opts: &GenerateOptions) -> LlamaSampler {
    let mut samplers = Vec::new();

    if (opts.repeat_penalty - 1.0).abs() >= f32::EPSILON && opts.repeat_last_n > 0 {
        samplers.push(LlamaSampler::penalties(
            i32::try_from(opts.repeat_last_n).unwrap_or(i32::MAX),
            opts.repeat_penalty,
            0.0,
            0.0,
        ));
    }

    if opts.temperature <= 0.0 {
        samplers.push(LlamaSampler::greedy());
        return LlamaSampler::chain_simple(samplers);
    }

    if let Some(top_k) = opts.top_k.filter(|value| *value > 0) {
        samplers.push(LlamaSampler::top_k(
            i32::try_from(top_k).unwrap_or(i32::MAX),
        ));
    }
    if let Some(top_p) = opts.top_p {
        samplers.push(LlamaSampler::top_p(top_p as f32, 1));
    }

    samplers.push(LlamaSampler::temp(opts.temperature as f32));
    samplers.push(LlamaSampler::dist(opts.seed as u32));
    LlamaSampler::chain_simple(samplers)
}

fn eos_token_for(id: ModelId, model: &LlamaModel) -> (LlamaToken, String) {
    let token = match id {
        ModelId::Sakura1_5bQwen2_5v1_0 => LlamaToken::new(SAKURA_QWEN_CORRECT_EOS_ID),
        _ => model.token_eos(),
    };
    (token, token_text(model, token))
}

fn token_text(model: &LlamaModel, token: LlamaToken) -> String {
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    match model.token_to_piece(token, &mut decoder, true, None) {
        Ok(piece) if !piece.is_empty() => piece,
        _ => token.to_string(),
    }
}

fn decode_token(
    model: &LlamaModel,
    token: LlamaToken,
    decoder: &mut encoding_rs::Decoder,
) -> Result<String> {
    model
        .token_to_piece(token, decoder, true, None)
        .context("failed to decode generated token")
}

fn rate(tokens: usize, duration: std::time::Duration) -> f64 {
    if duration.as_secs_f64() > 0.0 {
        tokens as f64 / duration.as_secs_f64()
    } else {
        0.0
    }
}
