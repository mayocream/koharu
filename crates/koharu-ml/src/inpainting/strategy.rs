//! HD-strategy dispatcher for erase models.
//!
//! Mirrors IOPaint's `InpaintModel.__call__` (`iopaint/model/base.py`): one
//! entry point chooses between Original / Resize / Crop based on image size,
//! then delegates the raw forward to a model-specific [`InpaintForward`].
//!
//! ## Strategies
//!
//! - **Original** — pad to `pad_mod`, forward, unpad. Highest VRAM.
//! - **Resize** — downscale so `max(h,w) <= resize_limit`, pad, forward, unpad,
//!   upscale, then restore pixels outside the mask from the original. Medium
//!   VRAM, preserves quality outside the mask.
//! - **Crop** — extract one bounding box per connected mask contour, expand by
//!   `crop_margin` on each side, forward each crop independently, paste back.
//!   Lowest VRAM. Default for manga (many small speech bubbles).
//!
//! The Crop path uses [`pad_forward_bounded`] per crop, so an oversized crop
//! (e.g. a brush stroke covering most of a page) falls back to the Resize path
//! inside that single crop. Callers may optionally provide pre-planned crop
//! windows to override the default contour boxes. No `HdStrategy` ever OOMs on
//! a reasonable GPU provided `resize_limit` is within VRAM budget.
//!
//! Mask boxes come from `imageproc::contours::find_contours` on the binarized
//! mask — equivalent to OpenCV's `cv2.findContours(RETR_EXTERNAL)` that IOPaint
//! uses. Only `BorderType::Outer` contours become boxes (holes are ignored).

use anyhow::Result;
use image::{
    GrayImage, RgbImage,
    imageops::{FilterType, crop_imm, resize},
};
use imageproc::contours::{BorderType, find_contours};

/// Which preprocessing strategy to apply before the raw forward. See the
/// module docs for the semantics of each variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HdStrategy {
    Original,
    Resize,
    Crop,
}

/// Tunable knobs for [`run_inpaint`]. Defaults match IOPaint
/// (`iopaint/schema.py` — trigger 800, margin 128, resize limit 1280).
#[derive(Debug, Clone, Copy)]
pub struct HdStrategyConfig {
    pub strategy: HdStrategy,
    /// Crop strategy only activates when `max(image.w, image.h) >
    /// crop_trigger_size`. Smaller images fall through to Original.
    pub crop_trigger_size: u32,
    /// Additive margin (pixels) added to each side of a mask bounding box when
    /// cropping. Controls how much context the model sees around the mask.
    pub crop_margin: u32,
    /// Hard ceiling on the forward's longer side. Applied by Resize strategy at
    /// the top level, and as a nested fallback inside oversized crops.
    pub resize_limit: u32,
    /// Model-required spatial divisor. LaMa / AoT both need 8; larger for
    /// models with deeper downsampling.
    pub pad_mod: u32,
}

impl HdStrategyConfig {
    /// Manga-tuned default for Lama: Crop strategy with IOPaint's defaults.
    /// Many small speech bubbles → many small per-bubble crops → trivial VRAM.
    pub const fn lama_default() -> Self {
        Self {
            strategy: HdStrategy::Crop,
            crop_trigger_size: 800,
            crop_margin: 128,
            resize_limit: 1280,
            pad_mod: 8,
        }
    }

    /// Default for AoT: whole-image Resize with a fixed upper bound (AoT's
    /// upstream config calls this `default_max_side`).
    pub const fn aot_default(resize_limit: u32, pad_mod: u32) -> Self {
        Self {
            strategy: HdStrategy::Resize,
            crop_trigger_size: 800,
            crop_margin: 128,
            resize_limit,
            pad_mod,
        }
    }
}

/// `[x1, y1, x2, y2]` half-open rectangle: `x1,y1` inclusive, `x2,y2` exclusive.
pub type Xyxy = [u32; 4];

/// A raw forward pass on a (padded) image + mask, returning an image of the
/// same spatial size. Implementors are free to apply fast paths (e.g. Lama's
/// balloon-fill shortcut) before the model forward.
pub trait InpaintForward {
    fn forward(
        &self,
        image: &RgbImage,
        mask: &GrayImage,
        bubble_mask: Option<&GrayImage>,
    ) -> Result<RgbImage>;
}

/// Entry point: dispatch on `cfg.strategy` and return an RGB image with the
/// masked region inpainted. `mask` must already be binarized (0 or 255).
pub fn run_inpaint<F: InpaintForward>(
    model: &F,
    image: &RgbImage,
    mask: &GrayImage,
    bubble_mask: Option<&GrayImage>,
    cfg: &HdStrategyConfig,
) -> Result<RgbImage> {
    run_inpaint_with_windows(model, image, mask, bubble_mask, cfg, None)
}

/// Variant of [`run_inpaint`] that accepts caller-supplied crop windows for
/// the Crop strategy. Each window is an image-space `[x1, y1, x2, y2]`
/// rectangle. When omitted, crop mode falls back to contour boxes from the
/// mask itself.
pub fn run_inpaint_with_windows<F: InpaintForward>(
    model: &F,
    image: &RgbImage,
    mask: &GrayImage,
    bubble_mask: Option<&GrayImage>,
    cfg: &HdStrategyConfig,
    crop_windows: Option<&[Xyxy]>,
) -> Result<RgbImage> {
    assert_eq!(image.dimensions(), mask.dimensions());
    if let Some(bubble_mask) = bubble_mask {
        assert_eq!(image.dimensions(), bubble_mask.dimensions());
    }
    let max_side = image.width().max(image.height());

    match cfg.strategy {
        HdStrategy::Crop if max_side > cfg.crop_trigger_size => {
            run_crop(model, image, mask, bubble_mask, cfg, crop_windows)
        }
        HdStrategy::Resize if max_side > cfg.resize_limit => {
            run_resize(model, image, mask, bubble_mask, cfg)
        }
        _ => pad_forward(model, image, mask, bubble_mask, cfg.pad_mod),
    }
}

fn run_crop<F: InpaintForward>(
    model: &F,
    image: &RgbImage,
    mask: &GrayImage,
    bubble_mask: Option<&GrayImage>,
    cfg: &HdStrategyConfig,
    crop_windows: Option<&[Xyxy]>,
) -> Result<RgbImage> {
    let mut out = image.clone();
    let mut working_mask = mask.clone();

    if let Some(windows) = crop_windows.filter(|windows| !windows.is_empty()) {
        tracing::debug!(
            count = windows.len(),
            "inpaint crop strategy: caller-supplied crop windows"
        );

        for &window in windows {
            let Some((crop_img, crop_mask, [l, t, _r, _bt])) =
                crop_window(&out, &working_mask, window)
            else {
                continue;
            };
            let crop_bubble = bubble_mask.map(|bubble_mask| {
                crop_imm(bubble_mask, l, t, crop_mask.width(), crop_mask.height()).to_image()
            });
            if !mask_has_nonzero(&crop_mask) {
                continue;
            }
            let crop_result =
                pad_forward_bounded(model, &crop_img, &crop_mask, crop_bubble.as_ref(), cfg)?;
            composite_masked(&mut out, &crop_result, &crop_mask, l, t);
            clear_masked_region(&mut working_mask, &crop_mask, l, t);
        }
        return Ok(out);
    }

    let boxes = boxes_from_mask(mask);
    if boxes.is_empty() {
        return Ok(image.clone());
    }

    tracing::debug!(
        count = boxes.len(),
        "inpaint crop strategy: one forward per mask contour"
    );

    for b in boxes {
        let (crop_img, crop_mask, [l, t, _r, _bt]) =
            crop_box(&out, &working_mask, b, cfg.crop_margin);
        let crop_bubble = bubble_mask.map(|bubble_mask| {
            crop_imm(bubble_mask, l, t, crop_mask.width(), crop_mask.height()).to_image()
        });
        if !mask_has_nonzero(&crop_mask) {
            continue;
        }
        let crop_result =
            pad_forward_bounded(model, &crop_img, &crop_mask, crop_bubble.as_ref(), cfg)?;
        composite_masked(&mut out, &crop_result, &crop_mask, l, t);
        clear_masked_region(&mut working_mask, &crop_mask, l, t);
    }
    Ok(out)
}

fn run_resize<F: InpaintForward>(
    model: &F,
    image: &RgbImage,
    mask: &GrayImage,
    bubble_mask: Option<&GrayImage>,
    cfg: &HdStrategyConfig,
) -> Result<RgbImage> {
    let (w, h) = image.dimensions();
    let (nw, nh) = scaled_dims(w, h, cfg.resize_limit);
    tracing::debug!(
        from_w = w,
        from_h = h,
        to_w = nw,
        to_h = nh,
        "inpaint resize strategy"
    );

    let small_img = resize(image, nw, nh, FilterType::Triangle);
    let small_mask = rebinarize(&resize(mask, nw, nh, FilterType::Triangle));
    let small_bubble =
        bubble_mask.map(|bubble_mask| resize(bubble_mask, nw, nh, FilterType::Nearest));

    let small_out = pad_forward(
        model,
        &small_img,
        &small_mask,
        small_bubble.as_ref(),
        cfg.pad_mod,
    )?;
    let full_out = resize(&small_out, w, h, FilterType::CatmullRom);

    // Restore untouched pixels from the original so Resize only loses quality
    // where we actually inpainted. Matches IOPaint's
    // `original_pixel_indices = mask < 127`.
    let mut out = full_out;
    for y in 0..h {
        for x in 0..w {
            if mask.get_pixel(x, y).0[0] < 127 {
                out.put_pixel(x, y, *image.get_pixel(x, y));
            }
        }
    }
    Ok(out)
}

/// `pad_forward` with a nested Resize fallback when the input exceeds
/// `resize_limit`. Used inside the Crop loop so oversized crops don't OOM.
fn pad_forward_bounded<F: InpaintForward>(
    model: &F,
    image: &RgbImage,
    mask: &GrayImage,
    bubble_mask: Option<&GrayImage>,
    cfg: &HdStrategyConfig,
) -> Result<RgbImage> {
    if image.width().max(image.height()) > cfg.resize_limit {
        run_resize(model, image, mask, bubble_mask, cfg)
    } else {
        pad_forward(model, image, mask, bubble_mask, cfg.pad_mod)
    }
}

/// Pad both tensors to `pad_mod` on right/bottom with symmetric reflection,
/// forward through the model, then crop the output back to the input size.
/// Matches IOPaint's `_pad_forward` / `pad_img_to_modulo`.
fn pad_forward<F: InpaintForward>(
    model: &F,
    image: &RgbImage,
    mask: &GrayImage,
    bubble_mask: Option<&GrayImage>,
    pad_mod: u32,
) -> Result<RgbImage> {
    let (w, h) = image.dimensions();
    let pad_w = ceil_multiple(w, pad_mod);
    let pad_h = ceil_multiple(h, pad_mod);

    let out = if pad_w == w && pad_h == h {
        model.forward(image, mask, bubble_mask)?
    } else {
        let pad_img = symmetric_pad_rgb(image, pad_w, pad_h);
        let pad_msk = symmetric_pad_gray(mask, pad_w, pad_h);
        let pad_bubble = bubble_mask.map(|bubble_mask| zero_pad_gray(bubble_mask, pad_w, pad_h));
        let padded_out = model.forward(&pad_img, &pad_msk, pad_bubble.as_ref())?;
        crop_imm(&padded_out, 0, 0, w, h).to_image()
    };
    Ok(out)
}

/// External-contour bounding boxes of a binarized mask. Equivalent to
/// IOPaint's `boxes_from_mask` (`cv2.findContours(RETR_EXTERNAL)` +
/// `cv2.boundingRect`). Hole borders are discarded.
pub fn boxes_from_mask(mask: &GrayImage) -> Vec<Xyxy> {
    let contours = find_contours::<i32>(mask);
    let (mw, mh) = mask.dimensions();
    let mut boxes = Vec::new();
    for contour in contours {
        if contour.border_type != BorderType::Outer || contour.points.is_empty() {
            continue;
        }
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        for p in &contour.points {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        let x1 = (min_x.max(0) as u32).min(mw);
        let y1 = (min_y.max(0) as u32).min(mh);
        let x2 = (max_x.saturating_add(1).max(0) as u32).min(mw);
        let y2 = (max_y.saturating_add(1).max(0) as u32).min(mh);
        if x2 > x1 && y2 > y1 {
            boxes.push([x1, y1, x2, y2]);
        }
    }
    boxes
}

/// Expand `box_xyxy` by `margin` pixels on each side, clamped to the image.
/// When the expanded rect would overflow one edge, shift inward so the full
/// `(box + margin*2)` footprint still fits when possible — matches IOPaint's
/// `_crop_box` (`iopaint/model/base.py`).
pub fn crop_box(
    image: &RgbImage,
    mask: &GrayImage,
    box_xyxy: Xyxy,
    margin: u32,
) -> (RgbImage, GrayImage, Xyxy) {
    let [bx1, by1, bx2, by2] = box_xyxy;
    let (img_w, img_h) = image.dimensions();
    let cx = (bx1 + bx2) / 2;
    let cy = (by1 + by2) / 2;
    let want_w = (bx2 - bx1) + margin * 2;
    let want_h = (by2 - by1) + margin * 2;
    let half_w = want_w / 2;
    let half_h = want_h / 2;

    // Signed desired bounds before clamping (i64 to preserve negatives).
    let desire_l = cx as i64 - half_w as i64;
    let desire_r = cx as i64 + half_w as i64;
    let desire_t = cy as i64 - half_h as i64;
    let desire_b = cy as i64 + half_h as i64;

    let img_w_i = img_w as i64;
    let img_h_i = img_h as i64;

    let mut l = desire_l.max(0);
    let mut r = desire_r.min(img_w_i);
    let mut t = desire_t.max(0);
    let mut b = desire_b.min(img_h_i);

    if desire_l < 0 {
        r = (r - desire_l).min(img_w_i);
    }
    if desire_r > img_w_i {
        l = (l - (desire_r - img_w_i)).max(0);
    }
    if desire_t < 0 {
        b = (b - desire_t).min(img_h_i);
    }
    if desire_b > img_h_i {
        t = (t - (desire_b - img_h_i)).max(0);
    }

    let l = l.clamp(0, img_w_i) as u32;
    let r = r.clamp(0, img_w_i) as u32;
    let t = t.clamp(0, img_h_i) as u32;
    let b = b.clamp(0, img_h_i) as u32;
    let r = r.max(l + 1).min(img_w);
    let b = b.max(t + 1).min(img_h);

    let cw = r - l;
    let ch = b - t;
    let crop_img = crop_imm(image, l, t, cw, ch).to_image();
    let crop_mask = crop_imm(mask, l, t, cw, ch).to_image();
    (crop_img, crop_mask, [l, t, r, b])
}

fn crop_window(
    image: &RgbImage,
    mask: &GrayImage,
    window: Xyxy,
) -> Option<(RgbImage, GrayImage, Xyxy)> {
    let [l, t, r, b] = clamp_xyxy(window, image.width(), image.height())?;
    let cw = r - l;
    let ch = b - t;
    let crop_img = crop_imm(image, l, t, cw, ch).to_image();
    let crop_mask = crop_imm(mask, l, t, cw, ch).to_image();
    Some((crop_img, crop_mask, [l, t, r, b]))
}

/// Scale `(w, h)` so `max(w, h) == max_side`, preserving aspect ratio. No-op
/// when the image already fits. Mirrors IOPaint's `resize_max_size`.
pub fn scaled_dims(w: u32, h: u32, max_side: u32) -> (u32, u32) {
    let longer = w.max(h);
    if longer <= max_side {
        return (w, h);
    }
    let ratio = f64::from(max_side) / f64::from(longer);
    let nw = ((f64::from(w) * ratio).round() as u32).max(1);
    let nh = ((f64::from(h) * ratio).round() as u32).max(1);
    (nw, nh)
}

fn ceil_multiple(v: u32, m: u32) -> u32 {
    if m == 0 {
        return v;
    }
    let r = v % m;
    if r == 0 { v } else { v + (m - r) }
}

fn rebinarize(mask: &GrayImage) -> GrayImage {
    let mut out = mask.clone();
    for p in out.pixels_mut() {
        p.0[0] = if p.0[0] > 127 { 255 } else { 0 };
    }
    out
}

fn clamp_xyxy([x1, y1, x2, y2]: Xyxy, width: u32, height: u32) -> Option<Xyxy> {
    let x1 = x1.min(width);
    let y1 = y1.min(height);
    let x2 = x2.min(width);
    let y2 = y2.min(height);
    if x2 <= x1 || y2 <= y1 {
        return None;
    }
    Some([x1, y1, x2, y2])
}

fn mask_has_nonzero(mask: &GrayImage) -> bool {
    mask.pixels().any(|pixel| pixel.0[0] > 0)
}

fn composite_masked(
    out: &mut RgbImage,
    crop_result: &RgbImage,
    crop_mask: &GrayImage,
    left: u32,
    top: u32,
) {
    for y in 0..crop_mask.height() {
        for x in 0..crop_mask.width() {
            if crop_mask.get_pixel(x, y).0[0] == 0 {
                continue;
            }
            out.put_pixel(left + x, top + y, *crop_result.get_pixel(x, y));
        }
    }
}

fn clear_masked_region(mask: &mut GrayImage, crop_mask: &GrayImage, left: u32, top: u32) {
    for y in 0..crop_mask.height() {
        for x in 0..crop_mask.width() {
            if crop_mask.get_pixel(x, y).0[0] == 0 {
                continue;
            }
            mask.put_pixel(left + x, top + y, image::Luma([0]));
        }
    }
}

/// Numpy-style `mode="symmetric"` padding, but only on the right/bottom edges
/// (we only ever pad up to `pad_mod - 1` pixels to reach a modulo boundary).
fn symmetric_pad_rgb(img: &RgbImage, new_w: u32, new_h: u32) -> RgbImage {
    let (w, h) = img.dimensions();
    if new_w == w && new_h == h {
        return img.clone();
    }
    let mut out = RgbImage::new(new_w, new_h);
    for y in 0..new_h {
        let sy = reflect_index(y, h);
        for x in 0..new_w {
            let sx = reflect_index(x, w);
            out.put_pixel(x, y, *img.get_pixel(sx, sy));
        }
    }
    out
}

fn symmetric_pad_gray(img: &GrayImage, new_w: u32, new_h: u32) -> GrayImage {
    let (w, h) = img.dimensions();
    if new_w == w && new_h == h {
        return img.clone();
    }
    let mut out = GrayImage::new(new_w, new_h);
    for y in 0..new_h {
        let sy = reflect_index(y, h);
        for x in 0..new_w {
            let sx = reflect_index(x, w);
            out.put_pixel(x, y, *img.get_pixel(sx, sy));
        }
    }
    out
}

fn zero_pad_gray(img: &GrayImage, new_w: u32, new_h: u32) -> GrayImage {
    let (w, h) = img.dimensions();
    if new_w == w && new_h == h {
        return img.clone();
    }
    let mut out = GrayImage::new(new_w, new_h);
    for y in 0..h {
        for x in 0..w {
            out.put_pixel(x, y, *img.get_pixel(x, y));
        }
    }
    out
}

/// Reflect index for symmetric padding: `[0..len-1]` maps to itself, `[len..]`
/// reflects. Padding is always less than `len` for our use (right/bottom only,
/// by `pad_mod - 1` pixels max).
fn reflect_index(i: u32, len: u32) -> u32 {
    if len == 0 {
        return 0;
    }
    if i < len {
        return i;
    }
    let past = i - len;
    if past < len {
        len - 1 - past
    } else {
        past % len
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    use image::{Luma, Rgb};

    fn solid_rgb(w: u32, h: u32, rgb: [u8; 3]) -> RgbImage {
        RgbImage::from_pixel(w, h, Rgb(rgb))
    }

    struct IdentityForward;
    impl InpaintForward for IdentityForward {
        fn forward(
            &self,
            image: &RgbImage,
            _mask: &GrayImage,
            _bubble_mask: Option<&GrayImage>,
        ) -> Result<RgbImage> {
            Ok(image.clone())
        }
    }

    struct PaintForward {
        calls: Cell<u32>,
        color: [u8; 3],
    }

    impl PaintForward {
        fn new(color: [u8; 3]) -> Self {
            Self {
                calls: Cell::new(0),
                color,
            }
        }
    }

    impl InpaintForward for PaintForward {
        fn forward(
            &self,
            image: &RgbImage,
            _mask: &GrayImage,
            _bubble_mask: Option<&GrayImage>,
        ) -> Result<RgbImage> {
            self.calls.set(self.calls.get() + 1);
            Ok(RgbImage::from_pixel(
                image.width(),
                image.height(),
                Rgb(self.color),
            ))
        }
    }

    #[test]
    fn ceil_multiple_rounds_up() {
        assert_eq!(ceil_multiple(8, 8), 8);
        assert_eq!(ceil_multiple(9, 8), 16);
        assert_eq!(ceil_multiple(0, 8), 0);
    }

    #[test]
    fn reflect_index_mirrors_beyond_boundary() {
        // len=5 → symmetric pads: [..., 2, 1, 0, 1, 2, 3, 4, 4, 3, 2, ...]
        // but our padding is right-side only so we only care about i >= len:
        assert_eq!(reflect_index(0, 5), 0);
        assert_eq!(reflect_index(4, 5), 4);
        assert_eq!(reflect_index(5, 5), 4);
        assert_eq!(reflect_index(6, 5), 3);
        assert_eq!(reflect_index(9, 5), 0);
    }

    #[test]
    fn scaled_dims_preserves_aspect() {
        assert_eq!(scaled_dims(1600, 900, 1280), (1280, 720));
        assert_eq!(scaled_dims(800, 600, 1280), (800, 600));
        assert_eq!(scaled_dims(1000, 2000, 1280), (640, 1280));
    }

    #[test]
    fn boxes_from_mask_finds_each_contour() {
        let mut mask = GrayImage::new(100, 100);
        for y in 10..20 {
            for x in 10..25 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        for y in 50..60 {
            for x in 70..80 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        let boxes = boxes_from_mask(&mask);
        assert_eq!(boxes.len(), 2);
        let mut sorted = boxes;
        sorted.sort_by_key(|b| b[0]);
        assert_eq!(sorted[0], [10, 10, 25, 20]);
        assert_eq!(sorted[1], [70, 50, 80, 60]);
    }

    #[test]
    fn boxes_from_mask_ignores_holes() {
        // Filled rectangle with a hole in the middle.
        let mut mask = GrayImage::new(50, 50);
        for y in 5..45 {
            for x in 5..45 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        for y in 20..30 {
            for x in 20..30 {
                mask.put_pixel(x, y, Luma([0]));
            }
        }
        let boxes = boxes_from_mask(&mask);
        assert_eq!(boxes.len(), 1, "hole must not produce a second box");
    }

    #[test]
    fn crop_box_expands_by_margin_additively() {
        let img = solid_rgb(200, 200, [255, 255, 255]);
        let mask = GrayImage::new(200, 200);
        let (ci, _cm, [l, t, r, b]) = crop_box(&img, &mask, [80, 80, 120, 120], 20);
        assert_eq!([l, t, r, b], [60, 60, 140, 140]);
        assert_eq!(ci.dimensions(), (80, 80));
    }

    #[test]
    fn crop_box_shifts_inward_at_edges() {
        let img = solid_rgb(100, 100, [255, 255, 255]);
        let mask = GrayImage::new(100, 100);
        // Box hugging the left edge — desired crop starts at -10, so we shift
        // the right edge outward to keep the full (box + margin*2) width.
        let (_ci, _cm, [l, t, r, b]) = crop_box(&img, &mask, [0, 40, 20, 60], 10);
        assert_eq!(l, 0);
        assert_eq!(r, 40);
        assert_eq!(t, 30);
        assert_eq!(b, 70);
    }

    #[test]
    fn crop_strategy_skips_when_mask_empty() {
        let img = solid_rgb(900, 900, [50, 60, 70]);
        let mask = GrayImage::new(900, 900);
        let cfg = HdStrategyConfig::lama_default();
        let out = run_inpaint(&IdentityForward, &img, &mask, None, &cfg).unwrap();
        assert_eq!(out.get_pixel(0, 0).0, [50, 60, 70]);
    }

    #[test]
    fn resize_strategy_restores_unmasked_pixels() {
        // Small image → even under Resize, unmasked pixels must be identical.
        let mut img = solid_rgb(1600, 1200, [10, 20, 30]);
        // One pixel in the masked area, different value.
        img.put_pixel(500, 500, Rgb([200, 200, 200]));
        let mut mask = GrayImage::new(1600, 1200);
        mask.put_pixel(500, 500, Luma([255]));

        let cfg = HdStrategyConfig {
            strategy: HdStrategy::Resize,
            resize_limit: 640,
            ..HdStrategyConfig::lama_default()
        };
        let out = run_inpaint(&IdentityForward, &img, &mask, None, &cfg).unwrap();
        assert_eq!(out.get_pixel(0, 0).0, [10, 20, 30]);
        assert_eq!(out.get_pixel(1599, 1199).0, [10, 20, 30]);
    }

    #[test]
    fn crop_strategy_paste_bounds() {
        // Two masked blobs → two crops → full image untouched outside crops.
        let img = solid_rgb(1200, 1200, [100, 100, 100]);
        let mut mask = GrayImage::new(1200, 1200);
        for y in 100..120 {
            for x in 100..120 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        for y in 900..920 {
            for x in 900..920 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        let cfg = HdStrategyConfig::lama_default();
        let out = run_inpaint(&IdentityForward, &img, &mask, None, &cfg).unwrap();
        // IdentityForward is a no-op, so output == input everywhere.
        assert_eq!(out.get_pixel(0, 0).0, [100, 100, 100]);
        assert_eq!(out.get_pixel(500, 500).0, [100, 100, 100]);
    }

    #[test]
    fn crop_strategy_only_updates_masked_pixels() {
        let img = solid_rgb(900, 900, [12, 34, 56]);
        let mut mask = GrayImage::new(900, 900);
        for y in 100..120 {
            for x in 100..120 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        let forward = PaintForward::new([250, 10, 10]);
        let out = run_inpaint(
            &forward,
            &img,
            &mask,
            None,
            &HdStrategyConfig::lama_default(),
        )
        .unwrap();

        assert_eq!(forward.calls.get(), 1);
        assert_eq!(out.get_pixel(110, 110).0, [250, 10, 10]);
        assert_eq!(out.get_pixel(90, 90).0, [12, 34, 56]);
        assert_eq!(out.get_pixel(200, 200).0, [12, 34, 56]);
    }

    #[test]
    fn supplied_crop_windows_reduce_forward_count() {
        let img = solid_rgb(1200, 1200, [90, 90, 90]);
        let mut mask = GrayImage::new(1200, 1200);
        for y in 100..120 {
            for x in 100..120 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        for y in 150..170 {
            for x in 160..180 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        let forward = PaintForward::new([5, 6, 7]);
        let out = run_inpaint_with_windows(
            &forward,
            &img,
            &mask,
            None,
            &HdStrategyConfig::lama_default(),
            Some(&[[80, 80, 220, 220]]),
        )
        .unwrap();

        assert_eq!(forward.calls.get(), 1);
        assert_eq!(out.get_pixel(110, 110).0, [5, 6, 7]);
        assert_eq!(out.get_pixel(170, 160).0, [5, 6, 7]);
        assert_eq!(out.get_pixel(20, 20).0, [90, 90, 90]);
    }
}
