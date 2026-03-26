use clap::Parser;
use tracing_subscriber::fmt::format::FmtSpan;

use koharu_llm::{GenerateOptions, Language, Llm, ModelId};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Prompt to generate from
    #[arg(
        long,
        default_value = r#"ã€Œãˆã€ãƒžã‚¸ã§!?ã€
ä¿ºã€ç”°ä¸å¤ªéƒŽã¯æ°—ãŒã¤ãã¨è¦‹çŸ¥ã‚‰ã¬æ£®ã®ä¸ã«ã„ãŸã€‚ç›®ã®å‰ã«ã¯å·¨å¤§ãªé”ç£ãŒç‰™ã‚’å‰¥ã„ã¦ã„ã‚‹ã€‚
ã€Œã‚„ã°ã„ã€æ»ã¬!ã€
ãã®çž¬é–“ã€ä¿ºã®æ‰‹ã‹ã‚‰é’ç™½ã„å…‰ãŒæ”¾ãŸã‚ŒãŸã€‚é”ç£ã¯ä¸€çž¬ã§æ¶ˆæ»…ã™ã‚‹ã€‚
ã€Žãƒ¬ãƒ™ãƒ«ã‚¢ãƒƒãƒ—ã—ã¾ã—ãŸã€‚æ–°ã‚¹ã‚ãƒ«ã€Œçˆ†ç‚Žé”æ³•ã€ã‚’ç¿’å¾—ã€
é ã®ä¸ã«è¬Žã®å£°ãŒéŸ¿ãã€‚ã©ã†ã‚„ã‚‰ä¿ºã€ãƒãƒ¼ãƒˆèƒ½åŠ›ã‚’æ‰‹ã«å…¥ã‚ŒãŸã‚‰ã—ã„ã€‚
ã€Œã“ã®ä¸–ç•Œã§ã€ä¿ºã¯æœ€å¼·ã«ãªã‚‹!ã€
ã“ã†ã—ã¦ä¿ºã®ç•°ä¸–ç•Œãƒ©ã‚¤ãƒ•ãŒå§‹ã¾ã£ãŸã€‚ç¾Žå°‘å¥³ã¨ã®å‡ºä¼šã„ã‚‚ã€ã‚‚ã¡ã‚ã‚“å¾…ã£ã¦ã„ã‚‹ã€‚"#
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

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .init();
}

async fn initialize_runtime() -> anyhow::Result<()> {
    koharu_runtime::initialize().await?;
    koharu_llm::sys::initialize()?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let args = Args::parse();
    initialize_runtime().await?;

    let mut llm = Llm::load(args.model, args.cpu).await?;
    let target_language = Language::parse(&args.locale).unwrap_or(Language::English);

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

    let out = llm.generate(&args.prompt, &opts, target_language)?;

    println!("{}", out);
    println!(
        "** Out has {} lines",
        out.lines().filter(|l| !l.trim().is_empty()).count()
    );
    Ok(())
}
