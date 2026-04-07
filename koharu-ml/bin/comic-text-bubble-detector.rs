use anyhow::{Result, anyhow};
use clap::Parser;
use image::Rgba;
use imageproc::{
    drawing::{draw_filled_rect_mut, draw_hollow_rect_mut},
    rect::Rect,
};
use koharu_ml::comic_text_bubble_detector::ComicTextBubbleDetector;
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};
use tokio::runtime::Builder;

#[path = "common.rs"]
mod common;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(long, default_value_t = 0.3)]
    threshold: f32,

    #[arg(long, value_name = "FILE")]
    output: Option<String>,

    #[arg(long, value_name = "FILE")]
    annotated_output: Option<String>,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

fn overlay_stroke_radius(width: u32, height: u32) -> i32 {
    let max_dim = width.max(height) as f32;
    ((max_dim / 1800.0).round() as i32).clamp(1, 8)
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

fn draw_scaled_pixel(image: &mut image::RgbaImage, x: i32, y: i32, scale: i32, color: Rgba<u8>) {
    for dy in 0..scale {
        for dx in 0..scale {
            let px = x + dx;
            let py = y + dy;
            if px >= 0 && py >= 0 && px < image.width() as i32 && py < image.height() as i32 {
                image.put_pixel(px as u32, py as u32, color);
            }
        }
    }
}

fn glyph_for_char(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        '-' => [0, 0, 0, 0b11111, 0, 0, 0],
        _ => [0b11111, 0b00001, 0b00010, 0b00100, 0, 0b00100, 0],
    }
}

fn badge_size(label: &str, scale: i32) -> (i32, i32) {
    let padding = scale.max(1);
    let char_width = 5 * scale;
    let char_height = 7 * scale;
    let spacing = scale;
    let width = padding * 2
        + (label.len() as i32 * char_width)
        + ((label.len() as i32 - 1).max(0) * spacing);
    let height = padding * 2 + char_height;
    (width, height)
}

fn draw_label_badge(
    image: &mut image::RgbaImage,
    anchor_x: i32,
    anchor_y: i32,
    label: &str,
    fill: Rgba<u8>,
    scale: i32,
) {
    let label = label.replace('_', " ").to_ascii_uppercase();
    let (badge_w, badge_h) = badge_size(&label, scale);
    let image_w = image.width() as i32;
    let image_h = image.height() as i32;
    let mut x = anchor_x;
    let mut y = anchor_y - badge_h - scale;
    if y < 0 {
        y = anchor_y + scale;
    }
    x = x.clamp(0, (image_w - badge_w).max(0));
    y = y.clamp(0, (image_h - badge_h).max(0));

    draw_filled_rect_mut(
        image,
        Rect::at(x, y).of_size(badge_w.max(1) as u32, badge_h.max(1) as u32),
        fill,
    );

    let padding = scale.max(1);
    let spacing = scale;
    let mut cursor_x = x + padding;
    let cursor_y = y + padding;
    for ch in label.chars() {
        let glyph = glyph_for_char(ch);
        for (row, bits) in glyph.iter().copied().enumerate() {
            for col in 0..5 {
                if (bits >> (4 - col)) & 1 == 1 {
                    draw_scaled_pixel(
                        image,
                        cursor_x + col * scale,
                        cursor_y + row as i32 * scale,
                        scale,
                        Rgba([0, 0, 0, 255]),
                    );
                }
            }
        }
        cursor_x += 5 * scale + spacing;
    }
}

fn color_for_label(label_id: usize) -> image::Rgba<u8> {
    match label_id {
        0 => image::Rgba([0, 255, 0, 255]),
        1 => image::Rgba([255, 160, 0, 255]),
        2 => image::Rgba([0, 160, 255, 255]),
        _ => image::Rgba([255, 255, 255, 255]),
    }
}

fn main() -> Result<()> {
    common::init_tracing();

    std::thread::Builder::new()
        .name("comic-text-bubble-detector".to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let runtime = Builder::new_current_thread().enable_all().build()?;
            runtime.block_on(async_main())
        })?
        .join()
        .map_err(|_| anyhow!("comic-text-bubble-detector thread panicked"))?
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

    let model = ComicTextBubbleDetector::load(&runtime, cli.cpu).await?;
    let bytes = std::fs::read(&cli.input)?;
    let format = image::guess_format(&bytes)?;
    let image = image::load_from_memory_with_format(&bytes, format)?;
    let detection = model.inference_with_threshold(&image, cli.threshold)?;

    if let Some(path) = &cli.annotated_output {
        let mut annotated = image.to_rgba8();
        let stroke_radius = overlay_stroke_radius(annotated.width(), annotated.height());
        let badge_scale = (stroke_radius + 1).max(2);
        for region in &detection.detections {
            let color = color_for_label(region.label_id);
            draw_thick_rect(
                &mut annotated,
                Rect::at(region.bbox[0] as i32, region.bbox[1] as i32).of_size(
                    (region.bbox[2] - region.bbox[0]).max(1.0) as u32,
                    (region.bbox[3] - region.bbox[1]).max(1.0) as u32,
                ),
                color,
                stroke_radius,
            );
            draw_label_badge(
                &mut annotated,
                region.bbox[0].floor() as i32,
                region.bbox[1].floor() as i32,
                &region.label,
                color,
                badge_scale,
            );
        }
        image::DynamicImage::ImageRgba8(annotated).save(path)?;
    }

    let json = serde_json::to_string_pretty(&detection)?;
    if let Some(output) = cli.output {
        std::fs::write(output, json)?;
    } else {
        println!("{json}");
    }
    Ok(())
}
