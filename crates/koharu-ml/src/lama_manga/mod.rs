mod model;

use anyhow::{Context, Result};
use image::{DynamicImage, GrayImage, RgbImage};
use koharu_runtime::package::huggingface;
use koharu_torch::{Device, Kind, Tensor};

use crate::device;

use self::model::LamaMangaModel;

koharu_runtime::huggingface! {
    WEIGHTS => "mayocream/lama-manga" => "lama-manga.safetensors",
}

#[derive(Debug)]
pub struct LamaManga {
    device: Device,
    model: LamaMangaModel,
}

impl LamaManga {
    pub async fn load(cpu: bool) -> Result<Self> {
        let device: Device = device(cpu).try_into()?;
        let weights_path = huggingface::resolve(WEIGHTS)
            .await
            .context("failed to resolve lama-manga weights")?;
        let mut model = LamaMangaModel::new(device);
        model
            .load_safetensors(&weights_path)
            .context("failed to load lama-manga safetensors")?;
        Ok(Self { device, model })
    }

    pub fn inpaint(&self, image: &DynamicImage, mask: &GrayImage) -> Result<RgbImage> {
        let rgb = image.to_rgb8();
        anyhow::ensure!(
            rgb.dimensions() == mask.dimensions(),
            "image and mask dimensions differ: image={:?}, mask={:?}",
            rgb.dimensions(),
            mask.dimensions()
        );

        koharu_torch::no_grad(|| {
            let input = PreparedInput::new(&rgb, mask, self.device);
            let output = self.model.forward(&input.image, &input.mask);
            tensor_to_rgb_image(&output, rgb.width(), rgb.height())
        })
    }

    pub fn device(&self) -> Device {
        self.device
    }
}

#[derive(Debug)]
struct PreparedInput {
    image: Tensor,
    mask: Tensor,
}

impl PreparedInput {
    fn new(image: &RgbImage, mask: &GrayImage, device: Device) -> Self {
        let width = image.width() as usize;
        let height = image.height() as usize;
        let padded_width = ceil_modulo(width, 8);
        let padded_height = ceil_modulo(height, 8);

        let mut image_values = vec![0.0f32; 3 * padded_height * padded_width];
        let mut mask_values = vec![0.0f32; padded_height * padded_width];

        for y in 0..padded_height {
            let source_y = symmetric_index(y, height);
            for x in 0..padded_width {
                let source_x = symmetric_index(x, width);
                let dst = y * padded_width + x;
                let pixel = image.get_pixel(source_x as u32, source_y as u32).0;
                image_values[dst] = f32::from(pixel[0]) / 255.0;
                image_values[padded_height * padded_width + dst] = f32::from(pixel[1]) / 255.0;
                image_values[2 * padded_height * padded_width + dst] = f32::from(pixel[2]) / 255.0;

                let mask_value = mask.get_pixel(source_x as u32, source_y as u32).0[0];
                mask_values[dst] = if mask_value > 0 { 1.0 } else { 0.0 };
            }
        }

        let image = Tensor::from_slice(&image_values)
            .view([1, 3, padded_height as i64, padded_width as i64])
            .to_device(device);
        let mask = Tensor::from_slice(&mask_values)
            .view([1, 1, padded_height as i64, padded_width as i64])
            .to_device(device);

        Self { image, mask }
    }
}

fn tensor_to_rgb_image(tensor: &Tensor, width: u32, height: u32) -> Result<RgbImage> {
    let tensor = tensor
        .squeeze_dim(0)
        .slice(1, 0, height as i64, 1)
        .slice(2, 0, width as i64, 1)
        .clamp(0.0, 1.0)
        * 255.0;
    let tensor = tensor
        .to_device(Device::Cpu)
        .to_kind(Kind::Uint8)
        .contiguous();
    let plane = width as usize * height as usize;
    let mut chw = vec![0u8; plane * 3];
    tensor.copy_data(&mut chw, plane * 3);

    let mut rgb = vec![0u8; plane * 3];
    for index in 0..plane {
        rgb[index * 3] = chw[index];
        rgb[index * 3 + 1] = chw[plane + index];
        rgb[index * 3 + 2] = chw[plane * 2 + index];
    }

    RgbImage::from_raw(width, height, rgb)
        .context("failed to convert lama-manga tensor to RGB image")
}

fn ceil_modulo(value: usize, modulo: usize) -> usize {
    if value % modulo == 0 {
        value
    } else {
        (value / modulo + 1) * modulo
    }
}

fn symmetric_index(index: usize, len: usize) -> usize {
    if index < len {
        index
    } else {
        (2 * len).saturating_sub(index + 1).min(len - 1)
    }
}
