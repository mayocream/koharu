use std::io::Seek;

use anyhow::Result;
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::{quantized_llama, quantized_qwen2};
use tokenizers::Tokenizer;

use crate::device;
use crate::llm::ModelId;
use crate::llm::prompt::PromptRenderer;
use crate::llm::quantized_lfm2;

pub enum Model {
    Llama(quantized_llama::ModelWeights),
    Qwen2(quantized_qwen2::ModelWeights),
    Lfm2(quantized_lfm2::ModelWeights),
}

impl Model {
    fn forward(&mut self, input: &Tensor, pos: usize) -> candle_core::Result<Tensor> {
        match self {
            Model::Llama(m) => m.forward(input, pos),
            Model::Qwen2(m) => m.forward(input, pos),
            Model::Lfm2(m) => m.forward(input, pos),
        }
    }
}

/// Minimal quantized LLM wrapper
pub struct Llm {
    device: Device,
    model: Model,
    tokenizer: Tokenizer,
    prompt_renderer: PromptRenderer,
    eos_token_id: u32,
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

// refer: https://github.com/huggingface/candle/blob/d4545ebbbfb37d3cf0e228642ffaaa75b5d6bce9/candle-examples/examples/quantized/main.rs#L235
impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            max_tokens: 1000,
            temperature: 0.8,
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
    /// Constructs a new LLM instance from a quantized GGUF model and tokenizer.json.
    pub async fn load(id: ModelId, use_cpu: bool) -> Result<Self> {
        let (model_path, tokenizer_path) = id.get().await?;

        // Load tokenizer
        // TODO: load tokenzier from gguf, seems candle-transformers doesn't support it yet
        let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(anyhow::Error::msg)?;

        // Peek GGUF metadata to choose device/loader
        let mut file = std::fs::File::open(&model_path)?;
        let ct = gguf_file::Content::read(&mut file).map_err(|e| e.with_path(&model_path))?;
        let metadata = ct.metadata.clone();
        let md_get = |s: &str| {
            metadata
                .get(s)
                .ok_or_else(|| anyhow::anyhow!("missing GGUF metadata key `{s}`"))
        };
        let arch = md_get("general.architecture")?.to_string()?;
        let chat_template = md_get("tokenizer.ggml.chat_template")
            .or_else(|_| md_get("tokenizer.chat_template"))?
            .to_string()?;
        let bos_token_id = md_get("tokenizer.ggml.bos_token_id")?.to_u32()?;
        let eos_token_id = md_get("tokenizer.ggml.eos_token_id")?.to_u32()?;

        // The gguf metadata for Sakura1.5bQwen2.5v1.0 has wrong eos_token_id, override it here
        let eos_token_id = match id {
            ModelId::Sakura1_5bQwen2_5v1_0 => 151645,
            _ => eos_token_id,
        };

        let device = device(use_cpu)?;

        let bos_token = tokenizer
            .id_to_token(bos_token_id)
            .unwrap_or_else(|| bos_token_id.to_string());
        let eos_token = tokenizer
            .id_to_token(eos_token_id)
            .unwrap_or_else(|| eos_token_id.to_string());
        let prompt_renderer = PromptRenderer::new(id, chat_template.clone(), bos_token, eos_token);

        // Rewind reader before loading tensors
        file.rewind()?;

        // Load quantized model for the chosen architecture
        let model = match arch.as_str() {
            "llama" => Model::Llama(quantized_llama::ModelWeights::from_gguf(
                ct, &mut file, &device,
            )?),
            "qwen2" => Model::Qwen2(quantized_qwen2::ModelWeights::from_gguf(
                ct, &mut file, &device,
            )?),
            "lfm2" => Model::Lfm2(quantized_lfm2::ModelWeights::from_gguf(
                ct, &mut file, &device,
            )?),
            _ => anyhow::bail!("unsupported model architecture: {}", arch),
        };

        Ok(Self {
            device,
            model,
            tokenizer,
            prompt_renderer,
            eos_token_id,
        })
    }

    /// Generate up to `max_tokens` following `prompt` using temperature/top-k/p settings.
    /// Logs simple performance metrics via `tracing`.
    pub fn generate(&mut self, prompt: &str, opts: &GenerateOptions) -> Result<String> {
        let prompt = self
            .prompt_renderer
            .format_chat_prompt(prompt.to_string())?;
        tracing::info!("Generating with prompt:\n{}", prompt);

        // Encode prompt
        let enc = self
            .tokenizer
            .encode(prompt, true)
            .map_err(anyhow::Error::msg)?;
        let prompt_tokens: Vec<u32> = enc.get_ids().to_vec();
        let mut all_tokens: Vec<u32> = Vec::new();

        // Build sampler
        let mut logits_processor = {
            let temperature = opts.temperature;
            let sampling = if temperature <= 0.0 {
                Sampling::ArgMax
            } else {
                match (opts.top_k, opts.top_p) {
                    (None, None) => Sampling::All { temperature },
                    (Some(k), None) => Sampling::TopK { k, temperature },
                    (None, Some(p)) => Sampling::TopP { p, temperature },
                    (Some(k), Some(p)) => Sampling::TopKThenTopP { k, p, temperature },
                }
            };
            LogitsProcessor::from_sampling(opts.seed, sampling)
        };

        // Process prompt (all at once or token by token)
        let start_prompt_processing = std::time::Instant::now();
        let mut next_token = if !opts.split_prompt {
            let input = Tensor::new(prompt_tokens.as_slice(), &self.device)?.unsqueeze(0)?;
            let logits = self.model.forward(&input, 0)?.squeeze(0)?;
            logits_processor.sample(&logits)?
        } else {
            let mut next_token = 0u32;
            for (pos, token) in prompt_tokens.iter().enumerate() {
                let input = Tensor::new(&[*token], &self.device)?.unsqueeze(0)?;
                let logits = self.model.forward(&input, pos)?.squeeze(0)?;
                next_token = logits_processor.sample(&logits)?;
            }
            next_token
        };
        let prompt_dt = start_prompt_processing.elapsed();

        if next_token == self.eos_token_id {
            tracing::warn!("Early stopping: EOS token generated at end of prompt");
            return Ok("".to_string());
        }

        all_tokens.push(next_token);
        // Generate tokens autoregressively
        let start_post_prompt = std::time::Instant::now();
        let mut sampled = 0usize;
        for index in 0..opts.max_tokens.saturating_sub(1) {
            let input = Tensor::new(&[next_token], &self.device)?.unsqueeze(0)?;
            let logits = self
                .model
                .forward(&input, prompt_tokens.len() + index)?
                .squeeze(0)?;
            let logits = if (opts.repeat_penalty - 1.0).abs() < f32::EPSILON {
                logits
            } else {
                let start_at = all_tokens.len().saturating_sub(opts.repeat_last_n);
                candle_transformers::utils::apply_repeat_penalty(
                    &logits,
                    opts.repeat_penalty,
                    &all_tokens[start_at..],
                )?
            };
            next_token = logits_processor.sample(&logits)?;
            all_tokens.push(next_token);
            sampled += 1;
            if next_token == self.eos_token_id {
                break;
            }
        }
        let gen_dt = start_post_prompt.elapsed();

        tracing::info!(
            "{:4} prompt tokens processed: {:.2} token/s",
            prompt_tokens.len(),
            if prompt_dt.as_secs_f64() > 0.0 {
                prompt_tokens.len() as f64 / prompt_dt.as_secs_f64()
            } else {
                0.0
            }
        );
        tracing::info!(
            "{:<4} tokens generated: {:.2} token/s",
            sampled,
            if gen_dt.as_secs_f64() > 0.0 {
                sampled as f64 / gen_dt.as_secs_f64()
            } else {
                0.0
            }
        );

        self.tokenizer
            .decode(&all_tokens, true)
            .map_err(anyhow::Error::msg)
    }
}
