use clap::Parser;
use llm::{ChatMessage, GenerateOptions, Llm, ModelId};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Prompt to generate from
    #[arg(long, default_value = "Hello")]
    prompt: String,

    /// Model to use
    #[arg(long, default_value = "qwen2-1.5b-it")]
    model: ModelId,

    /// Max new tokens
    #[arg(long, default_value_t = 1000)]
    max_tokens: usize,

    /// Temperature (0 = greedy)
    #[arg(long, default_value_t = 0.8)]
    temperature: f64,

    /// Top-k (optional)
    #[arg(long)]
    top_k: Option<usize>,

    /// Top-p (optional)
    #[arg(long)]
    top_p: Option<f64>,

    /// PRNG seed
    #[arg(long, default_value_t = 299792458)]
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

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut llm = Llm::from_pretrained(args.model)?;

    let opts = GenerateOptions {
        max_tokens: args.max_tokens,
        temperature: args.temperature,
        top_k: args.top_k,
        top_p: args.top_p,
        seed: args.seed,
        split_prompt: args.split_prompt,
        repeat_penalty: args.repeat_penalty,
        repeat_last_n: args.repeat_last_n,
    };

    let out = llm.generate(&[ChatMessage::new(llm::ChatRole::User, args.prompt)], &opts)?;

    println!("{}", out);
    Ok(())
}
