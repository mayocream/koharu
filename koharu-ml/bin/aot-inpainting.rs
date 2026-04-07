use anyhow::{Result, bail};
use clap::Parser;
use koharu_ml::aot_inpainting::AotInpainting;
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};

#[path = "common.rs"]
mod common;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(short, long, value_name = "FILE")]
    mask: String,

    #[arg(short, long, value_name = "FILE")]
    output: String,

    #[arg(long, value_name = "FILE")]
    config_path: Option<String>,

    #[arg(long, value_name = "FILE")]
    weights_path: Option<String>,

    #[arg(long)]
    max_side: Option<u32>,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    common::init_tracing();

    let cli = Cli::parse();
    let runtime = RuntimeManager::new(
        default_app_data_root(),
        if cli.cpu {
            ComputePolicy::CpuOnly
        } else {
            ComputePolicy::PreferGpu
        },
    )?;
    runtime.prepare().await?;

    let model = match (&cli.config_path, &cli.weights_path) {
        (Some(config_path), Some(weights_path)) => {
            AotInpainting::load_from_paths(config_path, weights_path, cli.cpu)?
        }
        (None, None) => AotInpainting::load(&runtime, cli.cpu).await?,
        _ => bail!("--config-path and --weights-path must be provided together"),
    };

    let image = image::open(&cli.input)?;
    let mask = image::open(&cli.mask)?;
    let started = std::time::Instant::now();
    let output = if let Some(max_side) = cli.max_side {
        model.inference_with_max_side(&image, &mask, max_side)?
    } else {
        model.inference(&image, &mask)?
    };

    println!("Inference took: {:?}", started.elapsed());
    output.save(&cli.output)?;
    Ok(())
}
