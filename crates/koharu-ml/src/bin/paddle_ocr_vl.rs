use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use koharu_ml::paddle_ocr_vl::{PaddleOCRVL, PaddleOCRVLTask};

#[derive(Debug, Parser)]
#[command(about = "Run PaddleOCR-VL-1.6 element recognition")]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: PathBuf,

    #[arg(short, long, value_enum, default_value_t = Task::Ocr)]
    task: Task,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Task {
    Ocr,
    Table,
    Formula,
    Chart,
    Spotting,
    Seal,
}

impl From<Task> for PaddleOCRVLTask {
    fn from(value: Task) -> Self {
        match value {
            Task::Ocr => Self::Ocr,
            Task::Table => Self::Table,
            Task::Formula => Self::Formula,
            Task::Chart => Self::Chart,
            Task::Spotting => Self::Spotting,
            Task::Seal => Self::Seal,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let image = image::open(cli.input)?;
    koharu_ml::init().await?;
    let model = PaddleOCRVL::load(koharu_ml::device(cli.cpu)).await?;
    let result = model.inference(&image, cli.task.into())?;
    println!("{}", result.text);
    Ok(())
}
