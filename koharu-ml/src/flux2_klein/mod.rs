mod latents;
mod precomputed;
pub mod qwen;
mod scheduler;
mod transformer;
mod vae;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use candle_core::{DType, Device, Tensor};
use image::{DynamicImage, GenericImageView, RgbImage};
use koharu_runtime::RuntimeManager;
use tracing::instrument;

use crate::{device, inpainting, loading};

use self::{
    latents::{
        IMAGE_MULTIPLE, expand_mask, image_to_tensor, mask_to_packed_tensor, pack_latents,
        prepare_latent_ids, prepare_mask, prepare_rgb_image, resize_back_if_needed,
        tensor_to_rgb_image, unpack_latents,
    },
    precomputed::Flux2PromptEmbedder,
    scheduler::FlowMatchScheduler,
    transformer::Flux2Transformer,
    vae::Flux2Vae,
};

const FLUX2_REPO: &str = "unsloth/FLUX.2-klein-4B-GGUF";
const FLUX2_GGUF: &str = "flux-2-klein-4b-Q4_K_M.gguf";
const VAE_REPO: &str = "black-forest-labs/FLUX.2-small-decoder";
const VAE_FILE: &str = "diffusion_pytorch_model.safetensors";
const INPAINT_CROP_CONTEXT: u32 = 64;

#[derive(Debug, Clone, Copy)]
struct CropBounds {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

koharu_runtime::declare_hf_model_package!(
    id: "model:flux2-klein-4b:transformer-q4-k-m",
    repo: FLUX2_REPO,
    file: FLUX2_GGUF,
    bootstrap: false,
    order: 140,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:flux2-klein-4b:small-decoder",
    repo: VAE_REPO,
    file: VAE_FILE,
    bootstrap: false,
    order: 143,
);

#[derive(Debug, Clone)]
pub struct Flux2KleinPaths {
    pub transformer_gguf: PathBuf,
    pub vae_safetensors: PathBuf,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

pub struct Flux2Klein {
    transformer: Flux2Transformer,
    prompt_embedder: Flux2PromptEmbedder,
    vae: Flux2Vae,
    device: Device,
}

impl Flux2Klein {
    pub async fn load(runtime: &RuntimeManager) -> Result<Self> {
        let paths = Flux2KleinPaths {
            transformer_gguf: runtime
                .downloads()
                .huggingface_model(FLUX2_REPO, FLUX2_GGUF)
                .await?,
            vae_safetensors: runtime
                .downloads()
                .huggingface_model(VAE_REPO, VAE_FILE)
                .await?,
        };
        Self::load_from_paths(paths)
    }

    pub fn load_from_paths(paths: Flux2KleinPaths) -> Result<Self> {
        validate_path(&paths.transformer_gguf, "Flux2 transformer GGUF")?;
        validate_path(&paths.vae_safetensors, "Flux2 VAE")?;

        let model_device = device(false)?;

        let transformer = Flux2Transformer::from_gguf(&paths.transformer_gguf, &model_device)
            .with_context(|| {
                format!(
                    "failed to load Flux2 transformer from {}",
                    paths.transformer_gguf.display()
                )
            })?;
        if transformer.in_channels() != 128 {
            bail!(
                "unsupported Flux2 input channel count {}, expected 128",
                transformer.in_channels()
            );
        }
        if precomputed::PROMPT_EMBED_DIM != transformer.context_in_dim() {
            bail!(
                "embedded Flux2 prompt has {} channels, expected {} for this transformer",
                precomputed::PROMPT_EMBED_DIM,
                transformer.context_in_dim()
            );
        }
        let prompt_embedder = Flux2PromptEmbedder::new(&model_device);
        let vae =
            loading::load_mmaped_safetensors_path(&paths.vae_safetensors, &model_device, |vb| {
                Flux2Vae::new(vb)
            })
            .with_context(|| {
                format!(
                    "failed to load Flux2 VAE from {}",
                    paths.vae_safetensors.display()
                )
            })?;

        Ok(Self {
            transformer,
            prompt_embedder,
            vae,
            device: model_device,
        })
    }

    pub fn precompute_prompt_embeddings(&self) -> Result<()> {
        self.prompt_embedder.encode_prompt()?;
        Ok(())
    }

    #[instrument(level = "debug", skip_all)]
    pub fn image_to_image(
        &self,
        image: &DynamicImage,
        options: &Flux2ImageToImageOptions,
    ) -> Result<DynamicImage> {
        self.image_to_image_with_reference(image, None, options)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn image_to_image_with_reference(
        &self,
        image: &DynamicImage,
        reference_image: Option<&DynamicImage>,
        options: &Flux2ImageToImageOptions,
    ) -> Result<DynamicImage> {
        if options.strength <= 0.0 {
            return Ok(image.clone());
        }

        let (latents, packed_h, packed_w, size) = {
            let (rgb, size) = prepare_rgb_image(image, options.max_pixels);
            let image_latents = self.encode_image_latents(&rgb)?;
            let (batch, channels, packed_h, packed_w) = image_latents.dims4()?;
            if batch != 1 || channels != 128 {
                bail!("unexpected Flux2 latent shape {:?}", image_latents.shape());
            }

            let prompt_embeddings = self.prompt_embedder.encode_prompt()?;
            let transformer_dtype = transformer_dtype(&self.device);
            let prompt_embeds = prompt_embeddings
                .prompt_embeds
                .to_device(&self.device)?
                .to_dtype(transformer_dtype)?;
            let text_ids = prompt_embeddings.text_ids.to_device(&self.device)?;

            let image_latents_packed = pack_latents(&image_latents)?;
            let latent_ids = prepare_latent_ids(1, packed_h, packed_w, 0, &self.device)?;
            let mut condition_latents = image_latents_packed.clone();
            let mut condition_ids = prepare_latent_ids(1, packed_h, packed_w, 10, &self.device)?;
            if let Some(reference_image) = reference_image {
                let reference_latents =
                    self.encode_reference_latents(reference_image, options.max_pixels)?;
                let (_, _, ref_h, ref_w) = reference_latents.dims4()?;
                condition_latents =
                    Tensor::cat(&[condition_latents, pack_latents(&reference_latents)?], 1)?;
                condition_ids = Tensor::cat(
                    &[
                        condition_ids,
                        prepare_latent_ids(1, ref_h, ref_w, 20, &self.device)?,
                    ],
                    1,
                )?;
            }
            let condition_latents = condition_latents.to_dtype(transformer_dtype)?;

            let mut scheduler =
                FlowMatchScheduler::new(options.num_inference_steps, packed_h * packed_w);
            let timesteps = scheduler.timesteps().to_vec();
            let start_index = start_index_for_strength(timesteps.len(), options.strength);
            scheduler.set_step_index(start_index);
            let noise = Tensor::randn(0f32, 1f32, image_latents.shape(), &self.device)?;
            let initial_timestep = timesteps[start_index];
            let mut latents =
                pack_latents(&scheduler.scale_noise(&image_latents, initial_timestep, &noise)?)?;

            for step_idx in start_index..timesteps.len() {
                let timestep = Tensor::from_vec(
                    vec![scheduler.timestep_for_model(step_idx) as f32],
                    (1,),
                    &self.device,
                )?;
                let latent_model_input = Tensor::cat(
                    &[
                        latents.to_dtype(transformer_dtype)?,
                        condition_latents.clone(),
                    ],
                    1,
                )?;
                let img_ids = Tensor::cat(&[latent_ids.clone(), condition_ids.clone()], 1)?;
                let noise_pred = self.transformer.forward(
                    &latent_model_input,
                    &img_ids,
                    &prompt_embeds,
                    &text_ids,
                    &timestep,
                )?;
                let noise_pred = noise_pred
                    .narrow(1, 0, latents.dim(1)?)?
                    .to_dtype(DType::F32)?;
                latents = scheduler.step(&noise_pred, &latents)?;
            }

            (latents, packed_h, packed_w, size)
        };

        let rgb = self.decode_packed_latents(latents, packed_h, packed_w)?;
        let mut output = resize_back_if_needed(rgb, size);
        if image.color().has_alpha() {
            output = restore_original_alpha(output, image);
        }
        Ok(output)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inpaint(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        options: &Flux2InpaintOptions,
    ) -> Result<DynamicImage> {
        self.inpaint_with_reference(image, mask, None, options)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inpaint_with_reference(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        reference_image: Option<&DynamicImage>,
        options: &Flux2InpaintOptions,
    ) -> Result<DynamicImage> {
        if image.dimensions() != mask.dimensions() {
            bail!(
                "image/mask dimensions mismatch: image is {:?}, mask is {:?}",
                image.dimensions(),
                mask.dimensions()
            );
        }
        if options.strength <= 0.0 {
            return Ok(image.clone());
        }

        if let Some(bounds) = inpaint_crop_bounds(image, mask, options.mask_padding) {
            let image_crop = image.crop_imm(bounds.x, bounds.y, bounds.width, bounds.height);
            let mask_crop = mask.crop_imm(bounds.x, bounds.y, bounds.width, bounds.height);
            let generated =
                self.inpaint_full_frame(&image_crop, &mask_crop, reference_image, options)?;
            return composite_inpaint_crop(image, &generated, &mask_crop, bounds);
        }

        self.inpaint_full_frame(image, mask, reference_image, options)
    }

    fn inpaint_full_frame(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        reference_image: Option<&DynamicImage>,
        options: &Flux2InpaintOptions,
    ) -> Result<DynamicImage> {
        let (latents, packed_h, packed_w, size) = {
            let (rgb, size) = prepare_rgb_image(image, options.max_pixels);
            let resized_mask = expand_mask(
                &prepare_mask(mask, size.width, size.height),
                options.mask_padding,
            );
            let image_latents = self.encode_image_latents(&rgb)?;
            let (batch, channels, packed_h, packed_w) = image_latents.dims4()?;
            if batch != 1 || channels != 128 {
                bail!("unexpected Flux2 latent shape {:?}", image_latents.shape());
            }
            let prompt_embeddings = self.prompt_embedder.encode_prompt()?;
            let transformer_dtype = transformer_dtype(&self.device);
            let prompt_embeds = prompt_embeddings
                .prompt_embeds
                .to_device(&self.device)?
                .to_dtype(transformer_dtype)?;
            let text_ids = prompt_embeddings.text_ids.to_device(&self.device)?;
            let latent_mask =
                mask_to_packed_tensor(&resized_mask, packed_h, packed_w, &self.device)?;

            let image_latents_packed = pack_latents(&image_latents)?;
            let latent_ids = prepare_latent_ids(1, packed_h, packed_w, 0, &self.device)?;
            let mut condition_latents = image_latents_packed.clone();
            let mut condition_ids = prepare_latent_ids(1, packed_h, packed_w, 10, &self.device)?;
            if let Some(reference_image) = reference_image {
                let reference_latents =
                    self.encode_reference_latents(reference_image, options.max_pixels)?;
                let (_, _, ref_h, ref_w) = reference_latents.dims4()?;
                condition_latents =
                    Tensor::cat(&[condition_latents, pack_latents(&reference_latents)?], 1)?;
                condition_ids = Tensor::cat(
                    &[
                        condition_ids,
                        prepare_latent_ids(1, ref_h, ref_w, 20, &self.device)?,
                    ],
                    1,
                )?;
            }
            let condition_latents = condition_latents.to_dtype(transformer_dtype)?;

            let mut scheduler =
                FlowMatchScheduler::new(options.num_inference_steps, packed_h * packed_w);
            let timesteps = scheduler.timesteps().to_vec();
            let start_index = start_index_for_strength(timesteps.len(), options.strength);
            scheduler.set_step_index(start_index);
            let noise = Tensor::randn(0f32, 1f32, image_latents.shape(), &self.device)?;
            let noise_packed = pack_latents(&noise)?;
            let initial_timestep = timesteps[start_index];
            let mut latents =
                pack_latents(&scheduler.scale_noise(&image_latents, initial_timestep, &noise)?)?;

            for step_idx in start_index..timesteps.len() {
                let timestep = Tensor::from_vec(
                    vec![scheduler.timestep_for_model(step_idx) as f32],
                    (1,),
                    &self.device,
                )?;
                let latent_model_input = Tensor::cat(
                    &[
                        latents.to_dtype(transformer_dtype)?,
                        condition_latents.clone(),
                    ],
                    1,
                )?;
                let img_ids = Tensor::cat(&[latent_ids.clone(), condition_ids.clone()], 1)?;
                let noise_pred = self.transformer.forward(
                    &latent_model_input,
                    &img_ids,
                    &prompt_embeds,
                    &text_ids,
                    &timestep,
                )?;
                let noise_pred = noise_pred
                    .narrow(1, 0, latents.dim(1)?)?
                    .to_dtype(DType::F32)?;
                latents = scheduler.step(&noise_pred, &latents)?;

                let init_latents = if step_idx + 1 < timesteps.len() {
                    scheduler.scale_noise(
                        &image_latents_packed,
                        timesteps[step_idx + 1],
                        &noise_packed,
                    )?
                } else {
                    image_latents_packed.clone()
                };
                let keep_mask = ((&latent_mask * -1.0)? + 1.0)?;
                latents = (keep_mask.broadcast_mul(&init_latents)?
                    + latent_mask.broadcast_mul(&latents)?)?;
            }

            (latents, packed_h, packed_w, size)
        };

        let rgb = self.decode_packed_latents(latents, packed_h, packed_w)?;
        let mut output = resize_back_if_needed(rgb, size);
        if image.color().has_alpha() {
            let original_alpha = inpainting::extract_alpha(&image.to_rgba8());
            let binary_mask = inpainting::binarize_mask(mask);
            let restored =
                inpainting::restore_alpha_channel(&output.to_rgb8(), &original_alpha, &binary_mask);
            output = DynamicImage::ImageRgba8(restored);
        }
        Ok(output)
    }

    fn encode_reference_latents(&self, image: &DynamicImage, max_pixels: u32) -> Result<Tensor> {
        let (rgb, _) = prepare_rgb_image(image, max_pixels);
        self.encode_image_latents(&rgb)
    }

    fn encode_image_latents(&self, image: &RgbImage) -> Result<Tensor> {
        let image_tensor = image_to_tensor(image, &self.device)?;
        let latents = self.vae.encode_patchified_normalized(&image_tensor)?;
        drop(image_tensor);
        Ok(latents.to_dtype(DType::F32)?)
    }

    fn decode_packed_latents(
        &self,
        packed_latents: Tensor,
        packed_h: usize,
        packed_w: usize,
    ) -> Result<RgbImage> {
        let patchified = unpack_latents(&packed_latents, packed_h, packed_w)?;
        drop(packed_latents);
        let decoded = self.vae.decode_patchified_normalized(&patchified)?;
        drop(patchified);
        let rgb = tensor_to_rgb_image(&decoded)?;
        drop(decoded);
        Ok(rgb)
    }
}

fn transformer_dtype(_device: &Device) -> DType {
    DType::F32
}

fn inpaint_crop_bounds(
    image: &DynamicImage,
    mask: &DynamicImage,
    mask_padding: u8,
) -> Option<CropBounds> {
    let gray = mask.to_luma8();
    let mut min_x = gray.width();
    let mut min_y = gray.height();
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;
    for (x, y, pixel) in gray.enumerate_pixels() {
        if pixel.0[0] == 0 {
            continue;
        }
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        found = true;
    }
    if !found {
        return None;
    }

    let padding = INPAINT_CROP_CONTEXT.max(mask_padding as u32);
    let multiple = IMAGE_MULTIPLE;
    let width = image.width();
    let height = image.height();
    let mut x0 = min_x.saturating_sub(padding);
    let mut y0 = min_y.saturating_sub(padding);
    let mut x1 = (max_x + 1 + padding).min(width);
    let mut y1 = (max_y + 1 + padding).min(height);

    x0 = (x0 / multiple) * multiple;
    y0 = (y0 / multiple) * multiple;
    x1 = x1.div_ceil(multiple) * multiple;
    y1 = y1.div_ceil(multiple) * multiple;
    x1 = x1.min(width);
    y1 = y1.min(height);

    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    if x0 == 0 && y0 == 0 && x1 == width && y1 == height {
        return None;
    }

    Some(CropBounds {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

fn composite_inpaint_crop(
    original: &DynamicImage,
    generated_crop: &DynamicImage,
    mask_crop: &DynamicImage,
    bounds: CropBounds,
) -> Result<DynamicImage> {
    if generated_crop.dimensions() != (bounds.width, bounds.height) {
        bail!(
            "generated crop dimensions mismatch: got {:?}, expected {}x{}",
            generated_crop.dimensions(),
            bounds.width,
            bounds.height
        );
    }

    let mut output = original.to_rgba8();
    let generated = generated_crop.to_rgba8();
    let mask = mask_crop.to_luma8();
    for y in 0..bounds.height {
        for x in 0..bounds.width {
            let alpha = mask.get_pixel(x, y).0[0] as f32 / 255.0;
            if alpha <= 0.0 {
                continue;
            }
            let generated_pixel = generated.get_pixel(x, y).0;
            let output_pixel = output.get_pixel_mut(bounds.x + x, bounds.y + y);
            for (channel, generated_channel) in generated_pixel.iter().enumerate().take(3) {
                output_pixel.0[channel] = (output_pixel.0[channel] as f32 * (1.0 - alpha)
                    + *generated_channel as f32 * alpha)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }

    if original.color().has_alpha() {
        Ok(DynamicImage::ImageRgba8(output))
    } else {
        Ok(DynamicImage::ImageRgb8(
            DynamicImage::ImageRgba8(output).to_rgb8(),
        ))
    }
}

fn start_index_for_strength(num_steps: usize, strength: f64) -> usize {
    let strength = strength.clamp(0.0, 1.0);
    let init_timestep = ((num_steps as f64) * strength).round() as usize;
    num_steps.saturating_sub(init_timestep.max(1))
}

fn validate_path(path: &Path, label: &str) -> Result<()> {
    if !path.exists() {
        bail!("{label} path does not exist: {}", path.display());
    }
    Ok(())
}

fn restore_original_alpha(output: DynamicImage, original: &DynamicImage) -> DynamicImage {
    let mut rgba = output.to_rgba8();
    let alpha = inpainting::extract_alpha(&original.to_rgba8());
    for (x, y, pixel) in rgba.enumerate_pixels_mut() {
        pixel.0[3] = alpha.get_pixel(x, y).0[0];
    }
    DynamicImage::ImageRgba8(rgba)
}
