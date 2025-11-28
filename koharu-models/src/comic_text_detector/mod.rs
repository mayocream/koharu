mod dbnet;
mod unet;
mod yolo_v5;

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
    let image = image.resize_to_fill(640, 640, FilterType::CatmullRom);
    let tensor =
        Tensor::from_vec(image.to_rgb8().into_raw(), (640, 640, 3), device)?.permute((2, 0, 1))?;
    let tensor = (tensor.unsqueeze(0)?.to_dtype(DType::F32)? * (1. / 255.))?;
    Ok(tensor)
}

fn postprocess_yolo(
    predictions: &Tensor,
    original_dimensions: (u32, u32),
    resized_dimensions: (u32, u32),
    confidence_threshold: f32,
    nms_threshold: f32,
) -> anyhow::Result<Vec<Bbox<usize>>> {
    let predictions = predictions.squeeze(0)?;
    let (num_outputs, num_boxes) = predictions.dims2()?;
    let num_classes = num_outputs - 5;

    let width_scale = original_dimensions.0 as f32 / resized_dimensions.0 as f32;
    let height_scale = original_dimensions.1 as f32 / resized_dimensions.1 as f32;

    let mut bboxes: Vec<Vec<Bbox<usize>>> = (0..num_classes).map(|_| vec![]).collect();

    for index in 0..num_boxes {
        let pred = Vec::<f32>::try_from(predictions.i((.., index))?)?;
        let confidence = *pred[4..].iter().max_by(|x, y| x.total_cmp(y)).unwrap();
        if confidence > confidence_threshold {
            let mut class_index = 0;
            for i in 0..num_classes {
                if pred[4 + i] > pred[4 + class_index] {
                    class_index = i;
                }
            }
            if pred[class_index + 4] > 0. {
                let xmin = (pred[0] - pred[2] / 2.) * width_scale;
                let ymin = (pred[1] - pred[3] / 2.) * height_scale;
                let xmax = (pred[0] + pred[2] / 2.) * width_scale;
                let ymax = (pred[1] + pred[3] / 2.) * height_scale;
                if xmax <= xmin || ymax <= ymin {
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
        }
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
