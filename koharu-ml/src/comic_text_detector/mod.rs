mod dbnet;
mod postprocess;
mod unet;
mod yolo_v5;

use std::{cmp, time::Instant};

use anyhow::Context;
use candle_core::{DType, Device, IndexOp, Tensor};
use image::{DynamicImage, GenericImageView, GrayImage, RgbImage, imageops};
use tracing::instrument;

use crate::{define_models, device, loading};

pub use postprocess::{
    ComicTextDetection, Quad, crop_text_block_bbox, extract_text_block_regions,
    refine_segmentation_mask,
};

const GPU_DETECT_SIZE: u32 = 1280;
const CPU_DETECT_SIZE: u32 = 640;
const DET_REARRANGE_MAX_BATCHES: usize = 4;
const DET_REARRANGE_DOWNSCALE_THRESHOLD: f32 = 2.5;
const DET_REARRANGE_ASPECT_THRESHOLD: f32 = 3.0;

struct StitchBuffers<'a> {
    shrink_sum: &'a mut [f32],
    threshold_sum: &'a mut [f32],
    mask_sum: &'a mut [f32],
    counts: &'a mut [f32],
}

struct PatchPlacement {
    width: u32,
    height: u32,
    offset_x: u32,
    top: u32,
    actual_height: u32,
}

define_models! {
    Yolov5 => ("mayocream/comic-text-detector", "yolo-v5.safetensors"),
    Unet => ("mayocream/comic-text-detector", "unet.safetensors"),
    DbNet => ("mayocream/comic-text-detector", "dbnet.safetensors"),
}

pub struct ComicTextDetector {
    yolo: yolo_v5::YoloV5,
    unet: unet::UNet,
    dbnet: Option<dbnet::DbNet>,
    device: Device,
}

impl ComicTextDetector {
    pub async fn load(cpu: bool) -> anyhow::Result<Self> {
        Self::load_inner(cpu, true).await
    }

    pub async fn load_segmentation_only(cpu: bool) -> anyhow::Result<Self> {
        Self::load_inner(cpu, false).await
    }

    async fn load_inner(cpu: bool, load_dbnet: bool) -> anyhow::Result<Self> {
        let device = device(cpu)?;
        let yolo = loading::load_mmaped_safetensors(Manifest::Yolov5.get(), &device, |vb| {
            yolo_v5::YoloV5::load(vb, 2, 3)
        })
        .await?;
        let unet =
            loading::load_mmaped_safetensors(Manifest::Unet.get(), &device, unet::UNet::load)
                .await?;
        let dbnet = if load_dbnet {
            Some(
                loading::load_mmaped_safetensors(
                    Manifest::DbNet.get(),
                    &device,
                    dbnet::DbNet::load,
                )
                .await?,
            )
        } else {
            None
        };

        Ok(Self {
            yolo,
            unet,
            dbnet,
            device,
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(&self, image: &DynamicImage) -> anyhow::Result<ComicTextDetection> {
        let detect_size = self.detect_size();
        let maps = if let Some(maps) =
            self.try_rearranged_maps(image, detect_size, DET_REARRANGE_MAX_BATCHES)?
        {
            maps
        } else {
            let original_dimensions = image.dimensions();
            let (image_tensor, resized_dimensions) = preprocess(image, &self.device, detect_size)?;
            let (mask, shrink_threshold) = self.forward(&image_tensor)?;
            postprocess_maps(
                &mask,
                &shrink_threshold,
                original_dimensions,
                resized_dimensions,
            )?
        };

        postprocess::build_detection(image, maps)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_segmentation(&self, image: &DynamicImage) -> anyhow::Result<GrayImage> {
        let started = Instant::now();
        let detect_size = self.detect_size();
        let (mask_map, rearranged) = if let Some(mask_map) =
            self.try_rearranged_mask_map(image, detect_size, DET_REARRANGE_MAX_BATCHES)?
        {
            (mask_map, true)
        } else {
            let original_dimensions = image.dimensions();
            let (image_tensor, resized_dimensions) = preprocess(image, &self.device, detect_size)?;
            let mask = self.forward_mask(&image_tensor)?;
            (
                postprocess_mask(&mask, original_dimensions, resized_dimensions)?,
                false,
            )
        };

        tracing::info!(
            width = image.width(),
            height = image.height(),
            rearranged,
            total_ms = started.elapsed().as_millis(),
            "comic text detector segmentation timings"
        );

        Ok(mask_map)
    }

    #[instrument(level = "debug", skip_all)]
    fn forward(&self, image: &Tensor) -> anyhow::Result<(Tensor, Tensor)> {
        let (mask, features) = self.forward_yolo_unet(image)?;
        let dbnet = self
            .dbnet
            .as_ref()
            .context("DBNet not loaded; use ComicTextDetector::load for full detection")?;
        let shrink_thresh = dbnet.forward(&features[0], &features[1], &features[2])?;

        Ok((mask, shrink_thresh))
    }

    #[instrument(level = "debug", skip_all)]
    fn forward_mask(&self, image: &Tensor) -> anyhow::Result<Tensor> {
        let (mask, _features) = self.forward_yolo_unet(image)?;
        Ok(mask)
    }

    #[instrument(level = "debug", skip_all)]
    fn forward_yolo_unet(&self, image: &Tensor) -> anyhow::Result<(Tensor, [Tensor; 3])> {
        let (_, features) = self.yolo.forward(image)?;
        let (mask, features) = self.unet.forward(
            &features[0],
            &features[1],
            &features[2],
            &features[3],
            &features[4],
        )?;
        Ok((mask, features))
    }

    fn detect_size(&self) -> u32 {
        match self.device {
            Device::Cpu => CPU_DETECT_SIZE,
            _ => GPU_DETECT_SIZE,
        }
    }

    fn try_rearranged_maps(
        &self,
        image: &DynamicImage,
        detect_size: u32,
        max_batch_size: usize,
    ) -> anyhow::Result<Option<postprocess::DetectionMaps>> {
        let Some(layout) = build_rearranged_layout(image, detect_size) else {
            return Ok(None);
        };
        let RearrangedLayout {
            transpose,
            width,
            height,
            pw_num,
            metadata,
            composites,
            composite_size,
        } = layout;

        let pixel_count = (width * height) as usize;
        let mut shrink_sum = vec![0.0f32; pixel_count];
        let mut threshold_sum = vec![0.0f32; pixel_count];
        let mut mask_sum = vec![0.0f32; pixel_count];
        let mut counts = vec![0.0f32; pixel_count];

        for batch_start in (0..composites.len()).step_by(max_batch_size.max(1)) {
            let batch_end = (batch_start + max_batch_size).min(composites.len());
            let mut tensors = Vec::with_capacity(batch_end - batch_start);
            for composite in &composites[batch_start..batch_end] {
                tensors.push(preprocess_rgb_image(composite, &self.device, detect_size)?);
            }
            let refs: Vec<&Tensor> = tensors.iter().collect();
            let batch = Tensor::cat(&refs, 0)?;
            let (mask_batch, shrink_threshold_batch) = self.forward(&batch)?;

            for batch_index in 0..(batch_end - batch_start) {
                let mask_map = tensor_channel_to_score_map_resized(
                    &mask_batch.i((batch_index, 0))?,
                    composite_size,
                    composite_size,
                )?;
                let shrink_map = tensor_channel_to_score_map_resized(
                    &shrink_threshold_batch.i((batch_index, 0))?,
                    composite_size,
                    composite_size,
                )?;
                let threshold_map = tensor_channel_to_score_map_resized(
                    &shrink_threshold_batch.i((batch_index, 1))?,
                    composite_size,
                    composite_size,
                )?;

                for slot in 0..pw_num as usize {
                    let patch_index = (batch_start + batch_index) * pw_num as usize + slot;
                    if patch_index >= metadata.len() {
                        break;
                    }
                    let (top, actual_height) = metadata[patch_index];
                    if actual_height == 0 {
                        continue;
                    }

                    let offset_x = slot as u32 * width;
                    stitch_patch(
                        StitchBuffers {
                            shrink_sum: &mut shrink_sum,
                            threshold_sum: &mut threshold_sum,
                            mask_sum: &mut mask_sum,
                            counts: &mut counts,
                        },
                        &shrink_map,
                        &threshold_map,
                        &mask_map,
                        PatchPlacement {
                            width,
                            height,
                            offset_x,
                            top,
                            actual_height,
                        },
                    );
                }
            }
        }

        let mut raw_shrink_map = accumulate_to_score_map(width, height, &shrink_sum, &counts);
        let mut raw_threshold_map = accumulate_to_score_map(width, height, &threshold_sum, &counts);
        let mut mask_map = accumulate_to_gray_image(width, height, &mask_sum, &counts)?;

        if transpose {
            raw_shrink_map = transpose_score_map(&raw_shrink_map);
            raw_threshold_map = transpose_score_map(&raw_threshold_map);
            mask_map = transpose_gray_image(&mask_map);
        }

        Ok(Some(postprocess::DetectionMaps {
            shrink_map: score_map_to_gray_image(&raw_shrink_map)?,
            threshold_map: score_map_to_gray_image(&raw_threshold_map)?,
            raw_shrink_map,
            raw_threshold_map,
            mask_map,
        }))
    }

    fn try_rearranged_mask_map(
        &self,
        image: &DynamicImage,
        detect_size: u32,
        max_batch_size: usize,
    ) -> anyhow::Result<Option<GrayImage>> {
        let Some(layout) = build_rearranged_layout(image, detect_size) else {
            return Ok(None);
        };
        let RearrangedLayout {
            transpose,
            width,
            height,
            pw_num,
            metadata,
            composites,
            composite_size,
        } = layout;

        let pixel_count = (width * height) as usize;
        let mut mask_sum = vec![0.0f32; pixel_count];
        let mut counts = vec![0.0f32; pixel_count];

        for batch_start in (0..composites.len()).step_by(max_batch_size.max(1)) {
            let batch_end = (batch_start + max_batch_size).min(composites.len());
            let mut tensors = Vec::with_capacity(batch_end - batch_start);
            for composite in &composites[batch_start..batch_end] {
                tensors.push(preprocess_rgb_image(composite, &self.device, detect_size)?);
            }
            let refs: Vec<&Tensor> = tensors.iter().collect();
            let batch = Tensor::cat(&refs, 0)?;
            let mask_batch = self.forward_mask(&batch)?;

            for batch_index in 0..(batch_end - batch_start) {
                let mask_map = tensor_channel_to_score_map_resized(
                    &mask_batch.i((batch_index, 0))?,
                    composite_size,
                    composite_size,
                )?;

                for slot in 0..pw_num as usize {
                    let patch_index = (batch_start + batch_index) * pw_num as usize + slot;
                    if patch_index >= metadata.len() {
                        break;
                    }
                    let (top, actual_height) = metadata[patch_index];
                    if actual_height == 0 {
                        continue;
                    }

                    let offset_x = slot as u32 * width;
                    stitch_mask_patch(
                        &mut mask_sum,
                        &mut counts,
                        &mask_map,
                        PatchPlacement {
                            width,
                            height,
                            offset_x,
                            top,
                            actual_height,
                        },
                    );
                }
            }
        }

        let mut mask_map = accumulate_to_gray_image(width, height, &mask_sum, &counts)?;
        if transpose {
            mask_map = transpose_gray_image(&mask_map);
        }
        Ok(Some(mask_map))
    }
}

struct RearrangedLayout {
    transpose: bool,
    width: u32,
    height: u32,
    pw_num: u32,
    metadata: Vec<(u32, u32)>,
    composites: Vec<RgbImage>,
    composite_size: u32,
}

fn build_rearranged_layout(image: &DynamicImage, detect_size: u32) -> Option<RearrangedLayout> {
    let mut working = image.to_rgb8();
    let mut transpose = false;
    let (mut height, mut width) = working.dimensions();
    if height < width {
        transpose = true;
        working = transpose_rgb_image(&working);
        (width, height) = working.dimensions();
    }

    let aspect_ratio = height as f32 / width as f32;
    let down_scale_ratio = height as f32 / detect_size as f32;
    let require_rearrange = down_scale_ratio > DET_REARRANGE_DOWNSCALE_THRESHOLD
        && aspect_ratio > DET_REARRANGE_ASPECT_THRESHOLD;
    if !require_rearrange {
        return None;
    }

    let pw_num = (((2 * detect_size) as f32 / width as f32).floor() as u32).max(2);
    let patch_height = pw_num * width;
    let patch_count = height.div_ceil(patch_height);
    let patch_step = if patch_count > 1 {
        (height - patch_height) / (patch_count - 1)
    } else {
        0
    };

    let mut patches = Vec::new();
    let mut metadata = Vec::new();
    for index in 0..patch_count {
        let top = index * patch_step;
        let bottom = (top + patch_height).min(height);
        let actual_height = bottom.saturating_sub(top);
        let crop = imageops::crop_imm(&working, 0, top, width, actual_height).to_image();
        let mut padded = RgbImage::from_pixel(width, patch_height, image::Rgb([0, 0, 0]));
        imageops::replace(&mut padded, &crop, 0, 0);
        patches.push(padded);
        metadata.push((top, actual_height));
    }

    let composites_per_batch = (patch_count as usize).div_ceil(pw_num as usize);
    let total_slots = composites_per_batch * pw_num as usize;
    while patches.len() < total_slots {
        patches.push(RgbImage::from_pixel(
            width,
            patch_height,
            image::Rgb([0, 0, 0]),
        ));
        metadata.push((0, 0));
    }

    let composite_size = patch_height;
    let mut composites = Vec::new();
    for chunk in patches.chunks(pw_num as usize) {
        let mut composite =
            RgbImage::from_pixel(composite_size, composite_size, image::Rgb([0, 0, 0]));
        for (slot, patch) in chunk.iter().enumerate() {
            imageops::replace(&mut composite, patch, (slot as u32 * width) as i64, 0);
        }
        composites.push(composite);
    }

    Some(RearrangedLayout {
        transpose,
        width,
        height,
        pw_num,
        metadata,
        composites,
        composite_size,
    })
}

#[instrument(level = "debug", skip_all)]
fn preprocess(
    image: &DynamicImage,
    device: &Device,
    image_size: u32,
) -> anyhow::Result<(Tensor, (u32, u32))> {
    let (orig_w, orig_h) = image.dimensions();
    let scale = (image_size as f32 / orig_w as f32).min(image_size as f32 / orig_h as f32);
    let width = ((orig_w as f32 * scale).round() as u32).clamp(1, image_size);
    let height = ((orig_h as f32 * scale).round() as u32).clamp(1, image_size);
    let (w, h) = (width as usize, height as usize);
    let tensor = (Tensor::from_vec(
        image.to_rgb8().into_raw(),
        (1, orig_h as usize, orig_w as usize, 3),
        device,
    )?
    .permute((0, 3, 1, 2))?
    .to_dtype(DType::F32)?
    .interpolate2d(h, w)?
    .pad_with_zeros(2, 0, image_size as usize - h)?
    .pad_with_zeros(3, 0, image_size as usize - w)?
        * (1. / 255.))?;

    Ok((tensor, (width, height)))
}

fn preprocess_rgb_image(
    image: &RgbImage,
    device: &Device,
    image_size: u32,
) -> anyhow::Result<Tensor> {
    let resized = if image.width() == image_size && image.height() == image_size {
        image.clone()
    } else {
        imageops::resize(
            image,
            image_size,
            image_size,
            imageops::FilterType::Triangle,
        )
    };

    Ok((Tensor::from_vec(
        resized.into_raw(),
        (1, image_size as usize, image_size as usize, 3),
        device,
    )?
    .permute((0, 3, 1, 2))?
    .to_dtype(DType::F32)?
        * (1. / 255.))?)
}

fn postprocess_maps(
    mask: &Tensor,
    shrink_thresh: &Tensor,
    original_dimensions: (u32, u32),
    resized_dimensions: (u32, u32),
) -> anyhow::Result<postprocess::DetectionMaps> {
    let shrink = shrink_thresh.i((0, 0))?;
    let threshold = shrink_thresh.i((0, 1))?;
    let unet_mask = mask.i((0, 0))?;

    let (db_h, db_w) = shrink.dims2()?;
    let (mask_h, mask_w) = unet_mask.dims2()?;
    let h = cmp::min(db_h, mask_h);
    let w = cmp::min(db_w, mask_w);
    let valid_h = h.min(resized_dimensions.1 as usize);
    let valid_w = w.min(resized_dimensions.0 as usize);

    let shrink = shrink.narrow(0, 0, valid_h)?.narrow(1, 0, valid_w)?;
    let threshold = threshold.narrow(0, 0, valid_h)?.narrow(1, 0, valid_w)?;
    let unet_mask = unet_mask.narrow(0, 0, valid_h)?.narrow(1, 0, valid_w)?;

    let raw_shrink_map = tensor_channel_to_score_map_exact(&shrink)?;
    let raw_threshold_map = tensor_channel_to_score_map_exact(&threshold)?;
    let shrink_map =
        tensor_channel_to_gray_resized(&shrink, original_dimensions.0, original_dimensions.1)?;
    let threshold_map =
        tensor_channel_to_gray_resized(&threshold, original_dimensions.0, original_dimensions.1)?;
    let mask_map =
        tensor_channel_to_gray_resized(&unet_mask, original_dimensions.0, original_dimensions.1)?;

    Ok(postprocess::DetectionMaps {
        raw_shrink_map,
        raw_threshold_map,
        shrink_map,
        threshold_map,
        mask_map,
    })
}

fn postprocess_mask(
    mask: &Tensor,
    original_dimensions: (u32, u32),
    resized_dimensions: (u32, u32),
) -> anyhow::Result<GrayImage> {
    let unet_mask = mask.i((0, 0))?;
    let (mask_h, mask_w) = unet_mask.dims2()?;
    let valid_h = mask_h.min(resized_dimensions.1 as usize);
    let valid_w = mask_w.min(resized_dimensions.0 as usize);
    let unet_mask = unet_mask.narrow(0, 0, valid_h)?.narrow(1, 0, valid_w)?;
    tensor_channel_to_gray_resized(&unet_mask, original_dimensions.0, original_dimensions.1)
}

fn tensor_channel_to_gray_resized(
    tensor: &Tensor,
    width: u32,
    height: u32,
) -> anyhow::Result<GrayImage> {
    score_map_to_gray_image(&tensor_channel_to_score_map_resized(tensor, width, height)?)
}

fn tensor_channel_to_score_map_exact(tensor: &Tensor) -> anyhow::Result<postprocess::ScoreMap> {
    let (height, width) = tensor.dims2()?;
    let values: Vec<f32> = tensor.flatten_all()?.to_vec1()?;
    Ok(postprocess::ScoreMap {
        width: width as u32,
        height: height as u32,
        values,
    })
}

fn tensor_channel_to_score_map_resized(
    tensor: &Tensor,
    width: u32,
    height: u32,
) -> anyhow::Result<postprocess::ScoreMap> {
    let resized = tensor
        .unsqueeze(0)?
        .unsqueeze(0)?
        .interpolate2d(height as usize, width as usize)?
        .squeeze(0)?
        .squeeze(0)?;
    let values: Vec<f32> = resized.flatten_all()?.to_vec1()?;
    Ok(postprocess::ScoreMap {
        width,
        height,
        values,
    })
}

fn stitch_patch(
    buffers: StitchBuffers<'_>,
    shrink_map: &postprocess::ScoreMap,
    threshold_map: &postprocess::ScoreMap,
    mask_map: &postprocess::ScoreMap,
    placement: PatchPlacement,
) {
    let PatchPlacement {
        width,
        height,
        offset_x,
        top,
        actual_height,
    } = placement;
    for y in 0..actual_height.min(height.saturating_sub(top)) {
        for x in 0..width {
            let global_index = ((top + y) * width + x) as usize;
            let source_x = offset_x + x;
            buffers.shrink_sum[global_index] += shrink_map.get(source_x, y);
            buffers.threshold_sum[global_index] += threshold_map.get(source_x, y);
            buffers.mask_sum[global_index] += mask_map.get(source_x, y);
            buffers.counts[global_index] += 1.0;
        }
    }
}

fn stitch_mask_patch(
    mask_sum: &mut [f32],
    counts: &mut [f32],
    mask_map: &postprocess::ScoreMap,
    placement: PatchPlacement,
) {
    let PatchPlacement {
        width,
        height,
        offset_x,
        top,
        actual_height,
    } = placement;
    for y in 0..actual_height.min(height.saturating_sub(top)) {
        for x in 0..width {
            let global_index = ((top + y) * width + x) as usize;
            let source_x = offset_x + x;
            mask_sum[global_index] += mask_map.get(source_x, y);
            counts[global_index] += 1.0;
        }
    }
}

pub async fn prefetch_segmentation() -> anyhow::Result<()> {
    Manifest::Yolov5.get().await?;
    Manifest::Unet.get().await?;
    Ok(())
}

fn accumulate_to_score_map(
    width: u32,
    height: u32,
    values: &[f32],
    counts: &[f32],
) -> postprocess::ScoreMap {
    let values = values
        .iter()
        .zip(counts.iter())
        .map(|(value, count)| {
            if *count <= 0.0 {
                0.0
            } else {
                (value / count).clamp(0.0, 1.0)
            }
        })
        .collect();
    postprocess::ScoreMap {
        width,
        height,
        values,
    }
}

fn accumulate_to_gray_image(
    width: u32,
    height: u32,
    values: &[f32],
    counts: &[f32],
) -> anyhow::Result<GrayImage> {
    score_map_to_gray_image(&accumulate_to_score_map(width, height, values, counts))
}

fn transpose_rgb_image(image: &RgbImage) -> RgbImage {
    RgbImage::from_fn(image.height(), image.width(), |x, y| *image.get_pixel(y, x))
}

fn transpose_gray_image(image: &GrayImage) -> GrayImage {
    GrayImage::from_fn(image.height(), image.width(), |x, y| *image.get_pixel(y, x))
}

fn transpose_score_map(score_map: &postprocess::ScoreMap) -> postprocess::ScoreMap {
    let mut values = vec![0.0f32; (score_map.width * score_map.height) as usize];
    for y in 0..score_map.height {
        for x in 0..score_map.width {
            values[(x * score_map.height + y) as usize] = score_map.get(x, y);
        }
    }
    postprocess::ScoreMap {
        width: score_map.height,
        height: score_map.width,
        values,
    }
}

fn score_map_to_gray_image(score_map: &postprocess::ScoreMap) -> anyhow::Result<GrayImage> {
    let bytes: Vec<u8> = score_map
        .values
        .iter()
        .copied()
        .map(float_to_byte)
        .collect();
    GrayImage::from_raw(score_map.width, score_map.height, bytes)
        .context("failed to build CTD map image")
}

fn float_to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    #[test]
    fn transpose_helpers_round_trip() {
        let image = GrayImage::from_fn(3, 5, |x, y| Luma([(x + y * 3) as u8]));
        let transposed = transpose_gray_image(&image);
        let round_trip = transpose_gray_image(&transposed);
        assert_eq!(round_trip, image);
    }
}
