use clap::Parser;
use koharu_ml::{
    llm::{GenerateOptions, Llm, ModelId},
    set_default_locale,
};
use tracing_subscriber::fmt::format::FmtSpan;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Prompt to generate from
    #[arg(
        long,
        default_value = r#"「え、マジで!?」
俺、田中太郎は気がつくと見知らぬ森の中にいた。目の前には巨大な魔獣が牙を剥いている。
「やばい、死ぬ!」
その瞬間、俺の手から青白い光が放たれた。魔獣は一瞬で消滅する。
『レベルアップしました。新スキル「爆炎魔法」を習得』
頭の中に謎の声が響く。どうやら俺、チート能力を手に入れたらしい。
「この世界で、俺は最強になる!」
こうして俺の異世界ライフが始まった。美少女との出会いも、もちろん待っている。"#
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
    #[arg(long, default_value_t = 1.0)]
    repeat_penalty: f32,

    /// Context size considered for the repeat penalty
    #[arg(long, default_value_t = 64)]
    repeat_last_n: usize,

    #[arg(long, default_value_t = false)]
    cpu: bool,

    /// override locale for translation models
    #[arg(long, default_value = "zh-CN")]
    locale: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .init();

    let args = Args::parse();

    set_default_locale(args.locale.clone());

    let mut llm = Llm::load(args.model, args.cpu).await?;

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

    let out = llm.generate(&args.prompt, &opts)?;

    println!("{}", out);
    println!(
        "** Out has {} lines",
        out.lines().filter(|l| !l.trim().is_empty()).count()
    );
    Ok(())
}
