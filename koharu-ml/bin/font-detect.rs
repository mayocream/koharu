use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use koharu_ml::font_detector::{FontDetector, ModelKind, TextDirection};

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Run YuzuMarker.FontDetection (Candle) on an image"
)]
struct Args {
    /// Path to the input image.
    #[arg(short, long)]
    input: PathBuf,
    /// Number of top font classes to return.
    #[arg(short = 'k', long, default_value_t = 5)]
    top_k: usize,
    /// Force CPU even if GPU is available.
    #[arg(long)]
    cpu: bool,
    /// Backbone architecture (must match the converted checkpoint).
    #[arg(long, default_value = "resnet50", value_enum)]
    model: ModelKind,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let detector = FontDetector::load_with_kind(args.cpu, args.model).await?;
    let image = image::open(&args.input)?;
    let start = std::time::Instant::now();
    let result = detector.inference(&[image], args.top_k)?;
    let Some(pred) = result.first() else {
        return Ok(());
    };
    println!("Inference took: {:.2?}", start.elapsed());

    println!("Top fonts:");
    for (idx, prob) in &pred.top_fonts {
        let name = pred.named_fonts.iter().find(|f| f.index == *idx);
        if let Some(named) = name {
            if let Some(language) = &named.language {
                println!("  #{idx} ({} | lang={language}): {prob:.4}", named.name);
            } else {
                println!("  #{idx} ({}): {prob:.4}", named.name);
            }
        } else {
            println!("  #{idx}: {prob:.4}");
        }
    }
    println!(
        "Direction: {:?}",
        match pred.direction {
            TextDirection::Horizontal => "horizontal",
            TextDirection::Vertical => "vertical",
        }
    );
    println!(
        "Text color: rgb({},{},{})",
        pred.text_color[0], pred.text_color[1], pred.text_color[2]
    );
    println!(
        "Stroke color: rgb({},{},{}) width_px={:.2}",
        pred.stroke_color[0], pred.stroke_color[1], pred.stroke_color[2], pred.stroke_width_px
    );
    println!(
        "Font size (px): {:.2} | line height: {:.2} | angle: {:.1}°",
        pred.font_size_px, pred.line_height, pred.angle_deg
    );

    Ok(())
}
