use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use image::{Rgba, RgbaImage};
use imageproc::{drawing::draw_hollow_rect_mut, rect::Rect};
use koharu_ml::pp_doclayout_v3::{PPDocLayoutV3, PPDocLayoutV3Detections};

#[derive(Debug, Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: PathBuf,

    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    #[arg(long, value_name = "FILE")]
    annotated_output: Option<PathBuf>,

    #[arg(long, default_value_t = 0.3)]
    threshold: f32,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let image = image::open(&cli.input)?;

    // Initialize the Koharu ML framework
    koharu_ml::init().await?;

    let model = PPDocLayoutV3::load(cli.cpu).await?;
    let result = model.inference(&image, cli.threshold)?;

    if let Some(path) = cli.annotated_output {
        let mut annotated = image.to_rgba8();
        draw_regions(&mut annotated, &result);
        annotated.save(path)?;
    }

    let json = serde_json::to_string_pretty(&result)?;
    if let Some(path) = cli.output {
        std::fs::write(path, json)?;
    } else {
        println!("{json}");
    }

    Ok(())
}

fn draw_regions(image: &mut RgbaImage, detections: &PPDocLayoutV3Detections) {
    let color = Rgba([255, 32, 32, 255]);
    for region in &detections.regions {
        let x1 = region.bbox[0].min(region.bbox[2]).max(0.0);
        let y1 = region.bbox[1].min(region.bbox[3]).max(0.0);
        let x2 = region.bbox[0].max(region.bbox[2]).min(image.width() as f32);
        let y2 = region.bbox[1]
            .max(region.bbox[3])
            .min(image.height() as f32);
        let width = (x2 - x1).max(1.0) as u32;
        let height = (y2 - y1).max(1.0) as u32;
        draw_hollow_rect_mut(
            image,
            Rect::at(x1 as i32, y1 as i32).of_size(width, height),
            color,
        );
    }
}
