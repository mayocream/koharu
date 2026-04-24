use std::{path::PathBuf, time::Instant};

use anyhow::{Result, bail};
use clap::Parser;
use koharu_ml::flux2_klein::{
    Flux2ImageToImageOptions, Flux2InpaintOptions, Flux2Klein, Flux2KleinPaths,
};
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};

#[path = "common.rs"]
mod common;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: PathBuf,

    #[arg(short, long, value_name = "FILE")]
    mask: Option<PathBuf>,

    #[arg(short, long, value_name = "FILE")]
    output: PathBuf,

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

    #[arg(long, value_name = "FILE")]
    transformer_path: Option<PathBuf>,

    #[arg(long, value_name = "FILE")]
    vae_path: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    common::init_tracing();

    let cli = Cli::parse();
    let runtime = RuntimeManager::new(default_app_data_root(), ComputePolicy::PreferGpu)?;
    runtime.prepare().await?;

    let model = match model_paths(&cli)? {
        Some(paths) => Flux2Klein::load_from_paths(paths)?,
        None => Flux2Klein::load(&runtime).await?,
    };

    let image = image::open(&cli.input)?;
    let reference = match &cli.reference {
        Some(path) => Some(image::open(path)?),
        None => None,
    };

    let embed_start = Instant::now();
    model.precompute_prompt_embeddings()?;
    println!("Prompt embeddings took: {:?}", embed_start.elapsed());

    let start = Instant::now();
    let output = if let Some(mask_path) = &cli.mask {
        let mask = image::open(mask_path)?;
        let options = Flux2InpaintOptions {
            num_inference_steps: cli.steps,
            strength: cli.strength,
            max_pixels: cli.max_pixels,
            mask_padding: cli.mask_padding,
        };
        model.inpaint_with_reference(&image, &mask, reference.as_ref(), &options)?
    } else {
        let options = Flux2ImageToImageOptions {
            num_inference_steps: cli.steps,
            strength: cli.strength,
            max_pixels: cli.max_pixels,
        };
        model.image_to_image_with_reference(&image, reference.as_ref(), &options)?
    };
    println!("Inference took: {:?}", start.elapsed());
    output.save(&cli.output)?;

    Ok(())
}

fn model_paths(cli: &Cli) -> Result<Option<Flux2KleinPaths>> {
    match (&cli.transformer_path, &cli.vae_path) {
        (None, None) => Ok(None),
        (Some(transformer_gguf), Some(vae_safetensors)) => Ok(Some(Flux2KleinPaths {
            transformer_gguf: transformer_gguf.clone(),
            vae_safetensors: vae_safetensors.clone(),
        })),
        _ => bail!("--transformer-path and --vae-path must be provided together"),
    }
}
