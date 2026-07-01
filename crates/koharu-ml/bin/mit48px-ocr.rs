use clap::Parser;
use koharu_ml::TextRegion;
use koharu_ml::mit48px_ocr::{Mit48pxBlockPrediction, Mit48pxOcr, Mit48pxPrediction};
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};

#[path = "common.rs"]
mod common;

#[derive(Parser)]
struct Cli {
    #[arg(long, value_name = "FILE")]
    input: String,

    #[arg(long, value_name = "DIR")]
    model_dir: Option<String>,

    #[arg(long, value_name = "FILE")]
    blocks_json: Option<String>,

    #[arg(long, value_name = "FILE")]
    json_output: Option<String>,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OutputEnvelope {
    regions: Option<Vec<Mit48pxPrediction>>,
    blocks: Option<Vec<Mit48pxBlockPrediction>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    common::init_tracing();

    let cli = Cli::parse();
    let image = image::open(&cli.input)?;
    let runtime = RuntimeManager::new(
        default_app_data_root(),
        if cli.cpu {
            ComputePolicy::CpuOnly
        } else {
            ComputePolicy::PreferGpu
        },
    )?;
    runtime.prepare().await?;

    let model = if let Some(model_dir) = &cli.model_dir {
        Mit48pxOcr::load_from_dir(model_dir, cli.cpu)?
    } else {
        Mit48pxOcr::load(&runtime, cli.cpu).await?
    };

    let output = if let Some(blocks_path) = &cli.blocks_json {
        let blocks: Vec<TextRegion> = serde_json::from_str(&std::fs::read_to_string(blocks_path)?)?;
        let predictions = model.inference_text_blocks(&image, &blocks)?;
        for prediction in &predictions {
            println!(
                "#{} {:.4} {}",
                prediction.block_index, prediction.confidence, prediction.text
            );
        }
        OutputEnvelope {
            regions: None,
            blocks: Some(predictions),
        }
    } else {
        let predictions = model.inference_regions(&[image])?;
        for prediction in &predictions {
            println!("{:.4} {}", prediction.confidence, prediction.text);
        }
        OutputEnvelope {
            regions: Some(predictions),
            blocks: None,
        }
    };

    if let Some(path) = &cli.json_output {
        std::fs::write(path, serde_json::to_string_pretty(&output)?)?;
    }

    Ok(())
}
