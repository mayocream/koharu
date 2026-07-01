use clap::{Parser, ValueEnum};
use koharu_ml::paddleocr_vl::{PaddleOcrVl, PaddleOcrVlOutput, PaddleOcrVlTask};
use koharu_runtime::{ComputePolicy, RuntimeManager, default_app_data_root};

#[path = "common.rs"]
mod common;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TaskArg {
    Ocr,
    Table,
    Formula,
    Chart,
    Spotting,
    Seal,
}

impl From<TaskArg> for PaddleOcrVlTask {
    fn from(value: TaskArg) -> Self {
        match value {
            TaskArg::Ocr => Self::Ocr,
            TaskArg::Table => Self::Table,
            TaskArg::Formula => Self::Formula,
            TaskArg::Chart => Self::Chart,
            TaskArg::Spotting => Self::Spotting,
            TaskArg::Seal => Self::Seal,
        }
    }
}

#[derive(Parser)]
struct Cli {
    #[arg(long, value_name = "FILE", num_args = 1..)]
    input: Vec<String>,

    #[arg(long, value_name = "DIR")]
    model_dir: Option<String>,

    #[arg(long, value_enum, default_value = "ocr")]
    task: TaskArg,

    #[arg(long, default_value_t = 512)]
    max_new_tokens: usize,

    #[arg(long, value_name = "FILE")]
    json_output: Option<String>,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OutputEnvelope {
    outputs: Vec<PaddleOcrVlOutput>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    common::init_tracing();

    let cli = Cli::parse();
    let task: PaddleOcrVlTask = cli.task.into();
    let runtime = RuntimeManager::new(
        default_app_data_root(),
        if cli.cpu {
            ComputePolicy::CpuOnly
        } else {
            ComputePolicy::PreferGpu
        },
    )?;
    runtime.prepare().await?;

    let mut model = if let Some(model_dir) = &cli.model_dir {
        PaddleOcrVl::load_from_dir(model_dir, cli.cpu)?
    } else {
        PaddleOcrVl::load(&runtime, cli.cpu).await?
    };

    let images = cli
        .input
        .iter()
        .map(image::open)
        .collect::<Result<Vec<_>, _>>()?;
    let outputs = model.inference_images(&images, task, cli.max_new_tokens)?;
    for (input, output) in cli.input.iter().zip(&outputs) {
        if cli.input.len() > 1 {
            println!("== {} ==", input);
        }
        println!("{}", output.text);
    }

    if let Some(path) = &cli.json_output {
        std::fs::write(
            path,
            serde_json::to_string_pretty(&OutputEnvelope { outputs })?,
        )?;
    }

    Ok(())
}
