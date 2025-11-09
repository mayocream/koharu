use clap::Parser;
use koharu_models::manga_ocr::MangaOCR;
use ort::execution_providers::CUDAExecutionProvider;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ort::init()
        .with_execution_providers([CUDAExecutionProvider::default().build().error_on_failure()])
        .commit()?;

    let cli = Cli::parse();

    let mut model = MangaOCR::new().await?;
    let image = image::open(&cli.input)?;

    let output = model.inference(&image)?;
    println!("{:?}", output);

    Ok(())
}
