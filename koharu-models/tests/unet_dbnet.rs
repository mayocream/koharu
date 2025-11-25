use std::collections::HashMap;
use std::path::PathBuf;

use candle_core::{Device, IndexOp, Tensor};
use image::{DynamicImage, GenericImage, GenericImageView, GrayImage, Rgba, imageops, open};
use imageproc::{
    drawing::draw_hollow_rect_mut,
    rect::Rect,
    region_labelling::{Connectivity, connected_components},
};
use koharu_models::{dbnet::DbNet, unet::Unet, yolo_v5::YOLOv5};

fn rand_feat(device: &Device, shape: (usize, usize, usize, usize)) -> anyhow::Result<Tensor> {
    Tensor::randn(0f32, 1f32, shape, device).map_err(Into::into)
}

#[test]
fn unet_and_dbnet_forward_shapes() -> anyhow::Result<()> {
    let device = Device::Cpu;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let weights = manifest_dir.join("../temp/comictextdetector.pt");

    let unet = Unet::load(&weights, &device)?;
    let dbnet = DbNet::load(&weights, &device)?;

    // Feature maps follow BallonsTranslator TextDetBase usage.
    let f3 = rand_feat(&device, (1, 512, 20, 20))?;
    let f20 = rand_feat(&device, (1, 512, 20, 20))?;
    let f40 = rand_feat(&device, (1, 256, 40, 40))?;
    let f80 = rand_feat(&device, (1, 128, 80, 80))?;
    let f160 = rand_feat(&device, (1, 64, 160, 160))?;

    let (mask, features) = unet.forward_with_features(&f160, &f80, &f40, &f20, &f3)?;
    let (b, c, h, w) = mask.dims4()?;
    assert_eq!((b, c, h, w), (1, 1, 640, 640));

    let shrink_thresh = dbnet.forward(&features[0], &features[1], &features[2])?;
    let (b2, c2, h2, w2) = shrink_thresh.dims4()?;
    assert_eq!((b2, c2, h2, w2), (1, 2, 640, 640));

    Ok(())
}

#[allow(unused)]
#[derive(Debug)]
struct Preprocessed {
    tensor: Tensor,
    orig_size: (u32, u32),
    resized: (usize, usize),
    target: usize,
}

fn preprocess(
    image: &DynamicImage,
    device: &Device,
    target: usize,
) -> anyhow::Result<Preprocessed> {
    let (orig_w, orig_h) = image.dimensions();
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
        .map_err(|e| anyhow::anyhow!("pad copy failed: {e}"))?;

    let data: Vec<f32> = (0..3)
        .flat_map(|c| padded.pixels().map(move |p| p.0[c] as f32 / 255.0))
        .collect();
    let tensor = Tensor::from_vec(data, (1, 3, target, target), device)?;
    Ok(Preprocessed {
        tensor,
        orig_size: (orig_w, orig_h),
        resized: (resized_w, resized_h),
        target,
    })
}

fn mask_to_image(mask_hw: (usize, usize), mask: &Tensor) -> anyhow::Result<GrayImage> {
    let mask = mask.to_device(&Device::Cpu)?;
    let v: Vec<Vec<f32>> = mask.to_vec2()?;
    let data: Vec<u8> = v
        .into_iter()
        .flatten()
        .map(|x| (x.clamp(0.0, 1.0) * 255.0) as u8)
        .collect();
    GrayImage::from_raw(mask_hw.1 as u32, mask_hw.0 as u32, data)
        .ok_or_else(|| anyhow::anyhow!("failed to build mask image"))
}

fn rects_from_binary(img: &GrayImage, min_area: u32) -> Vec<Rect> {
    let labels = connected_components(img, Connectivity::Eight, image::Luma([0u8]));
    let mut stats: HashMap<u32, (u32, u32, u32, u32, u32)> = HashMap::new();
    for (x, y, pixel) in labels.enumerate_pixels() {
        let label = pixel[0] as u32;
        if label == 0 {
            continue;
        }
        stats
            .entry(label)
            .and_modify(|e| {
                e.0 = e.0.min(x);
                e.1 = e.1.min(y);
                e.2 = e.2.max(x);
                e.3 = e.3.max(y);
                e.4 += 1;
            })
            .or_insert((x, y, x, y, 1));
    }
    stats
        .into_values()
        .filter(|(_, _, _, _, area)| *area >= min_area)
        .map(|(x1, y1, x2, y2, _)| Rect::at(x1 as i32, y1 as i32).of_size(x2 - x1 + 1, y2 - y1 + 1))
        .collect()
}

#[test]
fn unet_dbnet_end_to_end_marks_image() -> anyhow::Result<()> {
    let device = Device::Cpu;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let weights_pt = manifest_dir.join("../temp/comictextdetector.pt");
    let yolo_weights = manifest_dir.join("models/comic-text-detector-yolov5.safetensors");
    let image_path = manifest_dir.join("../data/bluearchive_comics/1.jpg");

    let yolo = YOLOv5::load(&yolo_weights, &device)?;
    let unet = Unet::load(&weights_pt, &device)?;
    let dbnet = DbNet::load(&weights_pt, &device)?;

    let image = open(&image_path)?;
    let prep = preprocess(&image, &device, 640)?;
    let yolo_out = yolo.forward(&prep.tensor)?;
    let features = &yolo_out.features;

    let (mask, unet_feats) = unet.forward_with_features(
        &features[0],
        &features[1],
        &features[2],
        &features[3],
        &features[4],
    )?;
    let shrink_thresh = dbnet.forward(&unet_feats[0], &unet_feats[1], &unet_feats[2])?;

    let (_, _, mh, mw) = mask.dims4()?;
    let (_, _, sh, sw) = shrink_thresh.dims4()?;
    let shrink = shrink_thresh.i((0, 0))?;

    // Resize mask back to original image size.
    let mask_img = mask_to_image((mh, mw), &mask.i((0, 0))?)?;
    let mask_cropped = imageops::crop_imm(
        &mask_img,
        0,
        0,
        prep.resized.0 as u32,
        prep.resized.1 as u32,
    )
    .to_image();
    let mask_resized = imageops::resize(
        &mask_cropped,
        prep.orig_size.0,
        prep.orig_size.1,
        imageops::FilterType::Triangle,
    );

    // Threshold shrink map to get regions.
    let shrink_img = mask_to_image((sh, sw), &shrink)?;
    let shrink_cropped = imageops::crop_imm(
        &shrink_img,
        0,
        0,
        prep.resized.0 as u32,
        prep.resized.1 as u32,
    )
    .to_image();
    let shrink_resized = imageops::resize(
        &shrink_cropped,
        prep.orig_size.0,
        prep.orig_size.1,
        imageops::FilterType::Triangle,
    );
    let binary = imageproc::contrast::threshold(
        &shrink_resized,
        128,
        imageproc::contrast::ThresholdType::Binary,
    );
    let rects = rects_from_binary(&binary, 50);

    // Draw mask and rects on copies.
    let mut mask_rgba = image::DynamicImage::ImageLuma8(mask_resized.clone()).to_rgba8();
    for rect in &rects {
        draw_hollow_rect_mut(&mut mask_rgba, *rect, Rgba([255, 0, 0, 255]));
    }
    let mut orig_draw = image.to_rgba8();
    for rect in &rects {
        draw_hollow_rect_mut(&mut orig_draw, *rect, Rgba([255, 0, 0, 255]));
    }

    let out_dir = manifest_dir.join("target");
    std::fs::create_dir_all(&out_dir)?;
    mask_rgba.save(out_dir.join("unet_mask.png"))?;
    orig_draw.save(out_dir.join("unet_lines.png"))?;

    assert!(
        !rects.is_empty(),
        "expected at least one detected region; none found"
    );

    Ok(())
}
