use anyhow::Result;
use clap::Parser;
use llm::{Llm, LlmConfig};

#[derive(Parser, Debug)]
#[command(author, version, about = "Gemma 3 GGUF runner (GPU only, Candle)")]
struct Args {
    /// Prompt to generate from
    #[arg(long, default_value = "You are Gemma 3. Briefly introduce yourself.")]
    prompt: String,

    /// Override: HF repo that contains the GGUF
    #[arg(long)]
    gguf_repo: Option<String>,

    /// Override: filename of the GGUF in the repo
    #[arg(long)]
    gguf_filename: Option<String>,

    /// Override: local path to a GGUF file (skips download)
    #[arg(long)]
    gguf_path: Option<String>,

    /// Override: HF repo with tokenizer.json
    #[arg(long)]
    tokenizer_repo: Option<String>,

    /// Max new tokens
    #[arg(long, default_value_t = 256)]
    max_tokens: usize,

    /// Temperature (0 = greedy)
    #[arg(long, default_value_t = 0.7)]
    temperature: f64,

    /// Top-k (optional)
    #[arg(long)]
    top_k: Option<usize>,

    /// Top-p (optional)
    #[arg(long)]
    top_p: Option<f64>,

    /// PRNG seed
    #[arg(long, default_value_t = 42)]
    seed: u64,

    /// Process prompt elements separately (follows example behavior)
    #[arg(long, default_value_t = false)]
    split_prompt: bool,

    /// Penalty to be applied for repeating tokens (1.0 = no penalty)
    #[arg(long, default_value_t = 1.1)]
    repeat_penalty: f32,

    /// Context size considered for the repeat penalty
    #[arg(long, default_value_t = 64)]
    repeat_last_n: usize,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut cfg = LlmConfig::default();
    if let Some(v) = args.gguf_repo {
        cfg.gguf_repo = v;
    }
    if let Some(v) = args.gguf_filename {
        cfg.gguf_filename = v;
    }
    if let Some(v) = args.tokenizer_repo {
        cfg.tokenizer_repo = v;
    }
    if let Some(v) = args.gguf_path {
        cfg.gguf_path = Some(v.into());
    }
    eprintln!(
        "Using\n  gguf_repo      = {}\n  gguf_filename  = {}\n  tokenizer_repo = {}\n  device         = GPU (CUDA)",
        cfg.gguf_repo, cfg.gguf_filename, cfg.tokenizer_repo,
    );

    let mut llm = Llm::new(cfg)?;
    let out = llm.generate(
        &args.prompt,
        args.max_tokens,
        args.temperature,
        args.top_k,
        args.top_p,
        args.seed,
        args.split_prompt,
        args.repeat_penalty,
        args.repeat_last_n,
    )?;
    println!("{}", out);
    Ok(())
}
