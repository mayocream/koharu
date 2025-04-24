use std::thread;

use clap::Parser;
use hf_hub::api::sync::Api;
use image::GenericImageView;
use ndarray::Array;
use ort::{inputs, session::Session};

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = "test.png")]
    image: String,

    #[arg(long, default_value = "mask.png")]
    mask: String,

    #[arg(long, default_value = "output.png")]
    output: String,
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    let api = Api::new()?;
    let repo = api.model("Carve/LaMa-ONNX".to_string());
    let model_path = repo.get("lama_fp32.onnx")?;

    let model = Session::builder()?
        .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
        .with_intra_threads(thread::available_parallelism()?.get())?
        .commit_from_file(model_path)?;

    let image =
        image::open(&args.image).map_err(|e| anyhow::anyhow!("Failed to open image: {e}"))?;
    let image = image.resize_exact(512, 512, image::imageops::FilterType::CatmullRom);

    let mask = image::open(&args.mask).map_err(|e| anyhow::anyhow!("Failed to open mask: {e}"))?;
    let mask = mask.resize_exact(512, 512, image::imageops::FilterType::CatmullRom);

    let (orig_width, orig_height) = image.dimensions();

    // Explicitly specifying f32 data type
    let mut image_data = Array::zeros((1, 3, orig_height as usize, orig_width as usize));
    for y in 0..orig_height {
        for x in 0..orig_width {
            let pixel = image.get_pixel(x, y);
            let [r, g, b, _] = pixel.0;
            // Channel order: RGB
            image_data[[0, 0, y as usize, x as usize]] = (r as f32) / 255.0;
            image_data[[0, 1, y as usize, x as usize]] = (g as f32) / 255.0;
            image_data[[0, 2, y as usize, x as usize]] = (b as f32) / 255.0;
        }
    }

    // Fixed mask interpretation - black pixels (0) are now the area TO inpaint (value 1.0)
    let mut mask_data = Array::zeros((1, 1, orig_height as usize, orig_width as usize));
    for y in 0..orig_height {
        for x in 0..orig_width {
            let pixel = mask.get_pixel(x, y);
            // For LaMa, mask value of 1 indicates area to be inpainted
            // Assuming black pixels (value 0) in the mask are the areas to inpaint
            mask_data[[0, 0, y as usize, x as usize]] = if pixel[0] > 0 { 1.0f32 } else { 0.0f32 };
        }
    }

    let input = inputs![
        "image" => image_data.view(),
        "mask" => mask_data.view(),
    ]?;

    let outputs = model.run(input)?;

    let output = outputs["output"].try_extract_tensor::<f32>()?;
    let output = output.view();

    // Create output image
    let mut output_image = image::RgbImage::new(orig_width, orig_height);
    for y in 0..orig_height {
        for x in 0..orig_width {
            // Make sure the channel mapping is correct: RGB
            let r = (output[[0, 0, y as usize, x as usize]].clamp(0.0, 1.0) * 255.0) as u8;
            let g = (output[[0, 1, y as usize, x as usize]].clamp(0.0, 1.0) * 255.0) as u8;
            let b = (output[[0, 2, y as usize, x as usize]].clamp(0.0, 1.0) * 255.0) as u8;
            output_image.put_pixel(x, y, image::Rgb([r, g, b]));
        }
    }

    output_image
        .save(&args.output)
        .map_err(|e| anyhow::anyhow!("Failed to save output image: {e}"))?;

    println!("Successfully processed and saved image to {}", args.output);

    Ok(())
}
