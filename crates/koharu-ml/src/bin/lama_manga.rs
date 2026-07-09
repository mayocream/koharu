use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use koharu_ml::lama_manga::LamaManga;

#[derive(Debug, Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE")]
    input: PathBuf,

    #[arg(short, long, value_name = "FILE")]
    mask: PathBuf,

    #[arg(short, long, value_name = "FILE")]
    output: PathBuf,

    #[arg(long, default_value_t = false)]
    cpu: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let image = image::open(&cli.input)?;
    let mask = image::open(&cli.mask)?.to_luma8();

    koharu_ml::init().await?;

    let model = LamaManga::load(cli.cpu).await?;
    let inpainted = model.inpaint(&image, &mask)?;
    inpainted.save(cli.output)?;

    Ok(())
}
