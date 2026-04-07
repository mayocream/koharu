use anyhow::{Result, anyhow};
use clap::Parser;
use image::{DynamicImage, Rgb};
use imageproc::{drawing::draw_hollow_rect_mut, rect::Rect};
use koharu_ml::speech_bubble_segmentation::{SpeechBubbleRegion, SpeechBubbleSegmentation};
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
    config_path: Option<String>,

    #[arg(long, value_name = "FILE")]
    weights_path: Option<String>,

    #[arg(long, value_name = "FILE")]
    mask_output: Option<String>,

    #[arg(long, value_name = "FILE")]
    annotated_output: Option<String>,

    #[arg(long, default_value_t = 0.5)]
    threshold: f32,

    #[arg(long)]
    confidence_threshold: Option<f32>,

    #[arg(long)]
    nms_threshold: Option<f32>,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

fn main() -> Result<()> {
    common::init_tracing();

    std::thread::Builder::new()
        .name("speech-bubble-segmentation".to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let runtime = Builder::new_current_thread().enable_all().build()?;
            runtime.block_on(async_main())
        })?
        .join()
        .map_err(|_| anyhow!("speech-bubble-segmentation thread panicked"))?
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

    let model = match (&cli.config_path, &cli.weights_path) {
        (Some(config_path), Some(weights_path)) => {
            SpeechBubbleSegmentation::load_from_paths(config_path, weights_path, cli.cpu)?
        }
        (None, None) => SpeechBubbleSegmentation::load(&runtime, cli.cpu).await?,
        _ => anyhow::bail!("--config-path and --weights-path must be provided together"),
    };
    let bytes = std::fs::read(&cli.input)?;
    let format = image::guess_format(&bytes)?;
    let image = image::load_from_memory_with_format(&bytes, format)?;
    let result = match (cli.confidence_threshold, cli.nms_threshold) {
        (Some(confidence_threshold), Some(nms_threshold)) => {
            model.inference_with_thresholds(&image, confidence_threshold, nms_threshold)?
        }
        (Some(confidence_threshold), None) => {
            model.inference_with_thresholds(&image, confidence_threshold, 0.45)?
        }
        (None, Some(nms_threshold)) => {
            model.inference_with_thresholds(&image, 0.25, nms_threshold)?
        }
        (None, None) => model.inference(&image)?,
    };

    result.probability_map.to_gray_image()?.save(&cli.output)?;
    if let Some(mask_output) = &cli.mask_output {
        result
            .probability_map
            .threshold(cli.threshold)?
            .save(mask_output)?;
    }
    if let Some(annotated_output) = &cli.annotated_output {
        draw_regions(&image, &result.regions).save(annotated_output)?;
    }

    println!("{}", serde_json::to_string_pretty(&result.regions)?);
    Ok(())
}

fn draw_regions(image: &DynamicImage, regions: &[SpeechBubbleRegion]) -> DynamicImage {
    let mut annotated = image.to_rgb8();
    for region in regions {
        let bbox = region.bbox;
        let x = bbox[0].floor() as i32;
        let y = bbox[1].floor() as i32;
        let width = (bbox[2] - bbox[0]).ceil().max(1.0) as u32;
        let height = (bbox[3] - bbox[1]).ceil().max(1.0) as u32;
        draw_hollow_rect_mut(
            &mut annotated,
            Rect::at(x, y).of_size(width, height),
            Rgb([0, 255, 128]),
        );
    }
    DynamicImage::ImageRgb8(annotated)
}
