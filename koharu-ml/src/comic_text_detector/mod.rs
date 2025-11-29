mod dbnet;
mod unet;
mod yolo_v5;

use std::cmp;

use anyhow::bail;
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::object_detection::{Bbox, non_maximum_suppression};
use image::{
    DynamicImage, GenericImageView, GrayImage, ImageBuffer,
    imageops::{FilterType, replace, resize},
};
use imageproc::{
    contrast::{ThresholdType, threshold_mut},
    distance_transform::Norm,
    morphology::{close, dilate_mut},
};

use crate::hf_hub;

const IMAGE_SIZE: u32 = 1024;
const CONFIDENCE_THRESHOLD: f32 = 0.4;
const NMS_THRESHOLD: f32 = 0.35;
const DBNET_BINARIZE_K: f64 = 50.0;
const BINARY_THRESHOLD: u8 = 60;
const DILATION_RADIUS: u32 = 2;
const HOLE_CLOSE_RADIUS: u32 = 1;
const BBOX_DILATION: f32 = 1.0;

pub struct ComicTextDetector {
    yolo: yolo_v5::YoloV5,
    unet: unet::UNet,
    dbnet: dbnet::DbNet,
    device: Device,
}

impl ComicTextDetector {
    pub async fn load(device: Device) -> anyhow::Result<Self> {
        let yolo = {
            let weights = hf_hub("mayocream/comic-text-detector", "yolo-v5.safetensors").await?;
            let vb =
                unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &device)? };
            yolo_v5::YoloV5::load(vb, 2, 3)?
        };
        let unet = {
            let weights = hf_hub("mayocream/comic-text-detector", "unet.safetensors").await?;
            let vb =
                unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &device)? };
            unet::UNet::load(vb)?
        };
        let dbnet = {
            let weights = hf_hub("mayocream/comic-text-detector", "dbnet.safetensors").await?;
            let vb =
                unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &device)? };
            dbnet::DbNet::load(vb)?
        };

        Ok(Self {
            yolo,
            unet,
            dbnet,
            device,
        })
    }

    pub fn inference(&self, image: &DynamicImage) -> anyhow::Result<(Vec<Bbox<usize>>, GrayImage)> {
        let original_dimensions = image.dimensions();
        let (image_tensor, resized_dimensions) = preprocess(image, &self.device)?;
        let (predictions, mask, shrink_threshold) = self.forward(&image_tensor)?;
        let bboxes = postprocess_yolo(&predictions, original_dimensions, resized_dimensions)?;
        let mask = postprocess_mask(
            &mask,
            &shrink_threshold,
            original_dimensions,
            resized_dimensions,
        )?;

        Ok((bboxes, mask))
    }

    fn forward(&self, image: &Tensor) -> anyhow::Result<(Tensor, Tensor, Tensor)> {
        let (predictions, features) = self.yolo.forward(image)?;
        let (mask, features) = self.unet.forward(
            &features[0],
            &features[1],
            &features[2],
            &features[3],
            &features[4],
        )?;
        let shrink_thresh = self
            .dbnet
            .forward(&features[0], &features[1], &features[2])?;

        Ok((predictions, mask, shrink_thresh))
    }
}

fn preprocess(image: &DynamicImage, device: &Device) -> anyhow::Result<(Tensor, (u32, u32))> {
    let (initial_h, initial_w) = image.dimensions();
    // resize while keeping aspect ratio
    let (height, width) = if initial_h < initial_w {
        (IMAGE_SIZE * initial_h / initial_w, IMAGE_SIZE)
    } else {
        (IMAGE_SIZE, IMAGE_SIZE * initial_w / initial_h)
    };
    let resized = resize(&image.to_rgb8(), width, height, FilterType::Triangle);

    // pad to reserve aspect ratio
    let mut padded = ImageBuffer::new(IMAGE_SIZE, IMAGE_SIZE);
    replace(&mut padded, &resized, 0, 0);

    let data = padded.to_vec();
    let tensor = (Tensor::from_vec(
        data,
        (1, IMAGE_SIZE as usize, IMAGE_SIZE as usize, 3),
        device,
    )?
    // NHWC to NCHW
    .permute((0, 3, 1, 2))?
    // Normalize to [0, 1]
    .to_dtype(DType::F32)?
        * (1. / 255.))?;

    Ok((tensor, (width, height)))
}

fn postprocess_yolo(
    predictions: &Tensor,
    original_dimensions: (u32, u32),
    resized_dimensions: (u32, u32),
) -> anyhow::Result<Vec<Bbox<usize>>> {
    // predictions shape: (1, num_boxes, num_outputs)
    // this removes the batch dimension
    let predictions = predictions.squeeze(0)?;
    let (_, num_outputs) = predictions.dims2()?;
    if num_outputs < 6 {
        bail!("invalid prediction shape: expected at least 6 outputs, got {num_outputs}");
    }
    // YOLOv5 format: [cx, cy, w, h, objectness, class1_score, class2_score, ...]
    let num_classes = num_outputs - 5;

    let (orig_w, orig_h) = original_dimensions;
    let (resized_w, resized_h) = resized_dimensions;
    let w_ratio = orig_w as f32 / resized_w as f32;
    let h_ratio = orig_h as f32 / resized_h as f32;

    let mut bboxes: Vec<Vec<Bbox<usize>>> = (0..num_classes).map(|_| Vec::new()).collect();
    let predictions: Vec<Vec<f32>> = predictions.to_vec2()?;
    for pred in predictions {
        let (class_index, confidence) = {
            let (cls_idx, cls_score) = pred[5..]
                .iter()
                .copied()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap_or((0, 0.0));
            (cls_idx, pred[4] * cls_score)
        };
        if confidence < CONFIDENCE_THRESHOLD {
            continue;
        }

        let xmin = ((pred[0] - pred[2] / 2.) * w_ratio - BBOX_DILATION).clamp(0., orig_w as f32);
        let xmax = ((pred[0] + pred[2] / 2.) * w_ratio + BBOX_DILATION).clamp(0., orig_w as f32);
        let ymin = ((pred[1] - pred[3] / 2.) * h_ratio - BBOX_DILATION).clamp(0., orig_h as f32);
        let ymax = ((pred[1] + pred[3] / 2.) * h_ratio + BBOX_DILATION).clamp(0., orig_h as f32);

        let bbox = Bbox {
            xmin,
            xmax,
            ymin,
            ymax,
            confidence,
            data: class_index,
        };
        bboxes[class_index].push(bbox);
    }

    non_maximum_suppression(&mut bboxes, NMS_THRESHOLD);

    Ok(bboxes.into_iter().flatten().collect())
}

fn postprocess_mask(
    mask: &Tensor,
    shrink_thresh: &Tensor,
    original_dimensions: (u32, u32),
    resized_dimensions: (u32, u32),
) -> anyhow::Result<GrayImage> {
    let shrink_and_thresh = shrink_thresh.squeeze(0)?; // (2, H, W)
    let shrink = shrink_and_thresh.i(0)?; // (H, W)
    let thresh = shrink_and_thresh.i(1)?; // (H, W)
    let unet_mask = mask.squeeze(0)?; // (1, H, W)

    let (_, h_db, w_db) = shrink_and_thresh.dims3()?;
    let (_, h_unet, w_unet) = unet_mask.dims3()?;
    let h = cmp::min(h_db, h_unet);
    let w = cmp::min(w_db, w_unet);

    let shrink = shrink.narrow(0, 0, h)?.narrow(1, 0, w)?;
    let thresh = thresh.narrow(0, 0, h)?.narrow(1, 0, w)?;
    let unet_mask = unet_mask.narrow(1, 0, h)?.narrow(2, 0, w)?.squeeze(0)?;

    // Differentiable binarization: prob = sigmoid(k * (shrink - threshold)).
    let prob = candle_nn::ops::sigmoid(&((&shrink - &thresh)? * DBNET_BINARIZE_K)?)?;
    // Keep the thickest response between UNet mask and DBNet prob.
    let fused = prob.maximum(&unet_mask)?;

    let (mask_h, mask_w) = fused.dims2()?;
    let valid_h = mask_h.min(resized_dimensions.1 as usize);
    let valid_w = mask_w.min(resized_dimensions.0 as usize);

    let mask: Vec<Vec<f32>> = fused
        .narrow(0, 0, valid_h)?
        .narrow(1, 0, valid_w)?
        .to_vec2()?;
    let data: Vec<u8> = mask
        .into_iter()
        .flatten()
        .map(|x| (x.clamp(0.0, 1.0) * 255.0) as u8)
        .collect();
    let image = GrayImage::from_raw(valid_w as u32, valid_h as u32, data)
        .ok_or_else(|| anyhow::anyhow!("failed to build mask image"))?;

    let mut mask = resize(
        &image,
        original_dimensions.0,
        original_dimensions.1,
        FilterType::Nearest,
    );

    // Binarize aggressively to avoid thin edges from interpolation.
    threshold_mut(&mut mask, BINARY_THRESHOLD, ThresholdType::Binary);

    // Close small holes and smooth stroke interiors.
    mask = close(&mask, Norm::LInf, HOLE_CLOSE_RADIUS as u8);

    // Light dilation to regain thickness.
    dilate_mut(&mut mask, Norm::L1, DILATION_RADIUS as u8);

    Ok(mask)
}
