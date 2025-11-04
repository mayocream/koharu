use anyhow::{Result, anyhow};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::quantized_gemma3;
use hf_hub::{Repo, RepoType, api::sync::Api};
use tokenizers::Tokenizer;

/// Minimal quantized LLM wrapper
pub struct Llm {
    device: Device,
    model: Model,
    tokenizer: Tokenizer,
    eos_token_id: u32,
}

enum Model {
    Gemma3(quantized_gemma3::ModelWeights),
}

impl Model {
    fn forward(&mut self, input: &Tensor, pos: usize) -> candle_core::Result<Tensor> {
        match self {
            Model::Gemma3(m) => m.forward(input, pos),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LlmConfig {
    /// HF model repo hosting the `.gguf` file
    pub gguf_repo: String,
    /// File name of the GGUF within the repo
    pub gguf_filename: String,
    /// Local GGUF path (skips download when set)
    pub gguf_path: Option<std::path::PathBuf>,
    /// HF repo that contains `tokenizer.json`
    pub tokenizer_repo: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        // Defaults follow the Candle quantized Gemma 3 example.
        // Override via env: LLM_GGUF_REPO, LLM_GGUF_FILENAME, LLM_TOKENIZER_REPO.
        let gguf_repo = std::env::var("LLM_GGUF_REPO")
            .unwrap_or_else(|_| "google/gemma-3-4b-it-qat-q4_0-gguf".to_string());
        let gguf_filename = std::env::var("LLM_GGUF_FILENAME")
            .unwrap_or_else(|_| "gemma-3-4b-it-q4_0.gguf".to_string());
        let gguf_path = std::env::var("LLM_GGUF_PATH").ok().map(Into::into);
        let tokenizer_repo = std::env::var("LLM_TOKENIZER_REPO")
            .unwrap_or_else(|_| "google/gemma-3-4b-it".to_string());
        Self {
            gguf_repo,
            gguf_filename,
            gguf_path,
            tokenizer_repo,
        }
    }
}

impl Llm {
    pub fn new(config: LlmConfig) -> Result<Self> {
        // Download model and tokenizer paths
        let api = Api::new()?;
        let gguf_path = match &config.gguf_path {
            Some(local) => local.clone(),
            None => api
                .repo(Repo::new(config.gguf_repo.clone(), RepoType::Model))
                .get(&config.gguf_filename)
                .map_err(|e| anyhow!(
                    "Failed to download GGUF {}/{}: {}\n- If the model is gated (e.g., Gemma), accept terms and set HF_TOKEN.\n- Or set LLM_GGUF_PATH to a local .gguf file.\n- Or override with --gguf-repo/--gguf-filename.",
                    config.gguf_repo, config.gguf_filename, e
                ))?,
        };
        let tokenizer_path = api
            .model(config.tokenizer_repo.clone())
            .get("tokenizer.json")?;

        // Load tokenizer
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(anyhow::Error::msg)?;

        // Peek GGUF metadata to choose device/loader
        let mut file = std::fs::File::open(&gguf_path)?;
        let ct = gguf_file::Content::read(&mut file).map_err(|e| e.with_path(&gguf_path))?;
        let arch = ct
            .metadata
            .get("general.architecture")
            .and_then(|v| v.to_string().ok())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        // Only support Gemma 3 for now.
        if arch.as_str() != "gemma3" {
            return Err(anyhow!(
                "Unsupported architecture '{arch}'. This crate currently supports only Gemma 3 GGUFs."
            ));
        }

        // Device: require CUDA (no CPU inference supported here).
        let device = device_gpu()?;

        // Rewind reader before loading tensors
        use std::io::Seek;
        file.rewind()?;

        // Load quantized model for the chosen architecture
        let model = Model::Gemma3(quantized_gemma3::ModelWeights::from_gguf(
            ct, &mut file, &device,
        )?);

        // For Gemma 3, prefer <end_of_turn> as EOS; fall back to a few common alternatives.
        let vocab = tokenizer.get_vocab(true);
        let eos_token_id = vocab
            .get("<end_of_turn>")
            .or_else(|| vocab.get("<eos>"))
            .or_else(|| vocab.get("</s>"))
            .or_else(|| vocab.get("<|endoftext|>"))
            .or_else(|| vocab.get("<|end_of_text|>"))
            .cloned()
            .unwrap_or(2);

        Ok(Self {
            device,
            model,
            tokenizer,
            eos_token_id,
        })
    }

    /// Generate up to `max_tokens` following `prompt` using temperature/top-k/p settings.
    pub fn generate(
        &mut self,
        prompt: &str,
        max_tokens: usize,
        temperature: f64,
        top_k: Option<usize>,
        top_p: Option<f64>,
        seed: u64,
        split_prompt: bool,
        repeat_penalty: f32,
        repeat_last_n: usize,
    ) -> Result<String> {
        // Encode prompt
        let enc = self
            .tokenizer
            .encode(prompt, true)
            .map_err(anyhow::Error::msg)?;
        let prompt_tokens: Vec<u32> = enc.get_ids().to_vec();
        let mut all_tokens: Vec<u32> = Vec::new();

        // Build sampler
        let sampling = if temperature <= 0.0 {
            Sampling::ArgMax
        } else {
            match (top_k, top_p) {
                (None, None) => Sampling::All { temperature },
                (Some(k), None) => Sampling::TopK { k, temperature },
                (None, Some(p)) => Sampling::TopP { p, temperature },
                (Some(k), Some(p)) => Sampling::TopKThenTopP { k, p, temperature },
            }
        };
        let mut logits_processor = LogitsProcessor::from_sampling(seed, sampling);

        // Process prompt (all at once or token by token)
        let mut next_token = if !split_prompt {
            let input = Tensor::new(prompt_tokens.as_slice(), &self.device)?.unsqueeze(0)?;
            let logits = self.model.forward(&input, 0)?.squeeze(0)?;
            logits_processor.sample(&logits)?
        } else {
            let mut next = 0u32;
            for (pos, token) in prompt_tokens.iter().enumerate() {
                let input = Tensor::new(&[*token], &self.device)?.unsqueeze(0)?;
                let logits = self.model.forward(&input, pos)?.squeeze(0)?;
                next = logits_processor.sample(&logits)?;
            }
            next
        };
        all_tokens.push(next_token);
        if next_token == self.eos_token_id {
            return self.decode(&[prompt_tokens.as_slice(), all_tokens.as_slice()].concat());
        }

        // Generate tokens autoregressively
        for index in 0..max_tokens.saturating_sub(1) {
            let input = Tensor::new(&[next_token], &self.device)?.unsqueeze(0)?;
            let logits = self
                .model
                .forward(&input, prompt_tokens.len() + index)?
                .squeeze(0)?;
            let logits = if (repeat_penalty - 1.0).abs() < f32::EPSILON {
                logits
            } else {
                let start_at = all_tokens.len().saturating_sub(repeat_last_n);
                candle_transformers::utils::apply_repeat_penalty(
                    &logits,
                    repeat_penalty,
                    &all_tokens[start_at..],
                )?
            };
            next_token = logits_processor.sample(&logits)?;
            all_tokens.push(next_token);
            if next_token == self.eos_token_id {
                break;
            }
        }

        self.decode(&[prompt_tokens.as_slice(), all_tokens.as_slice()].concat())
    }

    fn decode(&self, tokens: &[u32]) -> Result<String> {
        let text = self
            .tokenizer
            .decode(tokens, true)
            .map_err(anyhow::Error::msg)?;
        Ok(text)
    }
}

fn device_gpu() -> Result<Device> {
    #[cfg(feature = "cuda")]
    {
        Device::new_cuda(0).map_err(|e| anyhow!(
            "CUDA device unavailable: {}. This crate requires GPU; enable feature 'cuda' and ensure CUDA is accessible.",
            e
        ))
    }
    #[cfg(not(feature = "cuda"))]
    {
        Err(anyhow!(
            "CUDA feature not enabled. Rebuild with `--features llm/cuda` (or workspace equivalent). CPU inference is disabled."
        ))
    }
}
