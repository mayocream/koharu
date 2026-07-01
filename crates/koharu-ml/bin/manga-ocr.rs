use clap::Parser;
use koharu_ml::manga_ocr::MangaOcr;
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};

#[path = "common.rs"]
mod common;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    common::init_tracing();

    let cli = Cli::parse();
    let image = image::open(&cli.input)?;
    let images = vec![image];

    let runtime = RuntimeManager::new(
        default_app_data_root(),
        if cli.cpu {
            ComputePolicy::CpuOnly
        } else {
            ComputePolicy::PreferGpu
        },
    )?;
    runtime.prepare().await?;

    let model = MangaOcr::load(&runtime, cli.cpu).await?;
    let output = model
        .inference(&images)?
        .into_iter()
        .next()
        .unwrap_or_default();

    println!("{output}");

    Ok(())
}
