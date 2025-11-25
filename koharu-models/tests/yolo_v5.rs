use std::path::PathBuf;

use candle_core::{Device, Error, IndexOp, Tensor};
use candle_transformers::object_detection::{Bbox, non_maximum_suppression};
use image::{DynamicImage, GenericImage, GenericImageView, Rgba, imageops, open};
use imageproc::{drawing::draw_hollow_rect_mut, rect::Rect};
use koharu_models::yolo_v5::YOLOv5;

fn round_up_to_multiple(value: usize, multiple: usize) -> usize {
    if value % multiple == 0 {
        value
    } else {
        value + multiple - value % multiple
    }
}

fn preprocess_image(
    image: &DynamicImage,
    device: &Device,
    target_size: Option<usize>,
) -> anyhow::Result<(Tensor, (u32, u32), (usize, usize))> {
    let (orig_w, orig_h) = image.dimensions();
    let longest_side = orig_w.max(orig_h) as usize;
    let mut target = target_size.unwrap_or_else(|| round_up_to_multiple(longest_side, 32));
    target = target.max(32);

    let scale = (target as f32 / orig_h as f32).min(target as f32 / orig_w as f32);
    let resized_w = ((orig_w as f32 * scale).round() as usize).max(1);
    let resized_h = ((orig_h as f32 * scale).round() as usize).max(1);

    let resized = imageops::resize(
        &image.to_rgb8(),
        resized_w as u32,
        resized_h as u32,
        imageops::FilterType::Triangle,
    );
    let mut padded =
        image::RgbImage::from_pixel(target as u32, target as u32, image::Rgb([0, 0, 0]));
    padded
        .copy_from(&resized, 0, 0)
        .map_err(|_| Error::Msg("failed to place resized image on canvas".to_string()))?;

    let data: Vec<f32> = (0..3)
        .flat_map(|c| padded.pixels().map(move |p| p.0[c] as f32 / 255.0))
        .collect();

    let input_tensor = Tensor::from_vec(data, (1, 3, target, target), device)?;
    Ok((input_tensor, (orig_w, orig_h), (resized_w, resized_h)))
}

fn postprocess_predictions(
    preds: &Tensor,
    orig_size: (u32, u32),
    resized_size: (usize, usize),
    conf_threshold: f32,
    nms_threshold: f32,
) -> anyhow::Result<Vec<[f32; 4]>> {
    let (batch, _, outputs) = preds.dims3()?;
    if batch != 1 {
        return Err(Error::Msg("post-processing supports batch size 1 only".to_string()).into());
    }
    if outputs != 7 {
        return Err(Error::Msg(format!(
            "unexpected prediction width {}, expected 7",
            outputs
        ))
        .into());
    }

    let pred = preds.i(0)?.to_device(&Device::Cpu)?;
    let pred: Vec<Vec<f32>> = pred.to_vec2()?;

    let scale_x = orig_size.0 as f32 / resized_size.0 as f32;
    let scale_y = orig_size.1 as f32 / resized_size.1 as f32;
    let mut boxes: Vec<Vec<Bbox<usize>>> = (0..2).map(|_| Vec::new()).collect();

    for row in pred {
        if row.len() < 7 {
            continue;
        }
        let obj_conf = row[4];
        let class_scores = &row[5..];
        let (class_idx, class_prob) = class_scores
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0, &0.0));
        let confidence = obj_conf * *class_prob;
        if confidence < conf_threshold {
            continue;
        }

        let [x, y, w, h] = [row[0], row[1], row[2], row[3]];
        let half_w = w * 0.5;
        let half_h = h * 0.5;
        let mut xmin = (x - half_w) * scale_x;
        let mut ymin = (y - half_h) * scale_y;
        let mut xmax = (x + half_w) * scale_x;
        let mut ymax = (y + half_h) * scale_y;

        let max_x = orig_size.0 as f32;
        let max_y = orig_size.1 as f32;
        xmin = xmin.clamp(0.0, max_x);
        xmax = xmax.clamp(0.0, max_x);
        ymin = ymin.clamp(0.0, max_y);
        ymax = ymax.clamp(0.0, max_y);

        boxes[class_idx].push(Bbox {
            xmin,
            ymin,
            xmax,
            ymax,
            confidence,
            data: class_idx,
        });
    }

    non_maximum_suppression(&mut boxes, nms_threshold);

    let mut detections = Vec::new();
    for per_class in boxes.into_iter() {
        for bbox in per_class {
            detections.push([bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax]);
        }
    }
    Ok(detections)
}

#[test]
fn yolov5_detects_comic_text_blocks() -> anyhow::Result<()> {
    let device = Device::Cpu;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let weights = manifest_dir.join("models/comic-text-detector-yolov5.safetensors");
    let image_path = manifest_dir.join("../data/bluearchive_comics/1.jpg");

    let model = YOLOv5::load(&weights, &device)?;
    let image = open(&image_path)?;
    let (input, orig_size, resized_size) = preprocess_image(&image, &device, None)?;
    let output = model.forward(&input)?;
    let detections =
        postprocess_predictions(&output.predictions, orig_size, resized_size, 0.4, 0.35)?;

    assert!(
        detections.len() >= 8,
        "expected at least 8 boxes after NMS, got {}",
        detections.len()
    );

    // Save a copy with boxes for debugging.
    let mut drawn = image.to_rgba8();
    for [xmin, ymin, xmax, ymax] in &detections {
        if xmax <= xmin || ymax <= ymin {
            continue;
        }
        let rect = Rect::at(*xmin as i32, *ymin as i32)
            .of_size((*xmax - *xmin) as u32, (*ymax - *ymin) as u32);
        draw_hollow_rect_mut(&mut drawn, rect, Rgba([255, 0, 0, 255]));
    }
    let save_path = manifest_dir.join("target").join("yolov5_test_boxes.png");
    if let Some(parent) = save_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    drawn.save(&save_path)?;

    Ok(())
}
