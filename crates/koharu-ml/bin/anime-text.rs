use anyhow::{Result, anyhow, ensure};
use clap::{Parser, ValueEnum};
use imageproc::{drawing::draw_hollow_rect_mut, rect::Rect};
use koharu_ml::anime_text::{AnimeTextDetector, AnimeTextYoloVariant};
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};
use tokio::runtime::Builder;

#[path = "common.rs"]
mod common;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Variant {
    N,
    S,
    M,
    L,
    X,
}

impl From<Variant> for AnimeTextYoloVariant {
    fn from(value: Variant) -> Self {
        match value {
            Variant::N => Self::N,
            Variant::S => Self::S,
            Variant::M => Self::M,
            Variant::L => Self::L,
            Variant::X => Self::X,
        }
    }
}

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(short, long, value_name = "FILE")]
    output: String,

    #[arg(long, value_name = "FILE")]
    json_output: Option<String>,

    #[arg(long, value_enum, default_value_t = Variant::N)]
    variant: Variant,

    #[arg(long, default_value_t = 0.25)]
    confidence_threshold: f32,

    #[arg(long, default_value_t = 0.45)]
    nms_threshold: f32,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

fn main() -> Result<()> {
    common::init_tracing();

    std::thread::Builder::new()
        .name("anime-text-yolo".to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let runtime = Builder::new_current_thread().enable_all().build()?;
            runtime.block_on(async_main())
        })?
        .join()
        .map_err(|_| anyhow!("anime-text-yolo thread panicked"))?
}

async fn async_main() -> Result<()> {
    let cli = Cli::parse();
    let variant = AnimeTextYoloVariant::from(cli.variant);

    let runtime = RuntimeManager::new(
        default_app_data_root(),
        if cli.cpu {
            ComputePolicy::CpuOnly
        } else {
            ComputePolicy::PreferGpu
        },
    )?;
    runtime.prepare().await?;

    let model = AnimeTextDetector::load_variant(&runtime, variant, cli.cpu).await?;
    let bytes = std::fs::read(&cli.input)?;
    let format = image::guess_format(&bytes)?;
    let image = image::load_from_memory_with_format(&bytes, format)?;
    let detection =
        model.inference_with_thresholds(&image, cli.confidence_threshold, cli.nms_threshold)?;

    ensure!(
        !detection.regions.is_empty(),
        "No anime text blocks detected in the image."
    );

    let mut image = image.to_rgba8();
    for region in &detection.regions {
        let width = (region.bbox[2] - region.bbox[0]).max(1.0) as u32;
        let height = (region.bbox[3] - region.bbox[1]).max(1.0) as u32;
        draw_hollow_rect_mut(
            &mut image,
            Rect::at(region.bbox[0] as i32, region.bbox[1] as i32).of_size(width, height),
            image::Rgba([255, 0, 0, 255]),
        );
    }

    image::DynamicImage::ImageRgba8(image).save(&cli.output)?;
    if let Some(path) = &cli.json_output {
        std::fs::write(path, serde_json::to_vec_pretty(&detection)?)?;
    }
    Ok(())
}
