use anyhow::{Result, bail};
use candle_core::{DType, Device, Tensor};
use image::{DynamicImage, GrayImage, Rgb, RgbImage};
use imageproc::{distance_transform::Norm, morphology::dilate};

pub const VAE_SCALE_FACTOR: u32 = 8;
pub const LATENT_PACK_FACTOR: u32 = 2;
pub const IMAGE_MULTIPLE: u32 = VAE_SCALE_FACTOR * LATENT_PACK_FACTOR;

#[derive(Debug, Clone, Copy)]
pub struct PreparedSize {
    pub width: u32,
    pub height: u32,
    pub original_width: u32,
    pub original_height: u32,
}

pub fn bounded_size(width: u32, height: u32, max_pixels: u32) -> (u32, u32) {
    if max_pixels == 0 || width.saturating_mul(height) <= max_pixels {
        return (width, height);
    }
    let scale = (max_pixels as f64 / (width as f64 * height as f64)).sqrt();
    (
        ((width as f64 * scale).floor() as u32).max(IMAGE_MULTIPLE),
        ((height as f64 * scale).floor() as u32).max(IMAGE_MULTIPLE),
    )
}

pub fn round_to_flux_multiple(width: u32, height: u32) -> (u32, u32) {
    let round = |v: u32| (v / IMAGE_MULTIPLE).max(1) * IMAGE_MULTIPLE;
    (round(width), round(height))
}

pub fn prepare_rgb_image(image: &DynamicImage, max_pixels: u32) -> (RgbImage, PreparedSize) {
    let original_width = image.width();
    let original_height = image.height();
    let (width, height) = bounded_size(original_width, original_height, max_pixels);
    let (width, height) = round_to_flux_multiple(width, height);
    let rgb = image.to_rgb8();
    let resized = if width == original_width && height == original_height {
        rgb
    } else {
        image::imageops::resize(&rgb, width, height, image::imageops::FilterType::Lanczos3)
    };
    (
        resized,
        PreparedSize {
            width,
            height,
            original_width,
            original_height,
        },
    )
}

pub fn prepare_mask(mask: &DynamicImage, width: u32, height: u32) -> GrayImage {
    let gray = mask.to_luma8();
    image::imageops::resize(&gray, width, height, image::imageops::FilterType::Triangle)
}

pub fn expand_mask(mask: &GrayImage, padding: u8) -> GrayImage {
    if padding == 0 {
        mask.clone()
    } else {
        dilate(mask, Norm::LInf, padding)
    }
}

pub fn image_to_tensor(image: &RgbImage, device: &Device) -> candle_core::Result<Tensor> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let mut data = Vec::with_capacity(3 * width * height);
    for channel in 0..3 {
        for pixel in image.pixels() {
            data.push(pixel.0[channel] as f32 / 127.5 - 1.0);
        }
    }
    Tensor::from_vec(data, (1, 3, height, width), device)
}

pub fn tensor_to_rgb_image(tensor: &Tensor) -> Result<RgbImage> {
    let tensor = tensor
        .to_device(&Device::Cpu)?
        .to_dtype(DType::F32)?
        .clamp(-1.0, 1.0)?;
    let tensor = if tensor.rank() == 4 {
        tensor.squeeze(0)?
    } else {
        tensor
    };
    let (channels, height, width) = tensor.dims3()?;
    if channels != 3 {
        bail!("expected 3 image channels, found {channels}");
    }
    let data = tensor.flatten_all()?.to_vec1::<f32>()?;
    let mut image = RgbImage::new(width as u32, height as u32);
    let plane = width * height;
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let r = denorm_pixel(data[idx]);
            let g = denorm_pixel(data[plane + idx]);
            let b = denorm_pixel(data[2 * plane + idx]);
            image.put_pixel(x as u32, y as u32, Rgb([r, g, b]));
        }
    }
    Ok(image)
}

fn denorm_pixel(value: f32) -> u8 {
    ((value + 1.0) * 127.5).round().clamp(0.0, 255.0) as u8
}

pub fn mask_to_packed_tensor(
    mask: &GrayImage,
    packed_height: usize,
    packed_width: usize,
    device: &Device,
) -> candle_core::Result<Tensor> {
    let resized = image::imageops::resize(
        mask,
        packed_width as u32,
        packed_height as u32,
        image::imageops::FilterType::Triangle,
    );
    let mut data = Vec::with_capacity(packed_height * packed_width);
    for y in 0..packed_height {
        for x in 0..packed_width {
            data.push(resized.get_pixel(x as u32, y as u32).0[0] as f32 / 255.0);
        }
    }
    Tensor::from_vec(data, (1, packed_height * packed_width, 1), device)
}

pub fn patchify_latents(latents: &Tensor) -> candle_core::Result<Tensor> {
    let (b, c, h, w) = latents.dims4()?;
    if h % 2 != 0 || w % 2 != 0 {
        candle_core::bail!("latent size must be even, got {h}x{w}");
    }
    latents
        .reshape((b, c, h / 2, 2, w / 2, 2))?
        .permute((0, 1, 3, 5, 2, 4))?
        .reshape((b, c * 4, h / 2, w / 2))
}

pub fn unpatchify_latents(latents: &Tensor) -> candle_core::Result<Tensor> {
    let (b, c, h, w) = latents.dims4()?;
    if c % 4 != 0 {
        candle_core::bail!("patchified latent channels must be divisible by 4, got {c}");
    }
    latents
        .reshape((b, c / 4, 2, 2, h, w))?
        .permute((0, 1, 4, 2, 5, 3))?
        .reshape((b, c / 4, h * 2, w * 2))
}

pub fn pack_latents(latents: &Tensor) -> candle_core::Result<Tensor> {
    let (b, c, h, w) = latents.dims4()?;
    latents.permute((0, 2, 3, 1))?.reshape((b, h * w, c))
}

pub fn unpack_latents(
    latents: &Tensor,
    height: usize,
    width: usize,
) -> candle_core::Result<Tensor> {
    let (b, seq, c) = latents.dims3()?;
    if seq != height * width {
        candle_core::bail!(
            "latent sequence length {seq} does not match packed size {}",
            height * width
        );
    }
    latents
        .reshape((b, height, width, c))?
        .permute((0, 3, 1, 2))
}

pub fn prepare_text_ids(batch: usize, len: usize, device: &Device) -> candle_core::Result<Tensor> {
    let mut ids = Vec::with_capacity(batch * len * 4);
    for _ in 0..batch {
        for l in 0..len {
            ids.extend_from_slice(&[0f32, 0f32, 0f32, l as f32]);
        }
    }
    Tensor::from_vec(ids, (batch, len, 4), device)
}

pub fn prepare_latent_ids(
    batch: usize,
    height: usize,
    width: usize,
    time_index: usize,
    device: &Device,
) -> candle_core::Result<Tensor> {
    let mut ids = Vec::with_capacity(batch * height * width * 4);
    for _ in 0..batch {
        for h in 0..height {
            for w in 0..width {
                ids.extend_from_slice(&[time_index as f32, h as f32, w as f32, 0f32]);
            }
        }
    }
    Tensor::from_vec(ids, (batch, height * width, 4), device)
}

pub fn resize_back_if_needed(image: RgbImage, size: PreparedSize) -> DynamicImage {
    let out = if image.width() == size.original_width && image.height() == size.original_height {
        image
    } else {
        image::imageops::resize(
            &image,
            size.original_width,
            size.original_height,
            image::imageops::FilterType::Lanczos3,
        )
    };
    DynamicImage::ImageRgb8(out)
}
