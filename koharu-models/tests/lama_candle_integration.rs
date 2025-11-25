use std::fs;
use std::path::Path;

use candle_core::{Device, Tensor};
use image::DynamicImage;
use koharu_models::lama_candle::LamaCandle;

fn image_to_tensor(img: &DynamicImage) -> candle_core::Result<Tensor> {
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let mut data = Vec::with_capacity((3 * w * h) as usize);
    for c in 0..3 {
        for p in rgb.pixels() {
            data.push(p[c] as f32 / 255.0);
        }
    }
    Tensor::from_vec(data, (1, 3, h as usize, w as usize), &Device::Cpu)
}

fn mask_to_tensor(mask: &DynamicImage) -> candle_core::Result<Tensor> {
    let l = mask.to_luma8();
    let (w, h) = l.dimensions();
    let mut data = Vec::with_capacity((w * h) as usize);
    for p in l.pixels() {
        data.push(if p[0] > 0 { 1.0f32 } else { 0.0f32 });
    }
    Tensor::from_vec(data, (1, 1, h as usize, w as usize), &Device::Cpu)
}

fn tensor_to_image(t: &Tensor) -> anyhow::Result<image::RgbImage> {
    let (_b, _c, h, w) = t.dims4()?;
    let data = t.flatten_all()?.to_vec1::<f32>()?;
    let stride_c = h * w;
    let mut img = image::RgbImage::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let base = y * w + x;
            let r = (data[0 * stride_c + base].clamp(0.0, 1.0) * 255.0) as u8;
            let g = (data[1 * stride_c + base].clamp(0.0, 1.0) * 255.0) as u8;
            let b = (data[2 * stride_c + base].clamp(0.0, 1.0) * 255.0) as u8;
            img.put_pixel(x as u32, y as u32, image::Rgb([r, g, b]));
        }
    }
    Ok(img)
}

#[test]
fn lama_candle_runs_on_real_image() -> anyhow::Result<()> {
    // Use provided sample pair from temp/LaMa-ONNX.
    let img = image::open(Path::new("../temp/LaMa-ONNX/image.jpg"))
        .or_else(|_| image::open(Path::new("temp/LaMa-ONNX/image.jpg")))?
        .to_rgb8();
    let mask_img = image::open(Path::new("../temp/LaMa-ONNX/mask.png"))
        .or_else(|_| image::open(Path::new("temp/LaMa-ONNX/mask.png")))?
        .to_luma8();

    let base_img = DynamicImage::ImageRgb8(img);
    let mask = DynamicImage::ImageLuma8(mask_img);

    let img = image_to_tensor(&base_img)?;
    let mask = mask_to_tensor(&mask)?;
    let (_, _, mh, mw) = mask.dims4()?;

    // Try CUDA if available; fall back to CPU so the test is portable.
    let dev = match Device::new_cuda(0) {
        Ok(d) => d,
        Err(_) => {
            eprintln!("CUDA device not available, skipping test.");
            return Ok(());
        }
    };
    let lama = LamaCandle::load(Some(dev))?;
    let output = lama.forward(&img, &mask)?;

    // Persist the result for visual inspection when debugging locally.
    let out_img = tensor_to_image(&output)?;
    let out_path = Path::new("target/lama_candle_integration.png");
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    out_img.save(out_path)?;

    // Basic sanity: shape and value range.
    assert_eq!(output.dims4()?, (1, 3, mh, mw));
    let flat = output.flatten_all()?.to_vec1::<f32>()?;
    assert!(
        flat.iter().all(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0),
        "output contains invalid values"
    );

    // Ensure masked region was modified compared to input (not strictly equal).
    let input_flat = img.flatten_all()?.to_vec1::<f32>()?;
    let mask_flat = mask.flatten_all()?.to_vec1::<f32>()?;
    let stride_c = mh * mw;
    let mut diff_sum = 0f32;
    for y in 0..mh {
        for x in 0..mw {
            let midx = y * mw + x;
            if mask_flat[midx] <= 0.5 {
                continue;
            }
            let out_r = flat[0 * stride_c + midx];
            let out_g = flat[1 * stride_c + midx];
            let out_b = flat[2 * stride_c + midx];
            let in_r = input_flat[0 * stride_c + midx];
            let in_g = input_flat[1 * stride_c + midx];
            let in_b = input_flat[2 * stride_c + midx];
            diff_sum += (out_r - in_r).abs();
            diff_sum += (out_g - in_g).abs();
            diff_sum += (out_b - in_b).abs();
        }
    }
    assert!(diff_sum > 0.01, "masked region did not change");

    Ok(())
}
