use clap::Parser;
use koharu_ml::llm::{ChatMessage, ChatRole, GenerateOptions, Llm, ModelId};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Prompt to generate from
    #[arg(
        long,
        default_value = "「吾輩は猫である。名前はまだ無い。どこで生れたかとんと見当がつかぬ。何でも薄暗いじめじめした所でニャーニャー泣いていた事だけは記憶している。吾輩はここで始めて人間というものを見た。しかもあとで聞くとそれは書生という人間中で一番獰悪な種族であったそうだ。」"
    )]
    prompt: String,

    /// Model to use
    #[arg(long, default_value = "sakura-galtransl-7b-v3.7")]
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    let messages = match args.model {
        ModelId::VntlLlama3_8Bv2 => vec![
            ChatMessage::new(ChatRole::Name("Japanese"), args.prompt),
            ChatMessage::new(ChatRole::Name("English"), String::new()),
        ],
        ModelId::SakuraGalTransl7Bv3_7 => vec![
            ChatMessage::new(
                ChatRole::System,
                "你是一个视觉小说翻译模型，可以通顺地使用给定的术语表以指定的风格将日文翻译成简体中文，并联系上下文正确使用人称代词，注意不要混淆使役态和被动态的主语和宾语，不要擅自添加原文中没有的特殊符号，也不要擅自增加或减少换行。",
            ),
            ChatMessage::new(ChatRole::User, args.prompt),
            ChatMessage::assistant(),
        ],
    };

    let out = llm.generate(&messages, &opts)?;

    println!("{}", out);
    Ok(())
}
