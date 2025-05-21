use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let model = manga_ocr::MangaOCR::new()?;
    let image = image::open(&cli.input)?;

    let output = model.inference(&image)?;
    println!("{:?}", output);

    Ok(())
}
