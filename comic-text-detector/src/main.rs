use clap::Parser;
use image::GenericImageView;
use ndarray::Array;
use ort::{
    inputs,
    session::{Session, builder::GraphOptimizationLevel},
};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    image: String,

    #[arg(long, default_value = "comictextdetector.pt.onnx")]
    model: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let model = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(4)?
        .commit_from_file(args.model)?;

    println!("loaded model: {model:?}");

    let image =
        image::open(&args.image).map_err(|e| anyhow::anyhow!("Failed to open image: {e}"))?;

    let image = image.resize_exact(1024, 1024, image::imageops::FilterType::CatmullRom);
    let mut input = Array::zeros((1, 3, 1024, 1024));
    for pixel in image.pixels() {
        let x = pixel.0 as _;
        let y = pixel.1 as _;
        let [r, g, b, _] = pixel.2.0;
        input[[0, 0, y, x]] = (r as f32) / 255.;
        input[[0, 1, y, x]] = (g as f32) / 255.;
        input[[0, 2, y, x]] = (b as f32) / 255.;
    }

    let inputs = inputs!["images" => input.view()]?;
    let outputs = model.run(inputs)?;

    println!("outputs: {outputs:?}");

    Ok(())
}
