use clap::Parser;
use koharu_models::manga_ocr_candle::MangaOcr;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let image = image::open(&cli.input)?;

    let model = MangaOcr::new()?;
    let output = model.infer(&image)?;
    println!("{output}");

    Ok(())
}
