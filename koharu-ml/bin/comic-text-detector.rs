use anyhow::Result;
use clap::Parser;
use koharu_models::{comic_text_detector::ComicTextDetector, device};

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(short, long, value_name = "FILE")]
    output: String,

    #[arg(long, default_value_t = 0.5)]
    confidence_threshold: f32,

    #[arg(long, default_value_t = 0.4)]
    nms_threshold: f32,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let device = device(cli.cpu)?;

    let model = ComicTextDetector::load(device).await?;
    let image = image::open(&cli.input)?;

    let (bboxes, mask) = model.inference(&image)?;

    // draw the boxes on the image
    let mut image = image.to_rgba8();
    for bbox in bboxes {
        imageproc::drawing::draw_hollow_rect_mut(
            &mut image,
            imageproc::rect::Rect::at(bbox.xmin as i32, bbox.ymin as i32).of_size(
                (bbox.xmax - bbox.xmin) as u32,
                (bbox.ymax - bbox.ymin) as u32,
            ),
            image::Rgba([255, 0, 0, 255]),
        );
    }

    let output_image = image::DynamicImage::ImageRgba8(image);
    output_image.save(&cli.output)?;

    mask.save(format!("{}_mask.png", cli.output))?;

    Ok(())
}
