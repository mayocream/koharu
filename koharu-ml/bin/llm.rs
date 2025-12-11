use clap::Parser;
use koharu_ml::llm::{GenerateOptions, Llm, ModelId};
use tracing_subscriber::fmt::format::FmtSpan;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Prompt to generate from
    #[arg(
        long,
        default_value = "吾輩は猫である。\n名前はまだ無い。\nどこで生れたかとんと見当がつかぬ。\n何でも薄暗いじめじめした所でニャーニャー泣いていた事だけは記憶している。\n吾輩はここで始めて人間というものを見た。\nしかもあとで聞くとそれは書生という人間中で一番獰悪な種族であったそうだ。"
    )]
    prompt: String,

    /// Model to use
    #[arg(long, default_value = "sakura-galtransl-7b-v3.7")]
    model: ModelId,

    /// Max new tokens
    #[arg(long, default_value_t = 1000)]
    max_tokens: usize,

    /// Temperature (0 = greedy)
    #[arg(long, default_value_t = 0.3)]
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .init();

    let args = Args::parse();

    let mut llm = Llm::load(args.model).await?;

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

    let messages = args.model.prompt(args.prompt.as_str());

    let out = llm.generate(&messages, &opts)?;

    println!("{}", out);
    Ok(())
}
