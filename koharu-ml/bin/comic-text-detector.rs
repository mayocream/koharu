use anyhow::{Result, ensure};
use clap::Parser;
use koharu_ml::{comic_text_detector::ComicTextDetector, device};
use tracing_subscriber::fmt::format::FmtSpan;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(short, long, value_name = "FILE")]
    output: String,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .init();

    let cli = Cli::parse();
    let device = device(cli.cpu)?;

    let model = ComicTextDetector::load(device).await?;
    let image = image::open(&cli.input)?;

    let (bboxes, mask) = model.inference(&image)?;

    ensure!(!bboxes.is_empty(), "No text detected in the image.");
    ensure!(!mask.iter().all(|m| *m < 255), "No text mask generated.");

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
