use clap::Parser;
use koharu_ml::lama::Lama;
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};

#[path = "common.rs"]
mod common;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(short, long, value_name = "FILE")]
    mask: String,

    #[arg(long, value_name = "FILE")]
    bubble_mask: String,

    #[arg(short, long, value_name = "FILE")]
    output: String,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    let model = Lama::load(&runtime, cli.cpu).await?;
    let image = image::open(&cli.input)?;
    let mask = image::open(&cli.mask)?;
    let bubble_mask = image::open(&cli.bubble_mask)?;

    // inferernce start time
    let start = std::time::Instant::now();

    let output = model.inference(&image, &mask, &bubble_mask)?;

    // measure inference speed
    let duration = start.elapsed();
    println!("Inference took: {:?}", duration);

    output.save(&cli.output)?;

    Ok(())
}
