use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use koharu_ml::flux2_klein::{
    Flux2ImageToImageOptions, Flux2InferenceOptions, Flux2InpaintOptions, Flux2Klein,
};

#[derive(Debug, Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: Option<PathBuf>,

    #[arg(short, long, value_name = "FILE", requires = "input")]
    mask: Option<PathBuf>,

    #[arg(short, long, value_name = "FILE")]
    output: PathBuf,

    #[arg(short, long, value_name = "TEXT", required_unless_present = "input")]
    prompt: Option<String>,

    #[arg(long, value_name = "FILE")]
    reference: Option<PathBuf>,

    #[arg(long, default_value_t = 4)]
    steps: usize,

    #[arg(long, default_value_t = 1.0)]
    strength: f64,

    #[arg(long, default_value_t = 1024 * 1024)]
    max_pixels: u32,

    #[arg(long, default_value_t = 16)]
    mask_padding: u8,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let input = cli.input.as_ref().map(image::open).transpose()?;
    let mask = cli.mask.as_ref().map(image::open).transpose()?;
    let reference = cli.reference.as_ref().map(image::open).transpose()?;

    koharu_ml::init().await?;

    let model = Flux2Klein::load(koharu_ml::device(cli.cpu)).await?;
    let output = if let Some(input) = input.as_ref() {
        if let Some(mask) = mask.as_ref() {
            let options = Flux2InpaintOptions {
                num_inference_steps: cli.steps,
                strength: cli.strength,
                max_pixels: cli.max_pixels,
                mask_padding: cli.mask_padding,
            };
            model.inpaint_with_reference(input, mask, reference.as_ref(), &options)?
        } else {
            let options = Flux2ImageToImageOptions {
                num_inference_steps: cli.steps,
                strength: cli.strength,
                max_pixels: cli.max_pixels,
            };
            model.image_to_image_with_reference(input, reference.as_ref(), &options)?
        }
    } else {
        let prompt = cli
            .prompt
            .as_deref()
            .context("--prompt is required when --input is omitted")?;
        let options = Flux2InferenceOptions {
            num_inference_steps: i32::try_from(cli.steps)?,
            ..Flux2InferenceOptions::default()
        };
        let references = reference.as_ref().map(std::slice::from_ref).unwrap_or(&[]);
        let generated = model
            .inference(prompt, references, &options)?
            .into_iter()
            .next()
            .context("FLUX.2 Klein returned no image")?;
        image::DynamicImage::ImageRgb8(generated)
    };
    output.save(cli.output)?;

    Ok(())
}
