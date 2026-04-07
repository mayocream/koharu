use anyhow::{Result, anyhow, ensure};
use clap::Parser;
use imageproc::{
    drawing::{draw_hollow_polygon_mut, draw_hollow_rect_mut},
    point::Point,
    rect::Rect,
};
use koharu_ml::comic_text_detector::ComicTextDetector;
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
    json_output: Option<String>,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

fn overlay_stroke_radius(width: u32, height: u32) -> i32 {
    let max_dim = width.max(height) as f32;
    ((max_dim / 1800.0).round() as i32).clamp(1, 8)
}

fn draw_thick_polygon(
    image: &mut image::RgbaImage,
    points: &[Point<f32>],
    color: image::Rgba<u8>,
    radius: i32,
) {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy > radius * radius {
                continue;
            }
            let shifted = points
                .iter()
                .map(|point| Point::new(point.x + dx as f32, point.y + dy as f32))
                .collect::<Vec<_>>();
            draw_hollow_polygon_mut(image, &shifted, color);
        }
    }
}

fn draw_thick_rect(image: &mut image::RgbaImage, rect: Rect, color: image::Rgba<u8>, radius: i32) {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy > radius * radius {
                continue;
            }
            draw_hollow_rect_mut(
                image,
                Rect::at(rect.left() + dx, rect.top() + dy).of_size(rect.width(), rect.height()),
                color,
            );
        }
    }
}

fn main() -> Result<()> {
    common::init_tracing();

    std::thread::Builder::new()
        .name("comic-text-detector".to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let runtime = Builder::new_current_thread().enable_all().build()?;
            runtime.block_on(async_main())
        })?
        .join()
        .map_err(|_| anyhow!("comic-text-detector thread panicked"))?
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

    let model = ComicTextDetector::load(&runtime, cli.cpu).await?;
    let bytes = std::fs::read(&cli.input)?;
    let format = image::guess_format(&bytes)?;
    let image = image::load_from_memory_with_format(&bytes, format)?;

    let detection = model.inference(&image)?;

    ensure!(
        !detection.text_blocks.is_empty(),
        "No text detected in the image."
    );
    ensure!(
        !detection.mask.iter().all(|m| *m < 255),
        "No text mask generated."
    );

    let mut image = image.to_rgba8();
    let stroke_radius = overlay_stroke_radius(image.width(), image.height());
    for polygon in &detection.line_polygons {
        let points = polygon
            .iter()
            .map(|point| Point::new(point[0], point[1]))
            .collect::<Vec<_>>();
        draw_thick_polygon(
            &mut image,
            &points,
            image::Rgba([0, 255, 0, 255]),
            stroke_radius,
        );
    }
    for block in &detection.text_blocks {
        draw_thick_rect(
            &mut image,
            Rect::at(block.x as i32, block.y as i32)
                .of_size(block.width.max(1.0) as u32, block.height.max(1.0) as u32),
            image::Rgba([255, 0, 0, 255]),
            stroke_radius,
        );
    }

    image::DynamicImage::ImageRgba8(image).save(&cli.output)?;
    detection.mask.save(format!("{}_mask.png", cli.output))?;
    if let Some(path) = &cli.json_output {
        std::fs::write(path, serde_json::to_vec_pretty(&detection.text_blocks)?)?;
    }
    Ok(())
}
