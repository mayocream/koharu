mod fft;
mod model;

use crate::types::TextRegion;
use anyhow::{Result, bail};
use candle_core::{DType, Device, Tensor};
use image::{
    DynamicImage, GenericImageView, GrayImage, Luma, Rgb, RgbImage,
    imageops::{crop_imm, replace},
};
use imageproc::{
    contours::find_contours, distance_transform::Norm, drawing::draw_polygon_mut, edges::canny,
    filter::gaussian_blur_f32, morphology::dilate, point::Point,
};
use koharu_runtime::RuntimeManager;
use tracing::instrument;

use crate::{
    device,
    inpainting::{binarize_mask, extract_alpha, restore_alpha_channel},
    loading,
};

const HF_REPO: &str = "mayocream/lama-manga";

koharu_runtime::declare_hf_model_package!(
    id: "model:lama:weights",
    repo: "mayocream/lama-manga",
    file: "lama-manga.safetensors",
    bootstrap: false,
    order: 130,
);

const BALLOON_CANNY_LOW: f32 = 70.0;
const BALLOON_CANNY_HIGH: f32 = 140.0;
const BALLOON_WINDOW_RATIO: f64 = 1.7;
const BALLOON_WINDOW_ASPECT_RATIO: f64 = 1.0;
const SIMPLE_BG_THRESHOLD_LOW_VARIANCE: f64 = 10.0;
const SIMPLE_BG_THRESHOLD_HIGH_VARIANCE: f64 = 7.0;
const SIMPLE_BG_CHANNEL_STD_SWITCH: f64 = 1.0;
type Xyxy = [u32; 4];

struct BalloonMasks {
    balloon_mask: GrayImage,
    non_text_mask: GrayImage,
}

pub struct Lama {
    model: model::Lama,
    device: Device,
}

impl Lama {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let device = device(cpu)?;
        let weights_path = runtime
            .downloads()
            .huggingface_model(HF_REPO, "lama-manga.safetensors")
            .await?;
        let model = loading::load_buffered_safetensors_path(&weights_path, &device, |vb| {
            model::Lama::load(&vb)
        })?;

        Ok(Self { model, device })
    }

    #[instrument(level = "debug", skip_all)]
    fn forward(&self, image: &Tensor, mask: &Tensor) -> Result<Tensor> {
        self.model.forward(image, mask)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_model(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
    ) -> Result<DynamicImage> {
        let (image_tensor, mask_tensor) = self.preprocess(image, mask)?;
        let output = self.forward(&image_tensor, &mask_tensor)?;
        self.postprocess(&output)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(&self, image: &DynamicImage, mask: &DynamicImage) -> Result<DynamicImage> {
        self.inference_with_blocks(image, mask, None)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_with_blocks(
        &self,
        image: &DynamicImage,
        mask: &DynamicImage,
        text_blocks: Option<&[TextRegion]>,
    ) -> Result<DynamicImage> {
        if image.dimensions() != mask.dimensions() {
            bail!(
                "image and mask dimensions dismatch: image is {:?}, mask is {:?}",
                image.dimensions(),
                mask.dimensions()
            );
        }

        let binary_mask = binarize_mask(mask);
        let output_rgb = if let Some(blocks) = text_blocks.filter(|blocks| !blocks.is_empty()) {
            let image_rgb = image.to_rgb8();
            self.inference_blockwise(&image_rgb, &binary_mask, blocks)?
        } else {
            self.inference_crop(&image.to_rgb8(), &binary_mask)?
        };

        if image.color().has_alpha() {
            let original_alpha = image.to_rgba8();
            let alpha = extract_alpha(&original_alpha);
            let output = restore_alpha_channel(&output_rgb, &alpha, &binary_mask);
            Ok(DynamicImage::ImageRgba8(output))
        } else {
            Ok(DynamicImage::ImageRgb8(output_rgb))
        }
    }

    #[instrument(level = "debug", skip_all)]
    fn inference_crop(&self, image: &RgbImage, mask: &GrayImage) -> Result<RgbImage> {
        if let Some(filled) = try_fill_balloon(image, mask) {
            return Ok(filled);
        }

        self.inference_model_rgb(image, mask)
    }

    #[instrument(level = "debug", skip_all)]
    fn inference_blockwise(
        &self,
        image: &RgbImage,
        mask: &GrayImage,
        text_blocks: &[TextRegion],
    ) -> Result<RgbImage> {
        let (im_w, im_h) = image.dimensions();
        let mut inpainted = image.clone();
        let mut working_mask = mask.clone();

        for block in text_blocks {
            let Some(xyxy) = block_xyxy(block, im_w, im_h) else {
                continue;
            };
            let xyxy_e = enlarge_window(
                xyxy,
                im_w,
                im_h,
                BALLOON_WINDOW_RATIO,
                BALLOON_WINDOW_ASPECT_RATIO,
            );
            let crop_width = xyxy_e[2].saturating_sub(xyxy_e[0]);
            let crop_height = xyxy_e[3].saturating_sub(xyxy_e[1]);
            if crop_width == 0 || crop_height == 0 {
                continue;
            }

            let crop_image =
                crop_imm(&inpainted, xyxy_e[0], xyxy_e[1], crop_width, crop_height).to_image();
            let crop_mask =
                crop_imm(&working_mask, xyxy_e[0], xyxy_e[1], crop_width, crop_height).to_image();

            let output = if count_nonzero(&crop_mask) == 0 {
                crop_image
            } else if let Some(filled) = try_fill_balloon(&crop_image, &crop_mask) {
                filled
            } else {
                self.inference_model_rgb(&crop_image, &crop_mask)?
            };

            replace(
                &mut inpainted,
                &output,
                i64::from(xyxy_e[0]),
                i64::from(xyxy_e[1]),
            );
            clear_mask_bbox(&mut working_mask, xyxy);
        }

        Ok(inpainted)
    }

    #[instrument(level = "debug", skip_all)]
    fn inference_model_rgb(&self, image: &RgbImage, mask: &GrayImage) -> Result<RgbImage> {
        Ok(self
            .inference_model(
                &DynamicImage::ImageRgb8(image.clone()),
                &DynamicImage::ImageLuma8(mask.clone()),
            )?
            .to_rgb8())
    }

    #[instrument(level = "debug", skip_all)]
    fn preprocess(&self, image: &DynamicImage, mask: &DynamicImage) -> Result<(Tensor, Tensor)> {
        if image.dimensions() != mask.dimensions() {
            bail!(
                "image and mask dimensions dismatch: image is {:?}, mask is {:?}",
                image.dimensions(),
                mask.dimensions()
            );
        }
        let (w, h) = (image.width() as usize, image.height() as usize);

        let rgb = image.to_rgb8().into_raw();
        let luma = mask.to_luma8().into_raw();

        let image_tensor = (Tensor::from_vec(rgb, (1, h, w, 3), &self.device)?
            .permute((0, 3, 1, 2))?
            .to_dtype(DType::F32)?
            * (1. / 255.))?;

        let mask_tensor = Tensor::from_vec(luma, (1, h, w, 1), &self.device)?
            .permute((0, 3, 1, 2))?
            .to_dtype(DType::F32)?
            .gt(1.0f32)?;

        Ok((image_tensor, mask_tensor))
    }

    #[instrument(level = "debug", skip_all)]
    fn postprocess(&self, output: &Tensor) -> Result<DynamicImage> {
        let output = output.squeeze(0)?;
        let (channels, height, width) = output.dims3()?;
        if channels != 3 {
            bail!("expected 3 channels in output, got {channels}");
        }
        let output = (output * 255.)?
            .clamp(0., 255.)?
            .permute((1, 2, 0))?
            .to_dtype(DType::U8)?;
        let raw: Vec<u8> = output.flatten_all()?.to_vec1()?;
        let image = RgbImage::from_raw(width as u32, height as u32, raw)
            .ok_or_else(|| anyhow::anyhow!("failed to create image buffer from model output"))?;
        Ok(DynamicImage::ImageRgb8(image))
    }
}

fn block_xyxy(block: &TextRegion, width: u32, height: u32) -> Option<Xyxy> {
    let x1 = block.x.floor().max(0.0) as u32;
    let y1 = block.y.floor().max(0.0) as u32;
    let x2 = (block.x + block.width).ceil().max(block.x.floor()) as u32;
    let y2 = (block.y + block.height).ceil().max(block.y.floor()) as u32;

    let x1 = x1.min(width);
    let y1 = y1.min(height);
    let x2 = x2.min(width);
    let y2 = y2.min(height);

    if x2 <= x1 || y2 <= y1 {
        return None;
    }

    Some([x1, y1, x2, y2])
}

fn enlarge_window(rect: Xyxy, im_w: u32, im_h: u32, ratio: f64, aspect_ratio: f64) -> Xyxy {
    debug_assert!(ratio > 1.0);

    let [x1, y1, x2, y2] = rect;
    let w = f64::from(x2.saturating_sub(x1));
    let h = f64::from(y2.saturating_sub(y1));
    if w <= 0.0 || h <= 0.0 || aspect_ratio <= 0.0 {
        return [0, 0, 0, 0];
    }

    let a = aspect_ratio;
    let b = w + h * aspect_ratio;
    let c = (1.0 - ratio) * w * h;
    let discriminant = (b * b - 4.0 * a * c).max(0.0);
    let delta = ((-b + discriminant.sqrt()) / (2.0 * a) / 2.0).round();
    let mut delta_h = delta.max(0.0) as u32;
    let mut delta_w = (delta * aspect_ratio).round().max(0.0) as u32;

    delta_w = delta_w.min(x1).min(im_w.saturating_sub(x2));
    delta_h = delta_h.min(y1).min(im_h.saturating_sub(y2));

    [
        x1.saturating_sub(delta_w),
        y1.saturating_sub(delta_h),
        (x2 + delta_w).min(im_w),
        (y2 + delta_h).min(im_h),
    ]
}

fn try_fill_balloon(image: &RgbImage, mask: &GrayImage) -> Option<RgbImage> {
    let masks = extract_balloon_mask(image, mask)?;
    let average_bg_color = median_rgb(image, &masks.non_text_mask)?;
    let std_rgb = color_stddev(image, &masks.non_text_mask, average_bg_color);
    let inpaint_thresh = if stddev3(std_rgb) > SIMPLE_BG_CHANNEL_STD_SWITCH {
        SIMPLE_BG_THRESHOLD_HIGH_VARIANCE
    } else {
        SIMPLE_BG_THRESHOLD_LOW_VARIANCE
    };
    let std_max = std_rgb.into_iter().fold(0.0, f64::max);

    if std_max >= inpaint_thresh {
        return None;
    }

    let mut result = image.clone();
    let fill = [
        average_bg_color[0] as u8,
        average_bg_color[1] as u8,
        average_bg_color[2] as u8,
    ];
    for (x, y, pixel) in masks.balloon_mask.enumerate_pixels() {
        if pixel.0[0] > 0 {
            result.put_pixel(x, y, Rgb(fill));
        }
    }

    Some(result)
}

fn extract_balloon_mask(image: &RgbImage, mask: &GrayImage) -> Option<BalloonMasks> {
    if image.dimensions() != mask.dimensions() {
        return None;
    }

    let text_bbox = non_zero_bbox(mask)?;
    let text_sum = count_nonzero(mask);
    if text_sum == 0 {
        return None;
    }

    let gray = DynamicImage::ImageRgb8(image.clone()).to_luma8();
    let blurred = gaussian_blur_f32(&gray, 1.0);
    let mut cannyed = canny(&blurred, BALLOON_CANNY_LOW, BALLOON_CANNY_HIGH);
    cannyed = dilate(&cannyed, Norm::LInf, 1);
    draw_binary_border(&mut cannyed);
    subtract_binary_mask(&mut cannyed, mask);

    let contours = find_contours::<i32>(&cannyed);
    let (width, height) = cannyed.dimensions();
    let mut best_mask = None;
    let mut best_area = f64::INFINITY;

    for contour in contours {
        let Some(polygon) = contour_polygon(&contour.points) else {
            continue;
        };
        let bbox = polygon_bbox(&polygon)?;
        if bbox[0] > text_bbox[0]
            || bbox[1] > text_bbox[1]
            || bbox[2] < text_bbox[2]
            || bbox[3] < text_bbox[3]
        {
            continue;
        }

        let mut candidate = GrayImage::new(width, height);
        draw_polygon_mut(&mut candidate, &polygon, Luma([255u8]));
        if count_overlap(&candidate, mask) < text_sum {
            continue;
        }

        let area = polygon_area(&polygon);
        if area < best_area {
            best_area = area;
            best_mask = Some(candidate);
        }
    }

    let balloon_mask = best_mask?;
    let mut non_text_mask = balloon_mask.clone();
    for (x, y, pixel) in mask.enumerate_pixels() {
        if pixel.0[0] > 0 {
            non_text_mask.put_pixel(x, y, Luma([0]));
        }
    }

    Some(BalloonMasks {
        balloon_mask,
        non_text_mask,
    })
}

fn contour_polygon(points: &[Point<i32>]) -> Option<Vec<Point<i32>>> {
    let mut polygon = points.to_vec();
    if polygon.len() < 3 {
        return None;
    }
    if polygon.first() == polygon.last() {
        polygon.pop();
    }
    if polygon.len() < 3 {
        return None;
    }
    Some(polygon)
}

fn polygon_bbox(points: &[Point<i32>]) -> Option<Xyxy> {
    let first = points.first()?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x;
    let mut max_y = first.y;
    for point in points.iter().skip(1) {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }

    Some([
        min_x.max(0) as u32,
        min_y.max(0) as u32,
        max_x.max(min_x).saturating_add(1) as u32,
        max_y.max(min_y).saturating_add(1) as u32,
    ])
}

fn polygon_area(points: &[Point<i32>]) -> f64 {
    let mut area = 0.0;
    for index in 0..points.len() {
        let current = points[index];
        let next = points[(index + 1) % points.len()];
        area += f64::from(current.x) * f64::from(next.y) - f64::from(next.x) * f64::from(current.y);
    }
    area.abs() * 0.5
}

fn draw_binary_border(image: &mut GrayImage) {
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return;
    }

    for x in 0..width {
        image.put_pixel(x, 0, Luma([255]));
        image.put_pixel(x, height - 1, Luma([255]));
    }
    for y in 0..height {
        image.put_pixel(0, y, Luma([255]));
        image.put_pixel(width - 1, y, Luma([255]));
    }
}

fn subtract_binary_mask(image: &mut GrayImage, mask: &GrayImage) {
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        if mask.get_pixel(x, y).0[0] > 0 {
            pixel.0[0] = 0;
        }
    }
}

fn non_zero_bbox(mask: &GrayImage) -> Option<Xyxy> {
    let (width, height) = mask.dimensions();
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;

    for (x, y, pixel) in mask.enumerate_pixels() {
        if pixel.0[0] == 0 {
            continue;
        }
        found = true;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }

    found.then_some([
        min_x,
        min_y,
        max_x.saturating_add(1),
        max_y.saturating_add(1),
    ])
}

fn clear_mask_bbox(mask: &mut GrayImage, bbox: Xyxy) {
    for y in bbox[1]..bbox[3] {
        for x in bbox[0]..bbox[2] {
            mask.put_pixel(x, y, Luma([0]));
        }
    }
}

fn count_nonzero(mask: &GrayImage) -> u32 {
    mask.pixels().filter(|pixel| pixel.0[0] > 0).count() as u32
}

fn count_overlap(left: &GrayImage, right: &GrayImage) -> u32 {
    left.pixels()
        .zip(right.pixels())
        .filter(|(l, r)| l.0[0] > 0 && r.0[0] > 0)
        .count() as u32
}

fn median_rgb(image: &RgbImage, mask: &GrayImage) -> Option<[f64; 3]> {
    let mut channels = [Vec::new(), Vec::new(), Vec::new()];
    for (pixel, mask_pixel) in image.pixels().zip(mask.pixels()) {
        if mask_pixel.0[0] == 0 {
            continue;
        }
        channels[0].push(pixel.0[0]);
        channels[1].push(pixel.0[1]);
        channels[2].push(pixel.0[2]);
    }

    Some([
        median_channel(&channels[0])?,
        median_channel(&channels[1])?,
        median_channel(&channels[2])?,
    ])
}

fn median_channel(values: &[u8]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    let mut values = values.to_vec();
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        Some((f64::from(values[mid - 1]) + f64::from(values[mid])) / 2.0)
    } else {
        Some(f64::from(values[mid]))
    }
}

fn color_stddev(image: &RgbImage, mask: &GrayImage, median: [f64; 3]) -> [f64; 3] {
    let mut sum_sq = [0.0; 3];
    let mut count = 0.0;

    for (pixel, mask_pixel) in image.pixels().zip(mask.pixels()) {
        if mask_pixel.0[0] == 0 {
            continue;
        }
        count += 1.0;
        for channel in 0..3 {
            let diff = f64::from(pixel.0[channel]) - median[channel];
            sum_sq[channel] += diff * diff;
        }
    }

    if count == 0.0 {
        return [f64::INFINITY; 3];
    }

    [
        (sum_sq[0] / count).sqrt(),
        (sum_sq[1] / count).sqrt(),
        (sum_sq[2] / count).sqrt(),
    ]
}

fn stddev3(values: [f64; 3]) -> f64 {
    let mean = values.iter().sum::<f64>() / 3.0;
    let variance = values
        .iter()
        .map(|value| {
            let diff = value - mean;
            diff * diff
        })
        .sum::<f64>()
        / 3.0;
    variance.sqrt()
}

#[cfg(test)]
mod tests {
    use super::{
        BALLOON_WINDOW_ASPECT_RATIO, BALLOON_WINDOW_RATIO, clear_mask_bbox, count_nonzero,
        enlarge_window, extract_balloon_mask, try_fill_balloon,
    };
    use crate::inpainting::restore_alpha_channel;
    use crate::types::TextRegion;
    use image::{GrayImage, Luma, Rgb, RgbImage};
    use imageproc::drawing::draw_hollow_rect_mut;
    use imageproc::rect::Rect;

    const ALPHA_RING_RADIUS: u8 = 7;

    #[test]
    fn enlarge_window_matches_ratio_1_7_reference() {
        let enlarged = enlarge_window(
            [10, 20, 50, 60],
            200,
            150,
            BALLOON_WINDOW_RATIO,
            BALLOON_WINDOW_ASPECT_RATIO,
        );

        assert_eq!(enlarged, [4, 14, 56, 66]);
    }

    #[test]
    fn extract_balloon_mask_prefers_smallest_covering_contour() {
        let mut image = RgbImage::from_pixel(80, 80, Rgb([255, 255, 255]));
        draw_hollow_rect_mut(&mut image, Rect::at(4, 4).of_size(72, 72), Rgb([0, 0, 0]));
        draw_hollow_rect_mut(&mut image, Rect::at(20, 20).of_size(28, 20), Rgb([0, 0, 0]));

        let mut mask = GrayImage::new(80, 80);
        for y in 24..36 {
            for x in 24..44 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        let masks = extract_balloon_mask(&image, &mask).expect("balloon should be detected");
        let balloon_pixels = count_nonzero(&masks.balloon_mask);

        assert!(
            balloon_pixels < 900,
            "expected inner contour fill, got {balloon_pixels}"
        );
        assert!(
            balloon_pixels > 250,
            "expected meaningful bubble area, got {balloon_pixels}"
        );
    }

    #[test]
    fn simple_balloon_chooses_fill_but_textured_balloon_does_not() {
        let mut flat = RgbImage::from_pixel(64, 64, Rgb([240, 240, 240]));
        draw_hollow_rect_mut(&mut flat, Rect::at(8, 8).of_size(48, 32), Rgb([0, 0, 0]));

        let mut mask = GrayImage::new(64, 64);
        for y in 18..30 {
            for x in 18..46 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        assert!(try_fill_balloon(&flat, &mask).is_some());

        let mut textured = flat.clone();
        for y in 9..39 {
            for x in 9..55 {
                let noise = ((x + y) % 23) as u8;
                textured.put_pixel(
                    x,
                    y,
                    Rgb([200 + noise, 210 + (noise / 2), 220 - (noise / 3)]),
                );
            }
        }

        assert!(try_fill_balloon(&textured, &mask).is_none());
    }

    #[test]
    fn clearing_mask_consumes_only_original_bbox() {
        let mut mask = GrayImage::from_pixel(32, 32, Luma([255]));
        clear_mask_bbox(&mut mask, [8, 10, 16, 18]);

        for y in 10..18 {
            for x in 8..16 {
                assert_eq!(mask.get_pixel(x, y).0[0], 0);
            }
        }

        assert_eq!(mask.get_pixel(7, 10).0[0], 255);
        assert_eq!(mask.get_pixel(16, 17).0[0], 255);
        assert_eq!(mask.get_pixel(8, 9).0[0], 255);
        assert_eq!(mask.get_pixel(15, 18).0[0], 255);
    }

    #[test]
    fn rgba_alpha_restore_uses_surrounding_ring() {
        let image = RgbImage::from_pixel(32, 32, Rgb([20, 30, 40]));
        let mut alpha = GrayImage::from_pixel(32, 32, Luma([255]));
        let mut mask = GrayImage::new(32, 32);

        for y in 10..22 {
            for x in 10..22 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        for y in (10 - u32::from(ALPHA_RING_RADIUS))..(22 + u32::from(ALPHA_RING_RADIUS)) {
            for x in (10 - u32::from(ALPHA_RING_RADIUS))..(22 + u32::from(ALPHA_RING_RADIUS)) {
                if x < 32 && y < 32 && mask.get_pixel(x, y).0[0] == 0 {
                    alpha.put_pixel(x, y, Luma([64]));
                }
            }
        }

        let restored = restore_alpha_channel(&image, &alpha, &mask);
        assert_eq!(restored.get_pixel(15, 15).0[3], 64);
        assert_eq!(restored.get_pixel(2, 2).0[3], 255);
    }

    #[test]
    fn block_xyxy_rounds_and_clamps_document_coords() {
        let block = TextRegion {
            x: 10.2,
            y: 20.7,
            width: 15.1,
            height: 9.4,
            ..Default::default()
        };

        let bbox = super::block_xyxy(&block, 100, 100).expect("bbox");
        assert_eq!(bbox, [10, 20, 26, 31]);
    }
}
