mod fft;
mod model;

use anyhow::{Result, bail};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use image::{DynamicImage, GenericImageView, ImageBuffer, RgbImage};

use crate::hf_hub;

pub struct Lama {
    model: model::Lama,
    device: Device,
}

impl Lama {
    pub async fn load(device: Device) -> Result<Self> {
        let weights = hf_hub("mayocream/lama-manga", "lama-manga.safetensors").await?;
        let data = std::fs::read(&weights)?;
        let vb = VarBuilder::from_buffered_safetensors(data, DType::F32, &device)?;
        let model = model::Lama::load(&vb)?;

        Ok(Self { model, device })
    }

    fn forward(&self, image: &Tensor, mask: &Tensor) -> Result<Tensor> {
        self.model.forward(image, mask)
    }

    pub fn inference(&self, image: &DynamicImage, mask: &DynamicImage) -> Result<DynamicImage> {
        let (w, h) = image.dimensions();
        if mask.dimensions() != (w, h) {
            bail!(
                "image and mask dimensions must match: image=({w}, {h}), mask={:?}",
                mask.dimensions()
            );
        }
        let (image_tensor, mask_tensor) = self.preprocess(image, mask)?;
        let output = self.forward(&image_tensor, &mask_tensor)?;
        self.postprocess(&output)
    }

    fn preprocess(&self, image: &DynamicImage, mask: &DynamicImage) -> Result<(Tensor, Tensor)> {
        let device = &self.device;
        let rgb = image.to_rgb8();
        let mask = mask.to_luma8();
        let (w, h) = rgb.dimensions();
        let (w_usize, h_usize) = (w as usize, h as usize);

        let mut image_data = Vec::with_capacity(3 * w_usize * h_usize);
        for c in 0..3 {
            for y in 0..h {
                for x in 0..w {
                    let p = rgb.get_pixel(x, y);
                    image_data.push(p[c] as f32 / 255.0);
                }
            }
        }

        let mut mask_data = Vec::with_capacity(w_usize * h_usize);
        for y in 0..h {
            for x in 0..w {
                let v = if mask.get_pixel(x, y)[0] > 0 {
                    1.0f32
                } else {
                    0.0f32
                };
                mask_data.push(v);
            }
        }

        let image_tensor = Tensor::from_vec(image_data, (1, 3, h_usize, w_usize), device)?;
        let mask_tensor = Tensor::from_vec(mask_data, (1, 1, h_usize, w_usize), device)?;
        Ok((image_tensor, mask_tensor))
    }

    fn postprocess(&self, output: &Tensor) -> Result<DynamicImage> {
        let output = output.to_device(&Device::Cpu)?;
        let output = output.squeeze(0)?;
        let (channels, height, width) = output.dims3()?;
        if channels != 3 {
            bail!("expected 3 channels in output, got {channels}");
        }
        let flat: Vec<f32> = output.flatten_all()?.to_vec1()?;
        let hw = height * width;

        let mut image: RgbImage = ImageBuffer::new(width as u32, height as u32);
        for y in 0..height {
            for x in 0..width {
                let base = y * width + x;
                let r = (flat[base] * 255.0).clamp(0.0, 255.0) as u8;
                let g = (flat[base + hw] * 255.0).clamp(0.0, 255.0) as u8;
                let b = (flat[base + 2 * hw] * 255.0).clamp(0.0, 255.0) as u8;
                image.put_pixel(x as u32, y as u32, image::Rgb([r, g, b]));
            }
        }

        Ok(DynamicImage::ImageRgb8(image))
    }
}
