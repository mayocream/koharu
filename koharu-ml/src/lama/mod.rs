mod fft;
mod model;

use anyhow::{Result, bail};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use image::{DynamicImage, GenericImageView, RgbImage};

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
        let (image_tensor, mask_tensor) = self.preprocess(image, mask)?;
        let output = self.forward(&image_tensor, &mask_tensor)?;
        self.postprocess(&output)
    }

    fn preprocess(&self, image: &DynamicImage, mask: &DynamicImage) -> Result<(Tensor, Tensor)> {
        if image.dimensions() != mask.dimensions() {
            bail!(
                "image and mask dimensions dismatch: image is {:?}, mask is {:?}",
                image.dimensions(),
                mask.dimensions()
            );
        }
        let (w, h) = (image.width() as usize, image.height() as usize);

        let rgb = image.to_rgb8().into_raw();
        let mask_gray = mask.to_luma8().into_raw();

        let image_tensor = (Tensor::from_vec(rgb, (1, h, w, 3), &self.device)?
            .permute((0, 3, 1, 2))?
            .to_dtype(DType::F32)?
            * (1. / 255.))?;

        let mask_tensor = (Tensor::from_vec(mask_gray, (1, h, w, 1), &self.device)?
            .permute((0, 3, 1, 2))?
            .to_dtype(DType::F32)?
            * (1. / 255.))?;

        Ok((image_tensor, mask_tensor))
    }

    fn postprocess(&self, output: &Tensor) -> Result<DynamicImage> {
        let output = output.to_device(&Device::Cpu)?;
        let output = output.squeeze(0)?;
        let (channels, height, width) = output.dims3()?;
        if channels != 3 {
            bail!("expected 3 channels in output, got {channels}");
        }
        let output = (output * 255.)?.clamp(0., 255.)?.to_dtype(DType::U8)?;
        let hwc = output.permute((1, 2, 0))?; // HWC for ImageBuffer
        let raw: Vec<u8> = hwc.flatten_all()?.to_vec1()?;
        let image = RgbImage::from_raw(width as u32, height as u32, raw)
            .ok_or_else(|| anyhow::anyhow!("failed to create image buffer from model output"))?;
        Ok(DynamicImage::ImageRgb8(image))
    }
}
