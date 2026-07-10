//! This is a translation of simple.cpp in llama.cpp using koharu-llm.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use koharu_llm::safe::context::params::LlamaContextParams;
use koharu_llm::safe::llama_backend::LlamaBackend;
use koharu_llm::safe::llama_batch::LlamaBatch;
use koharu_llm::safe::model::AddBos;
use koharu_llm::safe::model::LlamaModel;
use koharu_llm::safe::model::params::kv_overrides::ParamOverrideValue;
use koharu_llm::safe::model::params::{LlamaModelParams, LlamaSplitMode};
use koharu_llm::safe::sampling::LlamaSampler;
use koharu_llm::safe::{LogOptions, ggml_time_us, send_logs_to_tracing};
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};

use std::ffi::CString;
use std::io::Write;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::pin::pin;
use std::str::FromStr;
use std::time::Duration;

#[derive(clap::Parser, Debug, Clone)]
struct Args {
    /// The path to the model
    #[command(subcommand)]
    model: Model,
    /// The prompt
    #[clap(short = 'p', long)]
    prompt: Option<String>,
    /// Read the prompt from a file
    #[clap(short = 'f', long, help = "prompt file to start generation")]
    file: Option<String>,
    /// set the length of the prompt + output in tokens
    #[arg(long, default_value_t = 32)]
    n_len: i32,
    /// override some parameters of the model
    #[arg(short = 'o', value_parser = parse_key_val)]
    key_value_overrides: Vec<(String, ParamOverrideValue)>,
    /// Disable offloading layers to the gpu
    #[clap(long)]
    disable_gpu: bool,
    /// Set main GPU device index (default: 0)
    ///
    /// By setting this option, multiple GPU is disabled.
    #[arg(
        long,
        help = "Set main GPU device id (default: 0). Disables multi-GPU."
    )]
    main_gpu: Option<i32>,
    /// Set devices to use by index
    ///
    /// This option overrides `main-gpu` and enables multi-GPU.
    #[arg(
        long,
        value_delimiter = ',',
        help = "Set devices to use by index, separated by commas (e.g. --devices 0,1,2). Overrides main-gpu and enables multi-GPU."
    )]
    devices: Option<Vec<usize>>,
    #[arg(long, help = "Keep MoE layers on CPU")]
    cmoe: bool,
    #[arg(short = 's', long, help = "RNG seed (default: 1234)")]
    seed: Option<u32>,
    #[arg(
        short = 't',
        long,
        help = "number of threads to use during generation (default: use all available threads)"
    )]
    threads: Option<i32>,
    #[arg(
        long,
        help = "number of threads to use during batch and prompt processing (default: use all available threads)"
    )]
    threads_batch: Option<i32>,
    #[arg(
        short = 'c',
        long,
        help = "size of the prompt context (default: loaded from themodel)"
    )]
    ctx_size: Option<NonZeroU32>,
    #[arg(short = 'v', long, help = "enable verbose llama.cpp logs")]
    verbose: bool,
    #[arg(long, help = "list backend devices")]
    list_devices: bool,
}

/// Parse a single key-value pair
fn parse_key_val(s: &str) -> Result<(String, ParamOverrideValue)> {
    let pos = s
        .find('=')
        .ok_or_else(|| anyhow!("invalid KEY=value: no `=` found in `{s}`"))?;
    let key = s[..pos].parse()?;
    let value: String = s[pos + 1..].parse()?;
    let value = i64::from_str(&value)
        .map(ParamOverrideValue::Int)
        .or_else(|_| f64::from_str(&value).map(ParamOverrideValue::Float))
        .or_else(|_| bool::from_str(&value).map(ParamOverrideValue::Bool))
        .map_err(|_| anyhow!("must be one of i64, f64, or bool"))?;

    Ok((key, value))
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Model {
    /// Use an already downloaded model
    Local {
        /// The path to the model. e.g. `/home/marcus/.cache/huggingface/hub/models--TheBloke--Llama-2-7B-Chat-GGUF/blobs/08a5566d61d7cb6b420c3e4387a39e0078e1f2fe5f055f3a03887385304d4bfa`
        path: PathBuf,
    },
    /// Download a model from huggingface (or use a cached version)
    #[clap(name = "hf-model")]
    HuggingFace {
        /// the repo containing the model. e.g. `TheBloke/Llama-2-7B-Chat-GGUF`
        repo: String,
        /// the model name. e.g. `llama-2-7b-chat.Q4_K_M.gguf`
        model: String,
    },
}

impl Model {
    /// Convert the model to a path - may download from huggingface
    async fn get_or_load(self, runtime: &RuntimeManager) -> Result<PathBuf> {
        match self {
            Model::Local { path } => Ok(path),
            Model::HuggingFace { model, repo } => runtime
                .downloads()
                .huggingface_model(&repo, &model)
                .await
                .with_context(|| "unable to download model"),
        }
    }
}

#[allow(clippy::too_many_lines)]
fn main() -> Result<()> {
    let Args {
        n_len,
        model,
        prompt,
        file,
        disable_gpu,
        main_gpu,
        devices,
        cmoe,
        key_value_overrides,
        seed,
        threads,
        threads_batch,
        ctx_size,
        verbose,
        list_devices,
    } = Args::parse();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .with_context(|| "unable to create tokio runtime")?;
    let runtime_manager = RuntimeManager::new(
        default_app_data_root(),
        if disable_gpu {
            ComputePolicy::CpuOnly
        } else {
            ComputePolicy::PreferGpu
        },
    )?;

    runtime.block_on(async {
        runtime_manager
            .prepare()
            .await
            .context("failed to initialize runtime libraries")?;
        Ok::<(), anyhow::Error>(())
    })?;

    if verbose {
        tracing_subscriber::fmt::init();
    }
    send_logs_to_tracing(LogOptions::default().with_logs_enabled(verbose));

    // init LLM
    let backend = LlamaBackend::init()?;

    if list_devices {
        let devices = koharu_llm::safe::list_llama_ggml_backend_devices();
        for (i, dev) in devices.iter().enumerate() {
            println!("Device {i:>2}: {}", dev.name);
            println!("           Description: {}", dev.description);
            println!("           Device Type: {:?}", dev.device_type);
            println!("           Backend: {}", dev.backend);
            println!(
                "           Memory total: {:?} MiB",
                dev.memory_total / 1024 / 1024
            );
            println!(
                "           Memory free:  {:?} MiB",
                dev.memory_free / 1024 / 1024
            );
        }
    }

    // offload all layers to the gpu
    let gpu_offload_enabled = backend.supports_gpu_offload() && !disable_gpu;
    let mut model_params = if gpu_offload_enabled {
        LlamaModelParams::default().with_n_gpu_layers(1000)
    } else {
        LlamaModelParams::default()
    };

    if let Some(devices) = devices {
        model_params = model_params
            .with_devices(&devices)
            .with_context(|| "invalid device index in --devices")?;
        if main_gpu.is_some() {
            eprintln!("warning: --devices overrides --main-gpu");
        }
    } else if let Some(main_gpu) = main_gpu {
        model_params = model_params.with_main_gpu(main_gpu);
        // Enable single GPU mode
        model_params = model_params.with_split_mode(LlamaSplitMode::None);
    }

    let prompt = if let Some(str) = prompt {
        if file.is_some() {
            bail!("either prompt or file must be specified, but not both")
        }
        str
    } else if let Some(file) = file {
        std::fs::read_to_string(&file).with_context(|| format!("unable to read {file}"))?
    } else {
        "Hello my name is".to_string()
    };

    let mut model_params = pin!(model_params);

    for (k, v) in &key_value_overrides {
        let k = CString::new(k.as_bytes()).with_context(|| format!("invalid key: {k}"))?;
        model_params.as_mut().append_kv_override(k.as_c_str(), *v);
    }

    if gpu_offload_enabled && cmoe {
        model_params.as_mut().add_cpu_moe_override();
    } else if cmoe {
        eprintln!("warning: --cmoe ignored because GPU offload is not active");
    }

    let model_path = runtime
        .block_on(model.get_or_load(&runtime_manager))
        .with_context(|| "failed to get model from args")?;

    let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
        .with_context(|| "unable to load model")?;

    // initialize the context
    let mut ctx_params =
        LlamaContextParams::default().with_n_ctx(ctx_size.or(Some(NonZeroU32::new(2048).unwrap())));

    if let Some(threads) = threads {
        ctx_params = ctx_params.with_n_threads(threads);
    }
    if let Some(threads_batch) = threads_batch.or(threads) {
        ctx_params = ctx_params.with_n_threads_batch(threads_batch);
    }

    let mut ctx = model
        .new_context(&backend, ctx_params)
        .with_context(|| "unable to create the llama_context")?;

    // tokenize the prompt

    let tokens_list = model
        .str_to_token(&prompt, AddBos::Always)
        .with_context(|| format!("failed to tokenize {prompt}"))?;

    let n_cxt = ctx.n_ctx() as i32;
    let n_kv_req = tokens_list.len() as i32 + (n_len - tokens_list.len() as i32);

    eprintln!("n_len = {n_len}, n_ctx = {n_cxt}, k_kv_req = {n_kv_req}");

    // make sure the KV cache is big enough to hold all the prompt and generated tokens
    if n_kv_req > n_cxt {
        bail!(
            "n_kv_req > n_ctx, the required kv cache size is not big enough
either reduce n_len or increase n_ctx"
        )
    }

    if tokens_list.len() >= usize::try_from(n_len)? {
        bail!("the prompt is too long, it has more tokens than n_len")
    }

    // print the prompt token-by-token
    eprintln!();

    let mut decoder = encoding_rs::UTF_8.new_decoder();

    for token in &tokens_list {
        eprint!(
            "{}",
            model.token_to_piece(*token, &mut decoder, true, None)?
        );
    }

    std::io::stderr().flush()?;

    // create a llama_batch with size 512
    // we use this object to submit token data for decoding
    let mut batch = LlamaBatch::new(512, 1);

    let last_index: i32 = (tokens_list.len() - 1) as i32;
    for (i, token) in (0_i32..).zip(tokens_list) {
        // llama_decode will output logits only for the last token of the prompt
        let is_last = i == last_index;
        batch.add(token, i, &[0], is_last)?;
    }

    ctx.decode(&mut batch)
        .with_context(|| "llama_decode() failed")?;

    // main loop

    let mut n_cur = batch.n_tokens();
    let mut n_decode = 0;

    let t_main_start = ggml_time_us();

    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::dist(seed.unwrap_or(1234)),
        LlamaSampler::greedy(),
    ]);

    while n_cur <= n_len {
        // sample the next token
        {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);

            sampler.accept(token);

            // is it an end of stream?
            if model.is_eog_token(token) {
                eprintln!();
                break;
            }

            let output_string = model.token_to_piece(token, &mut decoder, true, None)?;
            // use `Decoder.decode_to_string()` to avoid the intermediate buffer
            print!("{output_string}");
            std::io::stdout().flush()?;

            batch.clear();
            batch.add(token, n_cur, &[0], true)?;
        }

        n_cur += 1;

        ctx.decode(&mut batch).with_context(|| "failed to eval")?;

        n_decode += 1;
    }

    eprintln!("\n");

    let t_main_end = ggml_time_us();

    let duration = Duration::from_micros((t_main_end - t_main_start) as u64);

    eprintln!(
        "decoded {} tokens in {:.2} s, speed {:.2} t/s\n",
        n_decode,
        duration.as_secs_f32(),
        n_decode as f32 / duration.as_secs_f32()
    );

    println!("{}", ctx.timings());

    Ok(())
}
