mod model;

use std::cmp::Ordering;

use anyhow::{Context, Result, bail};
use burn::{
    store::{ModuleSnapshot, PyTorchToBurnAdapter, SafetensorsStore},
    tensor::{DType, Device, DeviceKind, FloatDType, Tensor, TensorData},
};
use image::{DynamicImage, imageops::FilterType};
use koharu_runtime::RuntimeManager;
use serde::Serialize;

use self::model::{
    IMAGE_SIZE, NUM_LABELS, NUM_QUERIES, PPDocLayoutV3ForObjectDetection, cast_module_float,
    tensor_to_f32_vec,
};

const HF_REPO: &str = "PaddlePaddle/PP-DocLayoutV3_safetensors";

koharu_runtime::declare_hf_model_package!(
    id: "model:pp-doclayout-v3:weights",
    repo: "PaddlePaddle/PP-DocLayoutV3_safetensors",
    file: "model.safetensors",
    bootstrap: false,
    order: 260
);

const ID2LABEL: [&str; NUM_LABELS] = [
    "abstract",
    "algorithm",
    "aside_text",
    "chart",
    "content",
    "formula",
    "doc_title",
    "figure_title",
    "footer",
    "footer",
    "footnote",
    "formula_number",
    "header",
    "header",
    "image",
    "formula",
    "number",
    "paragraph_title",
    "reference",
    "reference_content",
    "seal",
    "table",
    "text",
    "text",
    "vision_footnote",
];

#[derive(Debug, Clone, Serialize)]
pub struct PPDocLayoutV3Result {
    pub width: u32,
    pub height: u32,
    pub regions: Vec<PPDocLayoutV3Region>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PPDocLayoutV3Region {
    pub label: String,
    pub label_id: usize,
    pub score: f32,
    pub bbox: [f32; 4],
    pub polygon_points: Vec<[f32; 2]>,
    pub order: usize,
}

pub struct PPDocLayoutV3 {
    model: PPDocLayoutV3ForObjectDetection,
    device: Device,
    dtype: DType,
}

impl PPDocLayoutV3 {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let model_path = runtime
            .downloads()
            .huggingface_model(HF_REPO, "model.safetensors")
            .await
            .context("failed to resolve PP-DocLayoutV3 model.safetensors")?;

        let (device, cuda_bf16) = make_device(cpu);
        let mut model = PPDocLayoutV3ForObjectDetection::new(&device);
        let mut store = SafetensorsStore::from_file(model_path)
            .with_from_adapter(PyTorchToBurnAdapter)
            .with_key_remapping(
                r"^model\.encoder_input_proj\.(\d+)\.0\.",
                "model.encoder_input_proj.$1.conv.",
            )
            .with_key_remapping(
                r"^model\.encoder_input_proj\.(\d+)\.1\.",
                "model.encoder_input_proj.$1.norm.",
            )
            .with_key_remapping(
                r"^model\.decoder_input_proj\.(\d+)\.0\.",
                "model.decoder_input_proj.$1.conv.",
            )
            .with_key_remapping(
                r"^model\.decoder_input_proj\.(\d+)\.1\.",
                "model.decoder_input_proj.$1.norm.",
            )
            .with_key_remapping(r"^model\.enc_output\.0\.", "model.enc_output.linear.")
            .with_key_remapping(r"^model\.enc_output\.1\.", "model.enc_output.norm.")
            .with_key_remapping(
                r"^model\.encoder\.mask_feature_head\.scale_heads\.2\.layers\.2\.",
                "model.encoder.mask_feature_head.scale_heads.2.layers.1.",
            )
            .skip_enum_variants(true)
            .allow_partial(false);

        let result = model
            .load_from(&mut store)
            .context("failed to mmap/load PP-DocLayoutV3 safetensors through Burn store")?;

        if !result.errors.is_empty() {
            bail!("failed to load PP-DocLayoutV3 tensors: {}", result);
        }
        if !result.missing.is_empty() {
            bail!("PP-DocLayoutV3 checkpoint is missing tensors: {}", result);
        }

        let dtype = if cuda_bf16 {
            model = cast_module_float(model, FloatDType::BF16);
            DType::BF16
        } else {
            DType::F32
        };

        Ok(Self {
            model,
            device,
            dtype,
        })
    }

    pub fn inference(&self, image: &DynamicImage, threshold: f32) -> Result<PPDocLayoutV3Result> {
        let input = preprocess(image, &self.device, self.dtype);
        let output = self.model.forward(input);
        let logits = tensor_to_f32_vec(output.logits)?;
        let boxes = tensor_to_f32_vec(output.pred_boxes)?;
        let order_logits = tensor_to_f32_vec(output.order_logits)?;

        Ok(postprocess(
            &logits,
            &boxes,
            &order_logits,
            image.width(),
            image.height(),
            threshold,
        ))
    }
}

fn make_device(cpu: bool) -> (Device, bool) {
    #[cfg(feature = "cuda")]
    {
        if !cpu {
            let mut device = Device::cuda(0);
            if let Err(error) = device.configure(FloatDType::BF16) {
                tracing::warn!(%error, "failed to configure Burn CUDA default dtype to BF16");
            }
            return (device, true);
        }
    }

    let mut device = Device::wgpu(if cpu {
        DeviceKind::Cpu
    } else {
        DeviceKind::DefaultDevice
    });
    if let Err(error) = device.configure(FloatDType::F32) {
        tracing::warn!(%error, "failed to configure Burn WGPU default dtype to F32");
    }
    (device, false)
}

fn preprocess(image: &DynamicImage, device: &Device, dtype: DType) -> Tensor<4> {
    let resized = image::imageops::resize(
        &image.to_rgb8(),
        IMAGE_SIZE as u32,
        IMAGE_SIZE as u32,
        FilterType::CatmullRom,
    );
    let plane = IMAGE_SIZE * IMAGE_SIZE;
    let mut data = vec![0.0_f32; 3 * plane];
    for (x, y, pixel) in resized.enumerate_pixels() {
        let index = y as usize * IMAGE_SIZE + x as usize;
        data[index] = pixel[0] as f32 / 255.0;
        data[plane + index] = pixel[1] as f32 / 255.0;
        data[2 * plane + index] = pixel[2] as f32 / 255.0;
    }
    Tensor::from_data(
        TensorData::new(data, [1, 3, IMAGE_SIZE, IMAGE_SIZE]),
        (device, dtype),
    )
}

#[derive(Clone, Copy)]
struct Candidate {
    score: f32,
    query: usize,
    label_id: usize,
}

fn postprocess(
    logits: &[f32],
    boxes: &[f32],
    order_logits: &[f32],
    image_width: u32,
    image_height: u32,
    threshold: f32,
) -> PPDocLayoutV3Result {
    let mut candidates = Vec::with_capacity(NUM_QUERIES * NUM_LABELS);
    for query in 0..NUM_QUERIES {
        for label_id in 0..NUM_LABELS {
            let logit = logits[query * NUM_LABELS + label_id];
            candidates.push(Candidate {
                score: sigmoid_scalar(logit),
                query,
                label_id,
            });
        }
    }

    let top = NUM_QUERIES.min(candidates.len());
    candidates.select_nth_unstable_by(top, |a, b| score_order(b.score, a.score));
    candidates.truncate(top);
    candidates.sort_unstable_by(|a, b| score_order(b.score, a.score));

    let order_seq = order_sequence(order_logits);
    let image_width_f = image_width as f32;
    let image_height_f = image_height as f32;
    let mut regions = Vec::new();

    for candidate in candidates {
        if candidate.score < threshold {
            continue;
        }
        let base = candidate.query * 4;
        let cx = boxes[base];
        let cy = boxes[base + 1];
        let bw = boxes[base + 2];
        let bh = boxes[base + 3];
        let x0 = ((cx - bw * 0.5) * image_width_f).clamp(0.0, image_width_f);
        let y0 = ((cy - bh * 0.5) * image_height_f).clamp(0.0, image_height_f);
        let x1 = ((cx + bw * 0.5) * image_width_f).clamp(0.0, image_width_f);
        let y1 = ((cy + bh * 0.5) * image_height_f).clamp(0.0, image_height_f);

        if x1 <= x0 || y1 <= y0 {
            continue;
        }

        regions.push(PPDocLayoutV3Region {
            label: ID2LABEL[candidate.label_id].to_string(),
            label_id: candidate.label_id,
            score: candidate.score,
            bbox: [x0, y0, x1, y1],
            polygon_points: vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
            order: order_seq[candidate.query],
        });
    }

    regions.sort_unstable_by(|a, b| {
        a.order
            .cmp(&b.order)
            .then_with(|| score_order(b.score, a.score))
    });

    PPDocLayoutV3Result {
        width: image_width,
        height: image_height,
        regions,
    }
}

fn score_order(left: f32, right: f32) -> Ordering {
    left.partial_cmp(&right).unwrap_or(Ordering::Equal)
}

fn sigmoid_scalar(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn order_sequence(order_logits: &[f32]) -> Vec<usize> {
    let mut votes = vec![0.0_f32; NUM_QUERIES];
    for query in 0..NUM_QUERIES {
        for other in (query + 1)..NUM_QUERIES {
            votes[query] += sigmoid_scalar(order_logits[query * NUM_QUERIES + other]);
        }
        for other in 0..query {
            votes[query] += 1.0 - sigmoid_scalar(order_logits[other * NUM_QUERIES + query]);
        }
    }

    let mut pointers = (0..NUM_QUERIES).collect::<Vec<_>>();
    pointers.sort_unstable_by(|&left, &right| score_order(votes[left], votes[right]));

    let mut order = vec![0_usize; NUM_QUERIES];
    for (rank, query) in pointers.into_iter().enumerate() {
        order[query] = rank + 1;
    }
    order
}
