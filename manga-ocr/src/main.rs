use clap::Parser;
use manga_ocr::MangaOCR;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut model = MangaOCR::new()?;
    let image = image::open(&cli.input)?;

    let output = model.inference(&image)?;
    println!("{:?}", output);

    Ok(())
}
