use clap::Parser;
use koharu_ml::{device, lama::Lama};

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: String,

    #[arg(short, long, value_name = "FILE")]
    mask: String,

    #[arg(short, long, value_name = "FILE")]
    output: String,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let device = device(cli.cpu)?;
    let model = Lama::load(device).await?;
    let image = image::open(&cli.input)?;
    let mask = image::open(&cli.mask)?;

    let output = model.inference(&image, &mask)?;
    output.save(&cli.output)?;

    Ok(())
}
