use anyhow::{Result, anyhow};
use clap::Parser;
use koharu_ml::manga_text_segmentation_2025::MangaTextSegmentation;
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};
use tokio::runtime::Builder;

#[path = "common.rs"]
mod common;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(short, long, value_name = "FILE")]
    output: String,

    #[arg(long, value_name = "FILE")]
    mask_output: Option<String>,

    #[arg(long, default_value_t = 0.5)]
    threshold: f32,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

fn main() -> Result<()> {
    common::init_tracing();

    std::thread::Builder::new()
        .name("manga-text-segmentation-2025".to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let runtime = Builder::new_current_thread().enable_all().build()?;
            runtime.block_on(async_main())
        })?
        .join()
        .map_err(|_| anyhow!("manga-text-segmentation-2025 thread panicked"))?
}

async fn async_main() -> Result<()> {
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

    let model = MangaTextSegmentation::load(&runtime, cli.cpu).await?;
    let bytes = std::fs::read(&cli.input)?;
    let format = image::guess_format(&bytes)?;
    let image = image::load_from_memory_with_format(&bytes, format)?;
    let probability_map = model.inference(&image)?;

    probability_map.to_gray_image()?.save(&cli.output)?;
    if let Some(mask_output) = &cli.mask_output {
        probability_map
            .threshold(cli.threshold)?
            .save(mask_output)?;
    }

    Ok(())
}
