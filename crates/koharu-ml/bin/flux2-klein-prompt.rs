use std::{path::PathBuf, time::Instant};

use anyhow::{Context, Result, bail};
use candle_core::{DType, Device};
use clap::Parser;
use koharu_ml::flux2_klein::qwen::{PromptEmbedder, QwenTextEncoder};
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};

#[path = "common.rs"]
mod common;

const QWEN_REPO: &str = "unsloth/Qwen3-4B-GGUF";
const QWEN_GGUF: &str = "Qwen3-4B-Q4_K_M.gguf";
const QWEN_TOKENIZER_REPO: &str = "Qwen/Qwen3-4B";
const QWEN_TOKENIZER: &str = "tokenizer.json";

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    prompt: String,

    #[arg(
        short,
        long,
        value_name = "FILE",
        default_value = "koharu-ml/src/flux2_klein/prompt.safetensors"
    )]
    output: PathBuf,

    #[arg(long, value_name = "FILE")]
    qwen_path: Option<PathBuf>,

    #[arg(long, value_name = "FILE")]
    tokenizer_path: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    common::init_tracing();

    let cli = Cli::parse();
    let runtime = RuntimeManager::new(default_app_data_root(), ComputePolicy::PreferGpu)?;
    runtime.prepare().await?;

    let (qwen_path, tokenizer_path) = qwen_paths(&runtime, &cli).await?;
    let device = koharu_ml::device(false)?;

    let start = Instant::now();
    let qwen = QwenTextEncoder::from_gguf(&qwen_path, &device)
        .with_context(|| format!("failed to load Qwen3 from {}", qwen_path.display()))?;
    let embedder = PromptEmbedder::new(&tokenizer_path, qwen)?;
    let embeddings = embedder.encode_prompt(&cli.prompt)?;
    embeddings
        .prompt_embeds
        .to_device(&Device::Cpu)?
        .to_dtype(DType::F32)?
        .save_safetensors("prompt_embeds", &cli.output)?;
    println!("Prompt safetensors wrote in {:?}", start.elapsed());

    Ok(())
}

async fn qwen_paths(runtime: &RuntimeManager, cli: &Cli) -> Result<(PathBuf, PathBuf)> {
    match (&cli.qwen_path, &cli.tokenizer_path) {
        (Some(qwen_path), Some(tokenizer_path)) => Ok((qwen_path.clone(), tokenizer_path.clone())),
        (None, None) => {
            let qwen_path = runtime
                .downloads()
                .huggingface_model(QWEN_REPO, QWEN_GGUF)
                .await?;
            let tokenizer_path = runtime
                .downloads()
                .huggingface_model(QWEN_TOKENIZER_REPO, QWEN_TOKENIZER)
                .await?;
            Ok((qwen_path, tokenizer_path))
        }
        _ => bail!("--qwen-path and --tokenizer-path must be provided together"),
    }
}
