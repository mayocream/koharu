use anyhow::{Result, anyhow};
use clap::Parser;
use image::Rgba;
use imageproc::{
    drawing::{draw_filled_rect_mut, draw_hollow_polygon_mut, draw_hollow_rect_mut},
    point::Point,
    rect::Rect,
};
use koharu_ml::pp_doclayout_v3::PPDocLayoutV3;
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

const DIGIT_FONT_3X5: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

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

fn badge_size(label: &str, scale: i32) -> (i32, i32) {
    let padding = scale.max(1);
    let char_width = 3 * scale;
    let char_height = 5 * scale;
    let spacing = scale;
    let width = padding * 2
        + (label.len() as i32 * char_width)
        + ((label.len() as i32 - 1).max(0) * spacing);
    let height = padding * 2 + char_height;
    (width, height)
}

fn draw_order_badge(
    image: &mut image::RgbaImage,
    anchor_x: i32,
    anchor_y: i32,
    order: usize,
    scale: i32,
) {
    let label = order.to_string();
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
        Rgba([255, 230, 64, 255]),
    );

    let padding = scale.max(1);
    let spacing = scale;
    let mut cursor_x = x + padding;
    let cursor_y = y + padding;
    for ch in label.chars() {
        if let Some(digit) = ch.to_digit(10) {
            let glyph = DIGIT_FONT_3X5[digit as usize];
            for (row, bits) in glyph.iter().copied().enumerate() {
                for col in 0..3 {
                    if (bits >> (2 - col)) & 1 == 1 {
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
            cursor_x += 3 * scale + spacing;
        }
    }
}

fn main() -> Result<()> {
    common::init_tracing();

    std::thread::Builder::new()
        .name("pp-doclayout-v3".to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let runtime = Builder::new_current_thread().enable_all().build()?;
            runtime.block_on(async_main())
        })?
        .join()
        .map_err(|_| anyhow!("pp-doclayout-v3 thread panicked"))?
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

    let model = PPDocLayoutV3::load(&runtime, cli.cpu).await?;
    let bytes = std::fs::read(&cli.input)?;
    let format = image::guess_format(&bytes)?;
    let image = image::load_from_memory_with_format(&bytes, format)?;
    let result = model.inference_one(&image, cli.threshold)?;

    if let Some(path) = &cli.annotated_output {
        let mut annotated = image.to_rgba8();
        let stroke_radius = overlay_stroke_radius(annotated.width(), annotated.height());
        let badge_scale = (stroke_radius + 1).max(2);
        for region in &result.regions {
            let points = region
                .polygon_points
                .iter()
                .map(|point| Point::new(point[0], point[1]))
                .collect::<Vec<_>>();
            if points.len() >= 3 {
                draw_thick_polygon(
                    &mut annotated,
                    &points,
                    image::Rgba([0, 255, 0, 255]),
                    stroke_radius,
                );
            }
            draw_thick_rect(
                &mut annotated,
                Rect::at(region.bbox[0] as i32, region.bbox[1] as i32).of_size(
                    (region.bbox[2] - region.bbox[0]).max(1.0) as u32,
                    (region.bbox[3] - region.bbox[1]).max(1.0) as u32,
                ),
                image::Rgba([255, 0, 0, 255]),
                stroke_radius,
            );
            draw_order_badge(
                &mut annotated,
                region.bbox[0].floor() as i32,
                region.bbox[1].floor() as i32,
                region.order,
                badge_scale,
            );
        }
        image::DynamicImage::ImageRgba8(annotated).save(path)?;
    }

    let json = serde_json::to_string_pretty(&result)?;
    if let Some(output) = cli.output {
        std::fs::write(output, json)?;
    } else {
        println!("{json}");
    }
    Ok(())
}
