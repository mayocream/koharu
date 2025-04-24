use std::thread;

use hf_hub::api::sync::Api;
use image::{DynamicImage, GenericImageView};
use ort::{
    execution_providers::{
        CUDAExecutionProvider, CoreMLExecutionProvider, DirectMLExecutionProvider,
    },
    inputs,
    session::Session,
};

#[derive(Debug)]
pub struct Lama {
    model: Session,
}

impl Lama {
    pub fn new() -> anyhow::Result<Self> {
        let api = Api::new()?;
        let repo = api.model("Carve/LaMa-ONNX".to_string());
        let model_path = repo.get("lama_fp32.onnx")?;

        let model = Session::builder()?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
            .with_execution_providers([
                CUDAExecutionProvider::default().build(),
                // Use DirectML on Windows if NVIDIA EPs are not available
                DirectMLExecutionProvider::default().build(),
                // Or use ANE on Apple platforms
                CoreMLExecutionProvider::default().build(),
            ])?
            .with_intra_threads(thread::available_parallelism()?.get())?
            .commit_from_file(model_path)?;

        Ok(Lama { model })
    }

    pub fn inference(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
    ) -> anyhow::Result<DynamicImage> {
        let (orig_width, orig_height) = image.dimensions();
        let image = image.resize_exact(512, 512, image::imageops::FilterType::CatmullRom);
        let mask = mask.resize_exact(512, 512, image::imageops::FilterType::CatmullRom);

        let mut image_data = ndarray::Array::zeros((1, 3, 512, 512));
        for pixel in image.pixels() {
            let (x, y, pixel) = pixel;
            let x = x as usize;
            let y = y as usize;

            // Channel order: RGB
            image_data[[0, 0, y, x]] = (pixel[0] as f32) / 255.0;
            image_data[[0, 1, y, x]] = (pixel[1] as f32) / 255.0;
            image_data[[0, 2, y, x]] = (pixel[2] as f32) / 255.0;
        }

        // Fixed mask interpretation - black pixels (0) are now the area TO inpaint (value 1.0)
        let mut mask_data = ndarray::Array::zeros((1, 1, 512, 512));
        for pixel in mask.pixels() {
            let (x, y, pixel) = pixel;
            let x = x as usize;
            let y = y as usize;

            // For LaMa, mask value of 1 indicates area to be inpainted
            mask_data[[0, 0, y, x]] = if pixel[0] > 0 { 1.0f32 } else { 0.0f32 };
        }

        let inputs = inputs![
            "image" => image_data.view(),
            "mask" => mask_data.view(),
        ]?;
        let outputs = self.model.run(inputs)?;
        let output = outputs["output"].try_extract_tensor::<f32>()?;
        let output = output.view();

        let mut output_image = image::RgbImage::new(512, 512);
        for y in 0..512 {
            for x in 0..512 {
                let r = (output[[0, 0, y, x]]).clamp(0.0, 255.0).round() as u8;
                let g = (output[[0, 1, y, x]]).clamp(0.0, 255.0).round() as u8;
                let b = (output[[0, 2, y, x]]).clamp(0.0, 255.0).round() as u8;
                output_image.put_pixel(x as u32, y as u32, image::Rgb([r, g, b]));
            }
        }

        // Resize back to original dimensions
        output_image = image::imageops::resize(
            &output_image,
            orig_width,
            orig_height,
            image::imageops::FilterType::CatmullRom,
        );

        Ok(DynamicImage::ImageRgb8(output_image))
    }
}
