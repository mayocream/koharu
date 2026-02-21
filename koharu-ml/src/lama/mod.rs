mod fft;
mod model;

use anyhow::{Result, bail};
use candle_core::{DType, Device, Tensor};
use image::{DynamicImage, GenericImageView, RgbImage};
use tracing::instrument;

use crate::{define_models, device, loading};

define_models! {
    Lama => ("mayocream/lama-manga", "lama-manga.safetensors"),
}

pub struct Lama {
    model: model::Lama,
    device: Device,
}

impl Lama {
    pub async fn load(use_cpu: bool) -> Result<Self> {
        let device = device(use_cpu)?;
        let model = loading::load_buffered_safetensors(Manifest::Lama.get(), &device, |vb| {
            model::Lama::load(&vb)
        })
        .await?;

        Ok(Self { model, device })
    }

    #[instrument(level = "debug", skip_all)]
    fn forward(&self, image: &Tensor, mask: &Tensor) -> Result<Tensor> {
        self.model.forward(image, mask)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(&self, image: &DynamicImage, mask: &DynamicImage) -> Result<DynamicImage> {
        let (image_tensor, mask_tensor) = self.preprocess(image, mask)?;
        let output = self.forward(&image_tensor, &mask_tensor)?;
        self.postprocess(&output)
    }

    #[instrument(level = "debug", skip_all)]
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
        let luma = mask.to_luma8().into_raw();

        let image_tensor = (Tensor::from_vec(rgb, (1, h, w, 3), &self.device)?
            .permute((0, 3, 1, 2))?
            .to_dtype(DType::F32)?
            * (1. / 255.))?;

        let mask_tensor = Tensor::from_vec(luma, (1, h, w, 1), &self.device)?
            .permute((0, 3, 1, 2))?
            .to_dtype(DType::F32)?
            .gt(1.0f32)?;

        Ok((image_tensor, mask_tensor))
    }

    #[instrument(level = "debug", skip_all)]
    fn postprocess(&self, output: &Tensor) -> Result<DynamicImage> {
        let output = output.squeeze(0)?;
        let (channels, height, width) = output.dims3()?;
        if channels != 3 {
            bail!("expected 3 channels in output, got {channels}");
        }
        let output = (output * 255.)?
            .clamp(0., 255.)?
            .permute((1, 2, 0))?
            .to_dtype(DType::U8)?;
        let raw: Vec<u8> = output.flatten_all()?.to_vec1()?;
        let image = RgbImage::from_raw(width as u32, height as u32, raw)
            .ok_or_else(|| anyhow::anyhow!("failed to create image buffer from model output"))?;
        Ok(DynamicImage::ImageRgb8(image))
    }
}
