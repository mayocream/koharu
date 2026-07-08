use anyhow::{Result, bail};
use image::{DynamicImage, GenericImageView, imageops::FilterType};
use koharu_torch::{Device, IndexOp, Kind, Tensor};
use serde::{Deserialize, Serialize};

use super::model::PPDocLayoutV3ForwardOutput;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PPDocLayoutV3Processor {
    pub size: ProcessorSize,
    pub labels: Vec<String>,
}

impl Default for PPDocLayoutV3Processor {
    fn default() -> Self {
        Self {
            size: ProcessorSize {
                height: 800,
                width: 800,
            },
            labels: Vec::new(),
        }
    }
}

impl PPDocLayoutV3Processor {
    pub fn with_labels(mut self, labels: Vec<String>) -> Self {
        self.labels = labels;
        self
    }

    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json).unwrap_or_default())
    }

    pub fn preprocess(&self, image: &DynamicImage, device: Device) -> Tensor {
        let rgb = image.to_rgb8();
        let resized = image::imageops::resize(
            &rgb,
            self.size.width as u32,
            self.size.height as u32,
            FilterType::CatmullRom,
        );

        let mut pixels = Vec::with_capacity((self.size.height * self.size.width * 3) as usize);
        for pixel in resized.pixels() {
            pixels.push(pixel[0] as f32 / 255.0);
            pixels.push(pixel[1] as f32 / 255.0);
            pixels.push(pixel[2] as f32 / 255.0);
        }

        Tensor::from_slice(&pixels)
            .view([1, self.size.height, self.size.width, 3])
            .permute([0, 3, 1, 2])
            .to_device(device)
    }

    pub fn postprocess(
        &self,
        outputs: &PPDocLayoutV3ForwardOutput,
        image: &DynamicImage,
        threshold: f32,
    ) -> Result<PPDocLayoutV3Detections> {
        let (target_width, target_height) = image.dimensions();
        let logits = &outputs.logits;
        let pred_boxes = center_to_corners(&outputs.pred_boxes);
        let num_queries = logits.size()[1];
        let num_classes = logits.size()[2];

        let scale = Tensor::from_slice(&[
            target_width as f32,
            target_height as f32,
            target_width as f32,
            target_height as f32,
        ])
        .view([1, 1, 4])
        .to_device(pred_boxes.device());
        let pred_boxes = pred_boxes * scale;

        let scores_all = logits.sigmoid();
        let (scores, flat_index) = scores_all.flatten(1, -1).topk(num_queries, -1, true, true);
        let labels = flat_index.remainder(num_classes);
        let query_index = flat_index.floor_divide_scalar(num_classes);

        let boxes = pred_boxes.gather(
            1,
            &query_index
                .unsqueeze(-1)
                .repeat([1, 1, pred_boxes.size()[2]]),
            false,
        );

        let order_seq = get_order_seq(&outputs.order_logits).gather(1, &query_index, false);

        if scores.size()[0] != 1 {
            bail!("PP-DocLayout-V3 postprocess currently expects a batch size of 1");
        }

        let scores = tensor_to_vec_f32(&scores.i(0))?;
        let labels = tensor_to_vec_i64(&labels.i(0))?;
        let boxes = tensor_to_vec_f32(&boxes.i(0).contiguous().view([-1]))?;
        let orders = tensor_to_vec_i64(&order_seq.i(0))?;

        let mut regions = Vec::new();
        for query in 0..scores.len() {
            let score = scores[query];
            if score < threshold {
                continue;
            }
            let offset = query * 4;
            let bbox = [
                boxes[offset],
                boxes[offset + 1],
                boxes[offset + 2],
                boxes[offset + 3],
            ];
            let label_id = labels[query].max(0) as usize;
            let label = self
                .labels
                .get(label_id)
                .cloned()
                .unwrap_or_else(|| format!("LABEL_{label_id}"));
            regions.push(PPDocLayoutV3Region {
                order: orders[query].max(0) as usize,
                label_id,
                label,
                score,
                bbox,
                polygon_points: rect_polygon(bbox),
            });
        }

        regions.sort_by_key(|region| region.order);
        for (idx, region) in regions.iter_mut().enumerate() {
            region.order = idx + 1;
        }

        Ok(PPDocLayoutV3Detections { regions })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProcessorSize {
    pub height: i64,
    pub width: i64,
}

impl Default for ProcessorSize {
    fn default() -> Self {
        Self {
            height: 800,
            width: 800,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PPDocLayoutV3Detections {
    pub regions: Vec<PPDocLayoutV3Region>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PPDocLayoutV3Region {
    pub order: usize,
    pub label_id: usize,
    pub label: String,
    pub score: f32,
    pub bbox: [f32; 4],
    pub polygon_points: Vec<[f32; 2]>,
}

fn center_to_corners(boxes: &Tensor) -> Tensor {
    let centers = boxes.slice(-1, 0, 2, 1);
    let dims = boxes.slice(-1, 2, 4, 1);
    let top_left = &centers - &dims * 0.5;
    let bottom_right = centers + dims * 0.5;
    Tensor::cat(&[top_left, bottom_right], -1)
}

fn get_order_seq(order_logits: &Tensor) -> Tensor {
    let order_scores = order_logits.sigmoid();
    let size = order_scores.size();
    let batch_size = size[0];
    let sequence_length = size[1];

    let upper_votes = order_scores
        .triu(1)
        .sum_dim_intlist(&[1i64][..], false, None::<Kind>);
    let transposed_scores = order_scores.transpose(1, 2);
    let lower_votes = (transposed_scores.ones_like() - transposed_scores)
        .tril(-1)
        .sum_dim_intlist(&[1i64][..], false, None::<Kind>);
    let order_votes = upper_votes + lower_votes;
    let order_pointers = order_votes.argsort(1, false);
    let ranks = Tensor::arange(sequence_length, (Kind::Int64, order_logits.device()))
        .expand([batch_size, sequence_length], true);
    let mut order_seq = order_pointers.empty_like();
    order_seq.scatter_(1, &order_pointers, &ranks)
}

fn tensor_to_vec_f32(tensor: &Tensor) -> Result<Vec<f32>> {
    let tensor = tensor
        .to_device(Device::Cpu)
        .to_kind(Kind::Float)
        .contiguous();
    let mut values = vec![0f32; tensor.numel()];
    let len = values.len();
    tensor.f_copy_data(&mut values, len)?;
    Ok(values)
}

fn tensor_to_vec_i64(tensor: &Tensor) -> Result<Vec<i64>> {
    let tensor = tensor
        .to_device(Device::Cpu)
        .to_kind(Kind::Int64)
        .contiguous();
    let mut values = vec![0i64; tensor.numel()];
    let len = values.len();
    tensor.f_copy_data(&mut values, len)?;
    Ok(values)
}

fn rect_polygon(bbox: [f32; 4]) -> Vec<[f32; 2]> {
    vec![
        [bbox[0], bbox[1]],
        [bbox[2], bbox[1]],
        [bbox[2], bbox[3]],
        [bbox[0], bbox[3]],
    ]
}
