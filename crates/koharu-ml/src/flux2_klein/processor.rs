//! FLUX.2 Klein request preparation and image postprocessing.
//!
//! The 16-pixel image multiple, one-megapixel reference-image bound, Euler
//! flow schedule, and four-step distilled defaults map to:
//! https://github.com/huggingface/diffusers/blob/a37f6f8394ac2a7ee8360c3abea811efe54512b1/src/diffusers/pipelines/flux2/pipeline_flux2_klein.py

use anyhow::{Result, bail, ensure};
use image::{DynamicImage, GenericImageView, GrayImage, RgbImage};
use imageproc::{distance_transform::Norm, morphology::dilate};
use koharu_diffusion::{
    GuidanceParams, ImageGenerationParams, SampleMethod, SampleParams, Scheduler,
};

pub(super) const IMAGE_MULTIPLE: u32 = 16;
const INPAINT_CROP_CONTEXT: u32 = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flux2InferenceOptions {
    /// Requested output width. It is rounded down to a multiple of 16.
    pub width: u32,
    /// Requested output height. It is rounded down to a multiple of 16.
    pub height: u32,
    pub num_inference_steps: i32,
    pub seed: i64,
    pub batch_count: i32,
}

impl Default for Flux2InferenceOptions {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 1024,
            num_inference_steps: 4,
            seed: -1,
            batch_count: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Flux2InpaintOptions {
    pub num_inference_steps: usize,
    pub strength: f64,
    pub max_pixels: u32,
    pub mask_padding: u8,
}

impl Default for Flux2InpaintOptions {
    fn default() -> Self {
        Self {
            num_inference_steps: 4,
            strength: 1.0,
            max_pixels: 1024 * 1024,
            mask_padding: 16,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Flux2ImageToImageOptions {
    pub num_inference_steps: usize,
    pub strength: f64,
    pub max_pixels: u32,
}

impl Default for Flux2ImageToImageOptions {
    fn default() -> Self {
        Self {
            num_inference_steps: 4,
            strength: 1.0,
            max_pixels: 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CropBounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OriginalSize {
    pub width: u32,
    pub height: u32,
}

pub(super) fn generation_params(
    prompt: &str,
    width: u32,
    height: u32,
    steps: i32,
    seed: i64,
    batch_count: i32,
) -> Result<ImageGenerationParams> {
    ensure!(
        !prompt.contains('\0'),
        "prompt contains an interior NUL byte"
    );
    ensure!(width > 0 && height > 0, "image dimensions must be non-zero");
    ensure!(steps > 0, "num_inference_steps must be greater than zero");
    ensure!(batch_count > 0, "batch_count must be greater than zero");

    let width = round_to_flux_multiple(width);
    let height = round_to_flux_multiple(height);
    Ok(ImageGenerationParams {
        prompt: prompt.to_owned(),
        width: i32::try_from(width)?,
        height: i32::try_from(height)?,
        sample: SampleParams {
            guidance: GuidanceParams {
                // FLUX.2 Klein 4B is step-wise distilled. Diffusers ignores
                // classifier-free guidance for this checkpoint; scale 1 keeps
                // the native path to one conditional denoiser evaluation.
                text_cfg: 1.0,
                ..GuidanceParams::default()
            },
            scheduler: Scheduler::Flux2,
            sample_method: SampleMethod::Euler,
            sample_steps: steps,
            ..SampleParams::default()
        },
        seed,
        batch_count,
        auto_resize_reference_images: true,
        ..ImageGenerationParams::default()
    })
}

pub(super) fn prepare_image(
    image: &DynamicImage,
    max_pixels: u32,
) -> Result<(RgbImage, OriginalSize)> {
    ensure!(
        image.width() > 0 && image.height() > 0,
        "image dimensions must be non-zero"
    );
    let original = OriginalSize {
        width: image.width(),
        height: image.height(),
    };
    let (width, height) = bounded_size(image.width(), image.height(), max_pixels);
    let width = round_to_flux_multiple(width);
    let height = round_to_flux_multiple(height);
    let rgb = image.to_rgb8();
    let rgb = if rgb.dimensions() == (width, height) {
        rgb
    } else {
        image::imageops::resize(&rgb, width, height, image::imageops::FilterType::Lanczos3)
    };
    Ok((rgb, original))
}

pub(super) fn prepare_mask(mask: &DynamicImage, width: u32, height: u32, padding: u8) -> GrayImage {
    let mask = image::imageops::resize(
        &mask.to_luma8(),
        width,
        height,
        image::imageops::FilterType::Triangle,
    );
    if padding == 0 {
        mask
    } else {
        dilate(&mask, Norm::LInf, padding)
    }
}

pub(super) fn resize_to_original(image: RgbImage, size: OriginalSize) -> RgbImage {
    if image.dimensions() == (size.width, size.height) {
        image
    } else {
        image::imageops::resize(
            &image,
            size.width,
            size.height,
            image::imageops::FilterType::Lanczos3,
        )
    }
}

pub(super) fn restore_alpha(image: RgbImage, original: &DynamicImage) -> DynamicImage {
    if !original.color().has_alpha() {
        return DynamicImage::ImageRgb8(image);
    }
    let alpha = original.to_rgba8();
    let mut output = DynamicImage::ImageRgb8(image).to_rgba8();
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        pixel.0[3] = alpha.get_pixel(x, y).0[3];
    }
    DynamicImage::ImageRgba8(output)
}

pub(super) fn inpaint_crop_bounds(
    image: &DynamicImage,
    mask: &DynamicImage,
    mask_padding: u8,
) -> Result<Option<CropBounds>> {
    ensure!(
        image.dimensions() == mask.dimensions(),
        "image/mask dimensions differ: image={:?}, mask={:?}",
        image.dimensions(),
        mask.dimensions()
    );
    ensure!(
        image.width() > 0 && image.height() > 0,
        "image dimensions must be non-zero"
    );

    let mask = mask.to_luma8();
    let mut left = mask.width();
    let mut top = mask.height();
    let mut right = 0;
    let mut bottom = 0;
    for (x, y, pixel) in mask.enumerate_pixels() {
        if pixel.0[0] == 0 {
            continue;
        }
        left = left.min(x);
        top = top.min(y);
        right = right.max(x + 1);
        bottom = bottom.max(y + 1);
    }
    if right <= left || bottom <= top {
        return Ok(None);
    }

    let context = INPAINT_CROP_CONTEXT.max(u32::from(mask_padding));
    let x = (left.saturating_sub(context) / IMAGE_MULTIPLE) * IMAGE_MULTIPLE;
    let y = (top.saturating_sub(context) / IMAGE_MULTIPLE) * IMAGE_MULTIPLE;
    let right = right
        .saturating_add(context)
        .min(image.width())
        .div_ceil(IMAGE_MULTIPLE)
        .saturating_mul(IMAGE_MULTIPLE)
        .min(image.width());
    let bottom = bottom
        .saturating_add(context)
        .min(image.height())
        .div_ceil(IMAGE_MULTIPLE)
        .saturating_mul(IMAGE_MULTIPLE)
        .min(image.height());
    if right <= x || bottom <= y {
        bail!("computed FLUX.2 inpaint crop is empty");
    }
    Ok(Some(CropBounds {
        x,
        y,
        width: right - x,
        height: bottom - y,
    }))
}

pub(super) fn composite_crop(
    original: &DynamicImage,
    generated: RgbImage,
    mask: &DynamicImage,
    bounds: CropBounds,
) -> Result<DynamicImage> {
    ensure!(
        generated.dimensions() == (bounds.width, bounds.height),
        "generated crop dimensions differ: generated={:?}, expected={}x{}",
        generated.dimensions(),
        bounds.width,
        bounds.height
    );
    let mask = mask.to_luma8();
    ensure!(
        mask.dimensions() == generated.dimensions(),
        "generated crop/mask dimensions differ"
    );

    let keep_alpha = original.color().has_alpha();
    let mut output = original.to_rgba8();
    for y in 0..bounds.height {
        for x in 0..bounds.width {
            let amount = f32::from(mask.get_pixel(x, y).0[0]) / 255.0;
            if amount == 0.0 {
                continue;
            }
            let source = generated.get_pixel(x, y).0;
            let target = output.get_pixel_mut(bounds.x + x, bounds.y + y);
            for (channel, source) in source.iter().enumerate().take(3) {
                target.0[channel] = (f32::from(target.0[channel]) * (1.0 - amount)
                    + f32::from(*source) * amount)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
    if keep_alpha {
        Ok(DynamicImage::ImageRgba8(output))
    } else {
        Ok(DynamicImage::ImageRgb8(
            DynamicImage::ImageRgba8(output).to_rgb8(),
        ))
    }
}

fn bounded_size(width: u32, height: u32, max_pixels: u32) -> (u32, u32) {
    let pixels = u64::from(width) * u64::from(height);
    if max_pixels == 0 || pixels <= u64::from(max_pixels) {
        return (width, height);
    }
    let scale = (f64::from(max_pixels) / (f64::from(width) * f64::from(height))).sqrt();
    (
        (f64::from(width) * scale).floor().max(1.0) as u32,
        (f64::from(height) * scale).floor().max(1.0) as u32,
    )
}

fn round_to_flux_multiple(value: u32) -> u32 {
    (value / IMAGE_MULTIPLE).max(1) * IMAGE_MULTIPLE
}

#[cfg(test)]
mod tests {
    use image::{Luma, Rgb};

    use super::*;

    #[test]
    fn uses_distilled_diffusers_defaults() {
        let params = generation_params("a cat", 1025, 1019, 4, 7, 2).unwrap();
        assert_eq!((params.width, params.height), (1024, 1008));
        assert_eq!(params.sample.sample_method, SampleMethod::Euler);
        assert_eq!(params.sample.scheduler, Scheduler::Flux2);
        assert_eq!(params.sample.sample_steps, 4);
        assert_eq!(params.sample.guidance.text_cfg, 1.0);
        assert_eq!(params.seed, 7);
        assert_eq!(params.batch_count, 2);
        assert!(params.auto_resize_reference_images);
    }

    #[test]
    fn bounds_large_images_and_preserves_aspect_ratio() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(2048, 1024));
        let (prepared, original) = prepare_image(&image, 1024 * 1024).unwrap();
        assert_eq!(
            original,
            OriginalSize {
                width: 2048,
                height: 1024
            }
        );
        assert_eq!(prepared.dimensions(), (1440, 720));
    }

    #[test]
    fn finds_aligned_mask_crop() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(256, 256));
        let mut mask = GrayImage::new(256, 256);
        mask.put_pixel(130, 140, Luma([255]));
        let bounds = inpaint_crop_bounds(&image, &DynamicImage::ImageLuma8(mask), 16)
            .unwrap()
            .unwrap();
        assert_eq!(
            bounds,
            CropBounds {
                x: 64,
                y: 64,
                width: 144,
                height: 144
            }
        );
    }

    #[test]
    fn composites_only_masked_pixels() {
        let original = DynamicImage::ImageRgb8(RgbImage::from_pixel(2, 1, Rgb([10, 20, 30])));
        let generated = RgbImage::from_pixel(1, 1, Rgb([110, 120, 130]));
        let mask = DynamicImage::ImageLuma8(GrayImage::from_pixel(1, 1, Luma([255])));
        let output = composite_crop(
            &original,
            generated,
            &mask,
            CropBounds {
                x: 1,
                y: 0,
                width: 1,
                height: 1,
            },
        )
        .unwrap()
        .to_rgb8();
        assert_eq!(output.get_pixel(0, 0).0, [10, 20, 30]);
        assert_eq!(output.get_pixel(1, 0).0, [110, 120, 130]);
    }
}
