//! FLUX.2 Klein 4B generation and editing on top of `koharu-diffusion`.
//!
//! Upstream pipeline:
//! https://github.com/huggingface/diffusers/blob/a37f6f8394ac2a7ee8360c3abea811efe54512b1/src/diffusers/pipelines/flux2/pipeline_flux2_klein.py

mod model;
mod processor;

use anyhow::{Context, Result, ensure};
use image::{DynamicImage, RgbImage};
use koharu_runtime::package::huggingface;

use self::{
    model::{Model, ModelPaths},
    processor::{
        composite_crop, generation_params, inpaint_crop_bounds, prepare_image, prepare_mask,
        resize_to_original, restore_alpha,
    },
};

pub use self::processor::{Flux2ImageToImageOptions, Flux2InferenceOptions, Flux2InpaintOptions};

koharu_runtime::huggingface! {
    TRANSFORMER_WEIGHTS => "unsloth/FLUX.2-klein-4B-GGUF" => "flux-2-klein-4b-Q4_K_M.gguf",
    VAE_WEIGHTS => "black-forest-labs/FLUX.2-small-decoder" => "full_encoder_small_decoder.safetensors",
    TEXT_ENCODER_WEIGHTS => "unsloth/Qwen3-4B-GGUF" => "Qwen3-4B-Q4_K_M.gguf",
}

const DEFAULT_EDIT_PROMPT: &str =
    "Reconstruct the image while preserving its composition and visual style.";
const DEFAULT_INPAINT_PROMPT: &str = "Fill the masked area with the surrounding background, removing text and matching the original image.";

#[derive(Debug)]
pub struct Flux2Klein {
    model: Model,
}

impl Flux2Klein {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let (transformer, text_encoder, vae) = tokio::try_join!(
            huggingface::resolve(TRANSFORMER_WEIGHTS),
            huggingface::resolve(TEXT_ENCODER_WEIGHTS),
            huggingface::resolve(VAE_WEIGHTS),
        )
        .context("failed to resolve FLUX.2 Klein model assets")?;
        let model = Model::new(
            &device,
            ModelPaths {
                transformer,
                text_encoder,
                vae,
            },
        )?;
        Ok(Self { model })
    }

    /// Runs the Diffusers-aligned text-to-image or reference-image pipeline.
    ///
    /// Reference images use FLUX.2's editing tokens; pass an empty slice for
    /// text-to-image generation.
    pub fn inference(
        &self,
        prompt: &str,
        reference_images: &[DynamicImage],
        options: &Flux2InferenceOptions,
    ) -> Result<Vec<RgbImage>> {
        let mut params = generation_params(
            prompt,
            options.width,
            options.height,
            options.num_inference_steps,
            options.seed,
            options.batch_count,
        )?;
        params.reference_images = reference_images.iter().map(DynamicImage::to_rgb8).collect();
        self.model.forward(&params)
    }

    pub fn image_to_image(
        &self,
        image: &DynamicImage,
        options: &Flux2ImageToImageOptions,
    ) -> Result<DynamicImage> {
        self.image_to_image_with_reference(image, None, options)
    }

    pub fn image_to_image_with_reference(
        &self,
        image: &DynamicImage,
        reference_image: Option<&DynamicImage>,
        options: &Flux2ImageToImageOptions,
    ) -> Result<DynamicImage> {
        if options.strength <= 0.0 {
            return Ok(image.clone());
        }
        ensure!(
            options.strength <= 1.0,
            "FLUX.2 image-to-image strength must be in 0..=1"
        );
        let steps = i32::try_from(options.num_inference_steps)?;
        let (input, original_size) = prepare_image(image, options.max_pixels)?;
        let mut params = generation_params(
            DEFAULT_EDIT_PROMPT,
            input.width(),
            input.height(),
            steps,
            -1,
            1,
        )?;
        params.init_image = Some(input.clone());
        params.reference_images.push(input);
        if let Some(reference) = reference_image {
            params.reference_images.push(reference.to_rgb8());
        }
        params.strength = options.strength as f32;
        let output = self
            .model
            .forward(&params)?
            .into_iter()
            .next()
            .context("FLUX.2 Klein returned no image")?;
        let output = resize_to_original(output, original_size);
        Ok(restore_alpha(output, image))
    }

    pub fn inpaint(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        options: &Flux2InpaintOptions,
    ) -> Result<DynamicImage> {
        self.inpaint_with_reference(image, mask, None, options)
    }

    pub fn inpaint_with_reference(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        reference_image: Option<&DynamicImage>,
        options: &Flux2InpaintOptions,
    ) -> Result<DynamicImage> {
        ensure!(
            image.width() == mask.width() && image.height() == mask.height(),
            "image/mask dimensions differ: image={}x{}, mask={}x{}",
            image.width(),
            image.height(),
            mask.width(),
            mask.height()
        );
        if options.strength <= 0.0 {
            return Ok(image.clone());
        }
        ensure!(
            options.strength <= 1.0,
            "FLUX.2 inpaint strength must be in 0..=1"
        );
        let Some(bounds) = inpaint_crop_bounds(image, mask, options.mask_padding)? else {
            return Ok(image.clone());
        };

        let image_crop = image.crop_imm(bounds.x, bounds.y, bounds.width, bounds.height);
        let mask_crop = mask.crop_imm(bounds.x, bounds.y, bounds.width, bounds.height);
        let (input, crop_size) = prepare_image(&image_crop, options.max_pixels)?;
        let native_mask = prepare_mask(
            &mask_crop,
            input.width(),
            input.height(),
            options.mask_padding,
        );
        let steps = i32::try_from(options.num_inference_steps)?;
        let mut params = generation_params(
            DEFAULT_INPAINT_PROMPT,
            input.width(),
            input.height(),
            steps,
            -1,
            1,
        )?;
        params.init_image = Some(input.clone());
        params.reference_images.push(input);
        if let Some(reference) = reference_image {
            params.reference_images.push(reference.to_rgb8());
        }
        params.mask_image = Some(native_mask);
        params.strength = options.strength as f32;

        let generated = self
            .model
            .forward(&params)?
            .into_iter()
            .next()
            .context("FLUX.2 Klein returned no inpainted image")?;
        let generated = resize_to_original(generated, crop_size);
        composite_crop(image, generated, &mask_crop, bounds)
    }
}
