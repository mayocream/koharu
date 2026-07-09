use std::{
    io::{self, Read, Write},
    num::NonZeroU32,
    path::PathBuf,
};

use anyhow::{Context, bail};
use clap::{Parser, ValueEnum};
use koharu_llama::{LogOptions, send_logs_to_tracing};
use koharu_llm::{
    BuiltinModel, ChatMessage, GenerationControl, GenerationOptions, LlamaRuntime, LlmBuilder,
    LoadOptions, ModelSource,
};
use strum::IntoEnumIterator;

#[derive(Parser, Debug)]
#[command(author, version, about = "Run local GGUF LLM inference with llama.cpp")]
struct Args {
    /// Prompt text. If omitted, stdin is read until EOF.
    prompt: Option<String>,

    /// Built-in model ID.
    #[arg(long, default_value = "lfm2.5-1.2b-instruct")]
    model: BuiltinModel,

    /// Hugging Face repository, e.g. unsloth/Qwen3.5-0.8B-GGUF.
    #[arg(long)]
    repo: Option<String>,

    /// Hugging Face GGUF filename inside --repo.
    #[arg(long, alias = "filename")]
    file: Option<String>,

    /// Local GGUF model path.
    #[arg(long)]
    path: Option<PathBuf>,

    /// Print built-in model catalog and exit.
    #[arg(long)]
    list_builtins: bool,

    /// Show llama.cpp and ggml runtime logs.
    #[arg(long)]
    verbose: bool,

    /// Render input through the model chat template.
    #[arg(long)]
    chat: bool,

    /// System message used with --chat.
    #[arg(long)]
    system: Option<String>,

    /// Force CPU-only model loading.
    #[arg(long)]
    cpu: bool,

    /// llama.cpp runtime package to preload.
    #[arg(long = "llama-runtime", alias = "runtime", value_enum, default_value_t = LlamaRuntimeArg::Auto)]
    llama_runtime: LlamaRuntimeArg,

    /// Number of model layers to offload when GPU offload is available.
    #[arg(long, default_value_t = 1000)]
    gpu_layers: u32,

    /// Maximum number of new tokens.
    #[arg(long, default_value_t = 512)]
    max_tokens: usize,

    /// Sampling temperature. Use 0 for greedy decoding.
    #[arg(long, default_value_t = 0.1)]
    temperature: f32,

    /// Top-k sampling cutoff.
    #[arg(long)]
    top_k: Option<usize>,

    /// Top-p nucleus sampling cutoff.
    #[arg(long)]
    top_p: Option<f32>,

    /// Min-p sampling cutoff.
    #[arg(long)]
    min_p: Option<f32>,

    /// Sampling seed.
    #[arg(long, default_value_t = 299_792_458)]
    seed: u32,

    /// Repeat penalty. 1.0 disables it.
    #[arg(long, default_value_t = 1.1)]
    repeat_penalty: f32,

    /// Number of last tokens considered by repeat/frequency/presence penalties.
    #[arg(long, default_value_t = 64)]
    repeat_last_n: usize,

    /// Frequency penalty.
    #[arg(long, default_value_t = 0.0)]
    frequency_penalty: f32,

    /// Presence penalty.
    #[arg(long, default_value_t = 0.0)]
    presence_penalty: f32,

    /// Process prompt one token at a time.
    #[arg(long)]
    split_prompt: bool,

    /// Explicit context size.
    #[arg(long)]
    n_ctx: Option<u32>,

    /// Logical batch size.
    #[arg(long)]
    n_batch: Option<u32>,

    /// Physical batch size.
    #[arg(long)]
    n_ubatch: Option<u32>,

    /// Decode threads.
    #[arg(long)]
    n_threads: Option<i32>,

    /// Prompt/batch processing threads.
    #[arg(long)]
    n_threads_batch: Option<i32>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let args = Args::parse();
    if args.list_builtins {
        list_builtins();
        return Ok(());
    }
    configure_llama_logs(args.verbose);

    let source = source_from_args(&args)?;
    let prompt = prompt_from_args(&args)?;
    let load_options = LoadOptions {
        runtime: args.llama_runtime.into(),
        cpu_only: args.cpu,
        gpu_layers: args.gpu_layers,
        ..Default::default()
    };
    let mut generation_options = source.default_generation_options();
    apply_generation_args(&mut generation_options, &args)?;

    let mut model = LlmBuilder::new(source)
        .with_load_options(load_options)
        .load()
        .await?;

    let generation = if args.chat || args.system.is_some() {
        let mut messages = Vec::new();
        if let Some(system) = args.system {
            messages.push(ChatMessage::system(system));
        }
        messages.push(ChatMessage::user(prompt));
        model.chat_with_callback(&messages, &generation_options, print_chunk)?
    } else {
        model.generate_with_callback(&prompt, &generation_options, print_chunk)?
    };

    let mut stdout = io::stdout().lock();
    writeln!(stdout)?;
    writeln!(
        stdout,
        "[{} generated tokens, {:.2} tok/s, finish: {:?}]",
        generation.generated_tokens,
        generation.generated_tokens_per_second(),
        generation.finish_reason
    )?;

    Ok(())
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum LlamaRuntimeArg {
    Auto,
    Cpu,
    Cuda,
    Vulkan,
}

impl From<LlamaRuntimeArg> for LlamaRuntime {
    fn from(value: LlamaRuntimeArg) -> Self {
        match value {
            LlamaRuntimeArg::Auto => Self::Auto,
            LlamaRuntimeArg::Cpu => Self::Cpu,
            LlamaRuntimeArg::Cuda => Self::Cuda,
            LlamaRuntimeArg::Vulkan => Self::Vulkan,
        }
    }
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn configure_llama_logs(verbose: bool) {
    send_logs_to_tracing(LogOptions::default().with_logs_enabled(verbose));
}

fn source_from_args(args: &Args) -> anyhow::Result<ModelSource> {
    match (&args.path, &args.repo, &args.file) {
        (Some(path), None, None) => Ok(ModelSource::path(path)),
        (None, Some(repo), Some(file)) => Ok(ModelSource::huggingface(repo, file)),
        (None, None, None) => Ok(ModelSource::builtin(args.model)),
        (None, Some(_), None) => bail!("--repo requires --file"),
        (None, None, Some(_)) => bail!("--file requires --repo"),
        (Some(_), _, _) => bail!("--path cannot be combined with --repo/--file"),
    }
}

fn prompt_from_args(args: &Args) -> anyhow::Result<String> {
    if let Some(prompt) = &args.prompt {
        return Ok(prompt.clone());
    }

    let mut prompt = String::new();
    io::stdin()
        .read_to_string(&mut prompt)
        .context("failed to read prompt from stdin")?;
    if prompt.trim().is_empty() {
        bail!("prompt is empty");
    }
    Ok(prompt)
}

fn apply_generation_args(options: &mut GenerationOptions, args: &Args) -> anyhow::Result<()> {
    options.max_tokens = args.max_tokens;
    options.temperature = args.temperature;
    options.top_k = args.top_k;
    options.top_p = args.top_p;
    options.min_p = args.min_p;
    options.seed = args.seed;
    options.repeat_penalty = args.repeat_penalty;
    options.repeat_last_n = args.repeat_last_n;
    options.frequency_penalty = args.frequency_penalty;
    options.presence_penalty = args.presence_penalty;
    options.split_prompt = args.split_prompt;
    options.n_ctx = args
        .n_ctx
        .map(|value| {
            NonZeroU32::new(value).ok_or_else(|| anyhow::anyhow!("--n-ctx must be non-zero"))
        })
        .transpose()?;
    options.n_batch = args.n_batch;
    options.n_ubatch = args.n_ubatch;
    options.n_threads = args.n_threads;
    options.n_threads_batch = args.n_threads_batch;
    Ok(())
}

fn print_chunk(chunk: koharu_llm::TokenChunk<'_>) -> anyhow::Result<GenerationControl> {
    let mut stdout = io::stdout().lock();
    write!(stdout, "{}", chunk.piece)?;
    stdout.flush()?;
    Ok(GenerationControl::Continue)
}

fn list_builtins() {
    for model in BuiltinModel::iter() {
        println!(
            "{model}\n  repo: {}\n  file: {}\n  languages: {}",
            model.repo(),
            model.filename(),
            model.languages()
        );
    }
}
