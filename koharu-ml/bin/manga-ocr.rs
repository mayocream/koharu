use clap::Parser;
use koharu_ml::{device, manga_ocr::MangaOcr};
use tracing_subscriber::fmt::format::FmtSpan;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .init();

    let cli = Cli::parse();
    let image = image::open(&cli.input)?;
    let images = vec![image];

    let device = device(cli.cpu)?;

    let model = MangaOcr::load(device).await?;
    let output = model
        .inference(&images)?
        .into_iter()
        .next()
        .unwrap_or_default();

    println!("{output}");

    Ok(())
}
