use clap::Parser;
use koharu_ml::{device, manga_ocr::MangaOcr};

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let image = image::open(&cli.input)?;

    let device = device(cli.cpu)?;

    let model = MangaOcr::load(device).await?;
    let output = model.inference(&image)?;

    println!("{output}");

    Ok(())
}
