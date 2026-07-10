use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use koharu_ml::comic_text_bubble_detector::ComicTextBubbleDetector;

#[derive(Debug, Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: PathBuf,

    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    #[arg(long, value_name = "FILE")]
    annotated_output: Option<PathBuf>,

    #[arg(long, default_value_t = 0.3)]
    confidence_threshold: f32,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let image = image::open(&cli.input)?;

    koharu_ml::init().await?;

    let model = ComicTextBubbleDetector::load_with_threshold(
        koharu_ml::device(cli.cpu),
        cli.confidence_threshold,
    )
    .await?;
    let detection = model.inference(&image)?;

    if let Some(path) = cli.annotated_output {
        detection.annotated_image(&image).save(path)?;
    }

    let json = serde_json::to_string_pretty(&detection)?;
    if let Some(path) = cli.output {
        std::fs::write(path, json)?;
    } else {
        println!("{json}");
    }

    Ok(())
}
