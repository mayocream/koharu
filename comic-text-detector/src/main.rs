use clap::Parser;
use ort::{Environment, SessionBuilder};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    image: String,

    #[arg(long, default_value = "comictextdetector.pt.onnx")]
    model: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let env = std::sync::Arc::new(
        Environment::builder()
            .with_name("comic-text-detector")
            .build()?,
    );

    let session = SessionBuilder::new(&env)?.with_model_from_file(&args.model)?;

    println!("loaded model: {session:?}");

    Ok(())
}
