use anyhow::{anyhow, Result};
use candle_core::{Device, Tensor};
use candle_core::quantized::gguf_file;
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::{
    quantized_llama as qllama,
    quantized_gemma3 as qgemma3,
};
use hf_hub::{Repo, RepoType, api::sync::Api};
use tokenizers::Tokenizer;

/// Minimal quantized LLM wrapper able to download a GGUF for Gemma 3 12B and run inference.
pub struct Llm {
    device: Device,
    model: QuantModel,
    tokenizer: Tokenizer,
    eos_token_id: u32,
}

enum QuantModel {
    Llama(qllama::ModelWeights),
    Gemma3(qgemma3::ModelWeights),
}

impl QuantModel {
    fn forward(&mut self, input: &Tensor, pos: usize) -> candle_core::Result<Tensor> {
        match self {
            QuantModel::Llama(m) => m.forward(input, pos),
            QuantModel::Gemma3(m) => m.forward(input, pos),
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
    /// Use CPU even if GPU is available
    pub cpu: bool,
}

impl Default for LlmConfig {
    fn default() -> Self {
        // Defaults target Gemma 3 12B Instruct GGUF conventions.
        // Override via env: LLM_GGUF_REPO, LLM_GGUF_FILENAME, LLM_TOKENIZER_REPO.
        let gguf_repo = std::env::var("LLM_GGUF_REPO")
            .unwrap_or_else(|_| "lmstudio-community/gemma-3-12b-it-GGUF".to_string());
        let gguf_filename = std::env::var("LLM_GGUF_FILENAME")
            .unwrap_or_else(|_| "gemma-3-12b-it-Q3_K_L.gguf".to_string());
        let gguf_path = std::env::var("LLM_GGUF_PATH").ok().map(Into::into);
        let tokenizer_repo = std::env::var("LLM_TOKENIZER_REPO")
            .unwrap_or_else(|_| "google/gemma-3-12b-it".to_string());
        let cpu = std::env::var("LLM_CPU")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Self {
            gguf_repo,
            gguf_filename,
            gguf_path,
            tokenizer_repo,
            cpu,
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
            .unwrap_or_else(|| "llama".to_string());

        // Device: prefer CUDA, but Gemma3 quantized lacks CUDA RMSNorm, use CPU.
        let mut device = device(config.cpu)?;
        #[cfg(feature = "cuda")]
        if arch == "gemma3" && !config.cpu {
            // Fallback to CPU to avoid: "no cuda implementation for rms-norm"
            device = Device::Cpu;
            eprintln!(
                "[llm] Gemma 3 quantized on CUDA is not fully supported (rms-norm). Falling back to CPU."
            );
        }

        // Rewind reader before loading tensors
        use std::io::Seek;
        file.rewind()?;

        // Load quantized model for the chosen architecture
        let model = match arch.as_str() {
            "gemma3" => QuantModel::Gemma3(qgemma3::ModelWeights::from_gguf(ct, &mut file, &device)?),
            _ => QuantModel::Llama(qllama::ModelWeights::from_gguf(ct, &mut file, &device)?),
        };

        // Resolve EOS token id depending on tokenizer vocab
        // Try common candidates, otherwise fallback to 2 (often </s> in LLaMA-like).
        let vocab = tokenizer.get_vocab(true);
        let eos_token_id = vocab
            .get("<eos>")
            .or_else(|| vocab.get("</s>"))
            .or_else(|| vocab.get("<|endoftext|>"))
            .or_else(|| vocab.get("<|end_of_text|>"))
            .cloned()
            .unwrap_or(2);

        Ok(Self { device, model, tokenizer, eos_token_id })
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
    ) -> Result<String> {
        // Encode prompt
        let enc = self
            .tokenizer
            .encode(prompt, true)
            .map_err(anyhow::Error::msg)?;
        let mut all_tokens: Vec<u32> = enc.get_ids().to_vec();

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

        // Run prompt in one pass
        if !all_tokens.is_empty() {
            let input = Tensor::new(all_tokens.as_slice(), &self.device)?.unsqueeze(0)?;
            let logits = self.model.forward(&input, 0)?.squeeze(0)?;
            let next = logits_processor.sample(&logits)?;
            all_tokens.push(next);
            if next == self.eos_token_id {
                return self.decode(&all_tokens);
            }
        }

        // Autoregressive sampling
        for pos in all_tokens.len()..all_tokens.len() + max_tokens {
            let last = *all_tokens
                .last()
                .ok_or_else(|| anyhow!("empty token state"))?;
            let input = Tensor::new(&[last], &self.device)?.unsqueeze(0)?;
            let logits = self.model.forward(&input, pos - 1)?.squeeze(0)?;
            let next = logits_processor.sample(&logits)?;
            all_tokens.push(next);
            if next == self.eos_token_id {
                break;
            }
        }

        self.decode(&all_tokens)
    }

    fn decode(&self, tokens: &[u32]) -> Result<String> {
        let text = self
            .tokenizer
            .decode(tokens, true)
            .map_err(anyhow::Error::msg)?;
        Ok(text)
    }
}

fn device(force_cpu: bool) -> Result<Device> {
    if force_cpu {
        return Ok(Device::Cpu);
    }
    #[cfg(feature = "cuda")]
    if let Ok(dev) = Device::new_cuda(0) {
        return Ok(dev);
    }
    Ok(Device::Cpu)
}
