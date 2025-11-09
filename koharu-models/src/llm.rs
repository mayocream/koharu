use std::io::Seek;
use std::path::Path;

use anyhow::Result;
use candle_core::quantized::gguf_file;
use candle_core::utils::cuda_is_available;
use candle_core::{Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::{quantized_gemma3, quantized_qwen2};
use koharu_core::download;
use strum::{Display, EnumIter, EnumString, IntoEnumIterator};
use tokenizers::Tokenizer;

/// Supported model identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, Display, EnumIter)]
pub enum ModelId {
    #[strum(serialize = "gemma-3-4b-it")]
    Gemma3_4BInstruct,
    #[strum(serialize = "qwen2-1.5b-it")]
    Qwen2_1_5BInstruct,
    #[strum(serialize = "sakura-1.5b-qwen2.5-1.0")]
    Sakura1_5BQwen2_5_1_0,
}

impl ModelId {
    pub fn all() -> Vec<Self> {
        Self::iter().collect()
    }
}

#[derive(Debug, Clone)]
struct ModelConfig {
    /// HF model repo hosting the `.gguf` file
    repo: &'static str,
    /// File name of the GGUF within the repo
    filename: &'static str,
    /// HF repo that contains `tokenizer.json`
    tokenizer_repo: &'static str,
}

impl ModelId {
    const fn config(&self) -> ModelConfig {
        match self {
            ModelId::Gemma3_4BInstruct => ModelConfig {
                repo: "google/gemma-3-4b-it-qat-q4_0-gguf",
                filename: "gemma-3-4b-it-q4_0.gguf",
                tokenizer_repo: "google/gemma-3-4b-it",
            },
            ModelId::Qwen2_1_5BInstruct => ModelConfig {
                repo: "Qwen/Qwen2-1.5B-Instruct-GGUF",
                filename: "qwen2-1_5b-instruct-q4_0.gguf",
                tokenizer_repo: "Qwen/Qwen2-1.5B-Instruct",
            },
            ModelId::Sakura1_5BQwen2_5_1_0 => ModelConfig {
                repo: "SakuraLLM/Sakura-1.5B-Qwen2.5-v1.0-GGUF",
                filename: "sakura-1.5b-qwen2.5-v1.0-fp16.gguf",
                tokenizer_repo: "Qwen/Qwen2.5-1.5B-Instruct",
            },
        }
    }
}

/// Supported model architectures
enum Model {
    Gemma3(quantized_gemma3::ModelWeights),
    Qwen2(quantized_qwen2::ModelWeights),
}

impl Model {
    fn forward(&mut self, input: &Tensor, pos: usize) -> candle_core::Result<Tensor> {
        match self {
            Model::Gemma3(m) => m.forward(input, pos),
            Model::Qwen2(m) => m.forward(input, pos),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Markers {
    start: &'static str,
    end: &'static str,
    assistant_start: &'static str,
}

impl Model {
    fn markers(&self) -> Markers {
        match self {
            Model::Gemma3(_) => Markers {
                start: "<start_of_turn>",
                end: "<end_of_turn>",
                assistant_start: "<start_of_turn>assistant\n",
            },
            Model::Qwen2(_) => Markers {
                start: "<|im_start|>",
                end: "<|im_end|>",
                assistant_start: "<|im_start|>assistant\n",
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    role: ChatRole,
    content: String,
}

impl ChatMessage {
    pub fn new(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

/// Minimal quantized LLM wrapper
pub struct Llm {
    device: Device,
    model: Model,
    tokenizer: Tokenizer,
    eos_token: u32,
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
    /// Loads a quantized model from Hugging Face based on the given identifier.
    /// Downloads artifacts if necessary, following Candle examples' pattern.
    pub async fn from_pretrained(which: ModelId) -> Result<Self> {
        let cfg = which.config();
        let model_path = download::hf_hub(cfg.repo, cfg.filename).await?;
        let tokenizer_path = download::hf_hub(cfg.tokenizer_repo, "tokenizer.json").await?;
        Self::new(model_path, tokenizer_path)
    }

    /// Constructs a new LLM instance from a quantized GGUF model and tokenizer.json.
    pub fn new(model_path: impl AsRef<Path>, tokenizer_path: impl AsRef<Path>) -> Result<Self> {
        // Load tokenizer
        let tokenizer =
            Tokenizer::from_file(tokenizer_path.as_ref()).map_err(anyhow::Error::msg)?;

        // Peek GGUF metadata to choose device/loader
        let mut file = std::fs::File::open(model_path.as_ref())?;
        let ct =
            gguf_file::Content::read(&mut file).map_err(|e| e.with_path(model_path.as_ref()))?;
        let arch = ct
            .metadata
            .get("general.architecture")
            .and_then(|v| v.to_string().ok())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        let device = device()?;

        // Rewind reader before loading tensors
        file.rewind()?;

        // Load quantized model for the chosen architecture
        let model = match arch.as_str() {
            "gemma3" => Model::Gemma3(quantized_gemma3::ModelWeights::from_gguf(
                ct, &mut file, &device,
            )?),
            "qwen2" => Model::Qwen2(quantized_qwen2::ModelWeights::from_gguf(
                ct, &mut file, &device,
            )?),
            _ => anyhow::bail!("unsupported model architecture: {}", arch),
        };

        // Prefer the model's end-turn marker as EOS if it exists as a single special token;
        // fall back to a few common alternatives.
        let eos_token = {
            let markers = model.markers();
            let try_marker = |tok: &Tokenizer, s: &str| -> Option<u32> {
                let enc = tok.encode(s, true).ok()?;
                let ids = enc.get_ids();
                if ids.len() == 1 { Some(ids[0]) } else { None }
            };
            try_marker(&tokenizer, markers.end)
                .or_else(|| try_marker(&tokenizer, "<end_of_turn>"))
                .or_else(|| tokenizer.get_vocab(true).get("<eos>").cloned())
                .or_else(|| tokenizer.get_vocab(true).get("</s>").cloned())
                .or_else(|| tokenizer.get_vocab(true).get("<|im_end|>").cloned())
                .unwrap_or(2)
        };

        Ok(Self {
            device,
            model,
            tokenizer,
            eos_token,
        })
    }

    /// Generate up to `max_tokens` following `prompt` using temperature/top-k/p settings.
    /// Logs simple performance metrics via `tracing`.
    pub fn generate(&mut self, prompt: &[ChatMessage], opts: &GenerateOptions) -> Result<String> {
        let prompt = self.format_chat_prompt(prompt);

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
        all_tokens.push(next_token);

        // If EOS after prompt, log metrics and return.
        if next_token == self.eos_token {
            tracing::info!(
                "{:4} prompt tokens processed: {:.2} token/s",
                prompt_tokens.len(),
                if prompt_dt.as_secs_f64() > 0.0 {
                    prompt_tokens.len() as f64 / prompt_dt.as_secs_f64()
                } else {
                    0.0
                }
            );
            return self
                .tokenizer
                .decode(&all_tokens, true)
                .map_err(anyhow::Error::msg);
        }

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
            if next_token == self.eos_token {
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

    fn format_chat_prompt(&self, messages: &[ChatMessage]) -> String {
        let markers = self.model.markers();
        let mut out = String::new();

        for msg in messages {
            out.push_str(markers.start);
            out.push_str(msg.role.to_string().as_ref());
            out.push('\n');
            out.push_str(msg.content.as_str());
            out.push_str(markers.end);
            out.push('\n');
        }

        // If the last message isn't an assistant turn, open one to prompt generation.
        if !messages
            .last()
            .is_some_and(|m| m.role == ChatRole::Assistant)
        {
            out.push_str(markers.assistant_start);
        }
        out
    }
}

// refer: https://github.com/huggingface/candle/blob/d4545ebbbfb37d3cf0e228642ffaaa75b5d6bce9/candle-examples/src/lib.rs#L10
pub fn device() -> Result<Device> {
    if cuda_is_available() {
        Ok(Device::new_cuda(0)?)
    } else {
        tracing::info!("Running on CPU, to run on GPU, build with `--features cuda`");
        Ok(Device::Cpu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device() {
        let device = device();
        assert!(device.is_ok());
    }

    #[tokio::test]
    async fn test_translation_task() {
        let mut llm = Llm::from_pretrained(ModelId::Sakura1_5BQwen2_5_1_0)
            .await
            .unwrap();
        let messages = vec![
            ChatMessage::new(
                ChatRole::System,
                "你是一个轻小说翻译模型，可以流畅通顺地以日本轻小说的风格将日文翻译成简体中文，并联系上下文正确使用人称代词，不擅自添加原文中没有的代词。",
            ),
            ChatMessage::new(
                ChatRole::User,
                "彼は静かに微笑んだ。「君の言う通りだ。私たちは共に戦うべきだ」",
            ),
        ];

        let response = llm
            .generate(&messages, &GenerateOptions::default())
            .unwrap();

        println!("Translation: {}", response);
    }
}
