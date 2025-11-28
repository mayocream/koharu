mod dbnet;
mod unet;
mod yolo_v5;

use anyhow::bail;
use candle_core::IndexOp;
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::object_detection::{Bbox, non_maximum_suppression};
use image::DynamicImage;
use image::GenericImageView;
use image::GrayImage;
use image::imageops::FilterType;
use koharu_core::download::hf_hub;

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
        let image_t = preprocess(image, &self.device)?;
        let (predictions, mask, _shrink_thresh) = self.forward(&image_t)?;

        Ok((
            postprocess_yolo(&predictions, image.dimensions(), (640, 640), 0.3, 0.5)?,
            postprocess_mask(&mask, image.dimensions(), (640, 640))?,
        ))
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

fn preprocess(image: &DynamicImage, device: &Device) -> anyhow::Result<Tensor> {
    let image = image::imageops::resize(&image.to_rgb8(), 640, 640, FilterType::CatmullRom);
    let data = image.into_raw();
    let tensor = Tensor::from_vec(data, (1, 640, 640, 3), device)?
        .permute((0, 3, 1, 2))?
        .to_dtype(DType::F32)?;
    Ok((tensor * (1. / 255.))?)
}

fn postprocess_yolo(
    predictions: &Tensor,
    original_dimensions: (u32, u32),
    resized_dimensions: (u32, u32),
    confidence_threshold: f32,
    nms_threshold: f32,
) -> anyhow::Result<Vec<Bbox<usize>>> {
    let predictions = predictions.squeeze(0)?;
    let (num_boxes, num_outputs) = predictions.dims2()?;
    if num_outputs < 6 {
        bail!("invalid prediction shape: expected at least 6 outputs, got {num_outputs}");
    }
    let num_classes = num_outputs - 5;

    let width_scale = original_dimensions.0 as f32 / resized_dimensions.0 as f32;
    let height_scale = original_dimensions.1 as f32 / resized_dimensions.1 as f32;

    let mut bboxes: Vec<Vec<Bbox<usize>>> = (0..num_classes).map(|_| vec![]).collect();

    for index in 0..num_boxes {
        let pred = Vec::<f32>::try_from(predictions.i(index)?)?;
        if pred.len() < num_outputs {
            continue;
        }
        if !pred.iter().all(|v| v.is_finite()) {
            continue;
        }
        let objectness = pred[4];
        let (class_index, class_score) = pred[5..]
            .iter()
            .copied()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((0, 0.0));
        let confidence = objectness * class_score;
        if confidence < confidence_threshold || class_index >= num_classes {
            continue;
        }

        let cx = pred[0] * width_scale;
        let cy = pred[1] * height_scale;
        let w = pred[2] * width_scale;
        let h = pred[3] * height_scale;

        let mut xmin = cx - w / 2.0;
        let mut ymin = cy - h / 2.0;
        let mut xmax = cx + w / 2.0;
        let mut ymax = cy + h / 2.0;

        let (ow, oh) = (original_dimensions.0 as f32, original_dimensions.1 as f32);
        xmin = xmin.clamp(0.0, ow);
        xmax = xmax.clamp(0.0, ow);
        ymin = ymin.clamp(0.0, oh);
        ymax = ymax.clamp(0.0, oh);

        let width = xmax - xmin;
        let height = ymax - ymin;
        if width < 1.0 || height < 1.0 {
            continue;
        }

        let bbox = Bbox {
            xmin,
            ymin,
            xmax,
            ymax,
            confidence,
            data: class_index,
        };
        bboxes[class_index].push(bbox);
    }

    non_maximum_suppression(&mut bboxes, nms_threshold);

    let bboxes = bboxes.into_iter().flatten().collect();

    Ok(bboxes)
}

fn postprocess_mask(
    mask: &Tensor,
    original_dimensions: (u32, u32),
    resized_dimensions: (u32, u32),
) -> anyhow::Result<GrayImage> {
    let mask = mask.squeeze(0)?.squeeze(0)?;
    let mask: Vec<Vec<f32>> = mask.to_vec2()?;
    let data: Vec<u8> = mask
        .into_iter()
        .flatten()
        .map(|x| (x.clamp(0.0, 1.0) * 255.0) as u8)
        .collect();
    let image = GrayImage::from_raw(
        resized_dimensions.0 as u32,
        resized_dimensions.1 as u32,
        data,
    )
    .ok_or_else(|| anyhow::anyhow!("failed to build mask image"))?;

    Ok(image::imageops::resize(
        &image,
        original_dimensions.0,
        original_dimensions.1,
        image::imageops::FilterType::Lanczos3,
    ))
}
