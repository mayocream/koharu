use std::thread;

use candle_transformers::object_detection::{Bbox, non_maximum_suppression};
use hf_hub::api::sync::Api;
use image::GenericImageView;
use ort::session::Session;
use serde::Serialize;

#[derive(Debug)]
pub struct ComicTextDetector {
    model: Session,
}

#[derive(Debug, Serialize)]
pub struct Output {
    pub bboxes: Vec<ClassifiedBbox>,
    pub segment: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct ClassifiedBbox {
    pub xmin: f32,
    pub ymin: f32,
    pub xmax: f32,
    pub ymax: f32,
    pub confidence: f32,
    pub class: usize,
}

impl ComicTextDetector {
    pub fn new() -> anyhow::Result<Self> {
        let api = Api::new()?;
        let repo = api.model("mayocream/koharu".to_string());
        let model_path = repo.get("comictextdetector.onnx")?;

        let model = Session::builder()?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
            .with_intra_threads(thread::available_parallelism()?.get())?
            .commit_from_file(model_path)?;

        Ok(ComicTextDetector { model })
    }

    pub fn inference(
        &self,
        image: &image::DynamicImage,
        confidence_threshold: f32,
        nms_threshold: f32,
    ) -> anyhow::Result<Output> {
        let (orig_width, orig_height) = image.dimensions();
        let w_ratio = orig_width as f32 / 1024.0;
        let h_ratio = orig_height as f32 / 1024.0;
        let image = image.resize_exact(1024, 1024, image::imageops::FilterType::CatmullRom);

        let mut input = ndarray::Array::zeros((1, 3, 1024, 1024));
        for pixel in image.pixels() {
            let x = pixel.0 as usize;
            let y = pixel.1 as usize;
            let [r, g, b, _] = pixel.2.0;
            input[[0, 0, y, x]] = (r as f32) / 255.0;
            input[[0, 1, y, x]] = (g as f32) / 255.0;
            input[[0, 2, y, x]] = (b as f32) / 255.0;
        }

        let inputs = ort::inputs!["images" => input.view()]?;
        let outputs = self.model.run(inputs)?;

        // handle blocks
        let blk = outputs["blk"].try_extract_tensor::<f32>()?;
        let blk = blk.view();

        let mut boxes: Vec<Vec<Bbox<_>>> = (0..=1).map(|_| vec![]).collect();
        for i in 0..blk.shape()[1] {
            let confidence = blk[[0, i, 4]];
            if confidence < confidence_threshold {
                continue;
            }

            let mut class_index = 0;
            if blk[[0, i, 5]] < blk[[0, i, 6]] {
                class_index = 1;
            }

            let center_x = blk[[0, i, 0]] * w_ratio;
            let center_y = blk[[0, i, 1]] * h_ratio;
            let width = blk[[0, i, 2]] * w_ratio;
            let height = blk[[0, i, 3]] * h_ratio;

            boxes[class_index].push(Bbox {
                confidence,
                xmin: center_x - width / 2.,
                ymin: center_y - height / 2.,
                xmax: center_x + width / 2.,
                ymax: center_y + height / 2.,
                data: (),
            });
        }

        non_maximum_suppression(&mut boxes, nms_threshold);

        // Convert to output format
        let mut bboxes: Vec<ClassifiedBbox> = vec![];
        for (class_index, bboxes_for_class) in boxes.iter().enumerate() {
            for bbox in bboxes_for_class {
                bboxes.push(ClassifiedBbox {
                    xmin: bbox.xmin,
                    ymin: bbox.ymin,
                    xmax: bbox.xmax,
                    ymax: bbox.ymax,
                    confidence: bbox.confidence,
                    class: class_index,
                });
            }
        }

        // handle masks
        let mask = outputs["seg"].try_extract_tensor::<f32>()?;
        let mask = mask
            .view()
            .to_owned()
            .into_dimensionality::<ndarray::Ix4>()?;
        let mut segment = Vec::with_capacity(1024 * 1024);
        for i in 0..1024 {
            for j in 0..1024 {
                let val = (255.0 * mask[[0, 0, i, j]]).round() as u8;
                segment.push(val);
            }
        }
        // dilate the mask
        let segment = image::GrayImage::from_vec(1024, 1024, segment)
            .ok_or_else(|| anyhow::anyhow!("Failed to create GrayImage"))?;
        let segment = imageproc::morphology::grayscale_dilate(
            &segment,
            &imageproc::morphology::Mask::square(3),
        );
        let segment =
            imageproc::morphology::erode(&segment, imageproc::distance_transform::Norm::L2, 1);
        let segment = segment.into_raw();

        Ok(Output { bboxes, segment })
    }
}
