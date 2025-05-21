use clap::Parser;
use comic_text_detector::ComicTextDetector;
use image::GenericImageView;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(short, long, value_name = "FILE")]
    output: String,

    #[arg(short, long, default_value_t = 0.5)]
    confidence_threshold: f32,

    #[arg(short, long, default_value_t = 0.4)]
    nms_threshold: f32,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let model = ComicTextDetector::new()?;
    let image = image::open(&cli.input)?;
    let (orig_width, orig_height) = image.dimensions();

    let output = model.inference(&image, cli.confidence_threshold, cli.nms_threshold)?;

    // draw the boxes on the image
    let mut image = image.to_rgba8();
    for bbox in output.bboxes {
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

    // save the segment
    let segment = image::DynamicImage::ImageLuma8(
        image::GrayImage::from_raw(1024, 1024, output.segment)
            .expect("Failed to create segment image"),
    );

    let segment_image = segment.resize_exact(
        orig_width,
        orig_height,
        image::imageops::FilterType::CatmullRom,
    );
    segment_image.save(format!("{}_segment.png", cli.output))?;

    Ok(())
}
