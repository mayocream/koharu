use clap::Parser;
use lama::Lama;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(short, long, value_name = "FILE")]
    mask: String,

    #[arg(short, long, value_name = "FILE")]
    output: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut model = Lama::new()?;
    let image = image::open(&cli.input)?;
    let mask = image::open(&cli.mask)?;

    let output = model.inference(&image, &mask)?;
    output.save(&cli.output)?;

    Ok(())
}
