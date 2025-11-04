use std::path::PathBuf;

use anyhow::{Result, anyhow};
use candle_core::quantized::gguf_file;
use candle_core::utils::cuda_is_available;
use candle_core::{Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::quantized_gemma3;
use hf_hub::Cache;
use hf_hub::api::sync::Api;
use strum::{Display, EnumString};
use tokenizers::Tokenizer;

/// Supported models
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, Display)]
pub enum Which {
    #[strum(serialize = "gemma-3-4b-it")]
    Gemma3_4BInstruct,
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

impl Which {
    const fn config(&self) -> ModelConfig {
        match self {
            Which::Gemma3_4BInstruct => ModelConfig {
                repo: "google/gemma-3-4b-it-qat-q4_0-gguf",
                filename: "gemma-3-4b-it-q4_0.gguf",
                tokenizer_repo: "google/gemma-3-4b-it",
            },
        }
    }

    pub fn cached(&self) -> bool {
        let config = self.config();
        let cache = Cache::default();

        let model_path = cache.model(config.repo.to_string()).get(&config.filename);
        let tokenizer_path = cache
            .model(config.tokenizer_repo.to_string())
            .get("tokenizer.json");

        model_path.is_some() && tokenizer_path.is_some()
    }

    pub fn download(&self) -> Result<(PathBuf, PathBuf)> {
        let config = self.config();
        let api = Api::new()?;

        // TODO: implement progress bar
        let path = api.model(config.repo.to_string()).get(&config.filename)?;
        let tokenizer_path = api
            .model(config.tokenizer_repo.to_string())
            .get("tokenizer.json")?;
        Ok((path, tokenizer_path))
    }

    /// Load the selected model
    pub fn new(&self) -> Result<Llm> {
        let (model_path, tokenizer_path) = self.download()?;
        Llm::new(model_path, tokenizer_path)
    }
}

/// Supported model architectures
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

/// Minimal quantized LLM wrapper
pub struct Llm {
    device: Device,
    model: Model,
    tokenizer: Tokenizer,
    eos_token_id: u32,
}

impl Llm {
    pub fn new(model_path: PathBuf, tokenizer_path: PathBuf) -> Result<Self> {
        // Load tokenizer
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(anyhow::Error::msg)?;

        // Peek GGUF metadata to choose device/loader
        let mut file = std::fs::File::open(&model_path)?;
        let ct = gguf_file::Content::read(&mut file).map_err(|e| e.with_path(&model_path))?;
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

        let device = device()?;

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

// refer: https://github.com/huggingface/candle/blob/d4545ebbbfb37d3cf0e228642ffaaa75b5d6bce9/candle-examples/src/lib.rs#L10
pub fn device() -> Result<Device> {
    if cuda_is_available() {
        Ok(Device::new_cuda(0)?)
    } else {
        println!("Running on CPU, to run on GPU, build with `--features cuda`");
        Ok(Device::Cpu)
    }
}
