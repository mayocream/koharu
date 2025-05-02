use std::thread;

use hf_hub::api::sync::Api;
use image::{DynamicImage, GenericImageView};
use ort::{inputs, session::Session};

#[derive(Debug)]
pub struct Lama {
    model: Session,
}

fn resize_with_padding(
    img: &DynamicImage,
    target_size: u32,
    filter: image::imageops::FilterType,
) -> (DynamicImage, (u32, u32, u32, u32)) {
    let (orig_width, orig_height) = img.dimensions();

    // Calculate new dimensions while preserving aspect ratio
    let (new_width, new_height) = if orig_width > orig_height {
        // Width is the longer dimension
        let height = (target_size as f32 * orig_height as f32 / orig_width as f32).round() as u32;
        (target_size, height)
    } else {
        // Height is the longer dimension
        let width = (target_size as f32 * orig_width as f32 / orig_height as f32).round() as u32;
        (width, target_size)
    };

    // Resize the image
    let resized = img.resize(new_width, new_height, filter);

    // Calculate padding needed
    let pad_right = target_size.saturating_sub(new_width);
    let pad_bottom = target_size.saturating_sub(new_height);

    // Create a new image with padding
    let mut padded = DynamicImage::new_rgba8(target_size, target_size);

    // Copy the resized image to the padded image
    image::imageops::replace(&mut padded, &resized, 0, 0);

    // Add reflection padding
    if pad_right > 0 || pad_bottom > 0 {
        let mut buffer = padded.to_rgba8();

        // Add right padding (reflect)
        if pad_right > 0 {
            for y in 0..new_height {
                for x in 0..pad_right {
                    let source_x = new_width.saturating_sub(1 + x % new_width);
                    let pixel = buffer.get_pixel(source_x, y);
                    buffer.put_pixel(new_width + x, y, *pixel);
                }
            }
        }

        // Add bottom padding (reflect)
        if pad_bottom > 0 {
            for y in 0..pad_bottom {
                for x in 0..target_size {
                    let source_y = new_height.saturating_sub(1 + y % new_height);
                    let pixel = buffer.get_pixel(x, source_y);
                    buffer.put_pixel(x, new_height + y, *pixel);
                }
            }
        }

        padded = DynamicImage::ImageRgba8(buffer);
    }

    // Return padded image and padding info for reverting
    (padded, (new_width, new_height, pad_right, pad_bottom))
}

fn revert_resize_padding(
    padded: &DynamicImage,
    original_dimensions: (u32, u32),
    resize_info: (u32, u32, u32, u32),
    filter: image::imageops::FilterType,
) -> DynamicImage {
    let (orig_width, orig_height) = original_dimensions;
    let (resized_width, resized_height, _, _) = resize_info;

    // First crop to remove padding
    let cropped = padded.crop_imm(0, 0, resized_width, resized_height);

    // Then resize back to original dimensions
    cropped.resize(orig_width, orig_height, filter)
}

impl Lama {
    pub fn new() -> anyhow::Result<Self> {
        let api = Api::new()?;
        let repo = api.model("mayocream/koharu".to_string());
        let model_path = repo.get("lama-manga.onnx")?;

        let model = Session::builder()?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
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
        let (image, resize_info) =
            resize_with_padding(&image, 512, image::imageops::FilterType::CatmullRom);
        let (mask, _) = resize_with_padding(&mask, 512, image::imageops::FilterType::CatmullRom);

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
                let r = (output[[0, 0, y, x]] * 255.0).clamp(0.0, 255.0).round() as u8;
                let g = (output[[0, 1, y, x]] * 255.0).clamp(0.0, 255.0).round() as u8;
                let b = (output[[0, 2, y, x]] * 255.0).clamp(0.0, 255.0).round() as u8;
                output_image.put_pixel(x as u32, y as u32, image::Rgb([r, g, b]));
            }
        }

        let mut output_image = DynamicImage::ImageRgb8(output_image);
        output_image = revert_resize_padding(
            &output_image,
            (orig_width, orig_height),
            resize_info,
            image::imageops::FilterType::CatmullRom,
        );

        Ok(output_image)
    }
}
