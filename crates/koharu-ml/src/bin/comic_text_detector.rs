use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use image::{GrayImage, Rgba, RgbaImage};
use imageproc::{
    drawing::{draw_hollow_rect_mut, draw_line_segment_mut},
    rect::Rect,
};
use koharu_ml::comic_text_detector::{ComicTextDetection, ComicTextDetector, Quad};

#[derive(Debug, Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: PathBuf,

    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    #[arg(long, value_name = "FILE")]
    annotated_output: Option<PathBuf>,

    #[arg(long, value_name = "FILE")]
    mask_output: Option<PathBuf>,

    #[arg(long, value_name = "FILE")]
    shrink_output: Option<PathBuf>,

    #[arg(long, value_name = "FILE")]
    threshold_output: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let image = image::open(&cli.input)?;

    koharu_ml::init().await?;

    let model = ComicTextDetector::load(koharu_ml::device(cli.cpu)).await?;
    let detection = model.inference(&image)?;

    if let Some(path) = cli.annotated_output {
        let mut annotated = image.to_rgba8();
        draw_detection(&mut annotated, &detection);
        annotated.save(path)?;
    }
    if let Some(path) = cli.mask_output {
        detection.mask.save(path)?;
    }
    if let Some(path) = cli.shrink_output {
        detection.shrink_map.save(path)?;
    }
    if let Some(path) = cli.threshold_output {
        detection.threshold_map.save(path)?;
    }

    let json = serde_json::to_string_pretty(&detection.to_json())?;
    if let Some(path) = cli.output {
        std::fs::write(path, json)?;
    } else {
        println!("{json}");
    }

    Ok(())
}

fn draw_detection(image: &mut RgbaImage, detection: &ComicTextDetection) {
    overlay_mask(image, &detection.mask);

    for line in &detection.line_polygons {
        draw_quad(image, line, Rgba([20, 220, 80, 255]));
    }

    for block in &detection.blocks {
        let x1 = block.bbox[0].min(block.bbox[2]).max(0.0);
        let y1 = block.bbox[1].min(block.bbox[3]).max(0.0);
        let x2 = block.bbox[0].max(block.bbox[2]).min(image.width() as f32);
        let y2 = block.bbox[1].max(block.bbox[3]).min(image.height() as f32);
        let width = (x2 - x1).max(1.0) as u32;
        let height = (y2 - y1).max(1.0) as u32;
        draw_hollow_rect_mut(
            image,
            Rect::at(x1 as i32, y1 as i32).of_size(width, height),
            Rgba([255, 40, 40, 255]),
        );
        for line in &block.line_polygons {
            draw_quad(image, line, Rgba([255, 220, 40, 255]));
        }
    }
}

fn overlay_mask(image: &mut RgbaImage, mask: &GrayImage) {
    let width = image.width().min(mask.width());
    let height = image.height().min(mask.height());
    for y in 0..height {
        for x in 0..width {
            let value = mask.get_pixel(x, y)[0];
            if value == 0 {
                continue;
            }
            let pixel = image.get_pixel_mut(x, y);
            let alpha = (value as f32 / 255.0 * 0.35).clamp(0.0, 0.35);
            pixel.0[0] = ((pixel.0[0] as f32) * (1.0 - alpha)) as u8;
            pixel.0[1] = ((pixel.0[1] as f32) * (1.0 - alpha)) as u8;
            pixel.0[2] = ((pixel.0[2] as f32) * (1.0 - alpha) + 255.0 * alpha) as u8;
            pixel.0[3] = 255;
        }
    }
}

fn draw_quad(image: &mut RgbaImage, quad: &Quad, color: Rgba<u8>) {
    for index in 0..4 {
        let a = quad[index];
        let b = quad[(index + 1) % 4];
        draw_line_segment_mut(image, (a[0], a[1]), (b[0], b[1]), color);
    }
}
