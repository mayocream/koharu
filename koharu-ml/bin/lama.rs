use clap::Parser;
use koharu_ml::lama::Lama;

#[path = "common.rs"]
mod common;

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
    common::init_tracing();

    let cli = Cli::parse();

    let model = Lama::load(cli.cpu).await?;
    let image = image::open(&cli.input)?;
    let mask = image::open(&cli.mask)?;

    // inferernce start time
    let start = std::time::Instant::now();

    let output = model.inference(&image, &mask)?;

    // measure inference speed
    let duration = start.elapsed();
    println!("Inference took: {:?}", duration);

    output.save(&cli.output)?;

    Ok(())
}
