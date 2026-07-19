//! Bubble-mask indexing and safe text layout bounds.
//!
//! [`BubbleIndex`] maps a text frame to the safest rectangular area inside a
//! labeled bubble mask. Zero is background and every non-zero value identifies
//! one bubble.

use image::{GrayImage, Luma};
use imageproc::distance_transform::{Norm, distance_transform};

use crate::layout::WritingMode;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BubbleMatch {
    pub id: u8,
    pub layout_box: LayoutBox,
}

/// Fraction of the bubble's short side reserved as distance-from-edge
/// padding before choosing an interior safe area. This preserves the
/// bounding-box inset proportions for rectangular bubbles while letting irregular
/// masks steer text away from tails, notches, and thin connectors.
const SAFE_PADDING_FRAC_HORIZONTAL: f32 = 0.12;
/// Vertical CJK columns need more margin so neighboring columns do not crowd
/// the balloon outline.
const SAFE_PADDING_FRAC_VERTICAL: f32 = 0.20;

fn safe_padding_fraction(writing_mode: WritingMode) -> f32 {
    match writing_mode {
        WritingMode::Horizontal => SAFE_PADDING_FRAC_HORIZONTAL,
        WritingMode::VerticalRl | WritingMode::VerticalLr => SAFE_PADDING_FRAC_VERTICAL,
    }
}

#[derive(Clone, Copy, Debug)]
struct BubbleGeometry {
    horizontal_safe: LayoutBox,
    vertical_safe: LayoutBox,
}

/// Pre-built index over a bubble-segmentation mask.
///
/// The mask encodes one bubble per non-zero grayscale value (IDs 1..=N).
/// This index scans the mask once to extract each ID's bounding box, so
/// repeated [`BubbleIndex::lookup`] calls on successive text seeds cost
/// only an O(seed_bbox_area) pixel-count to find the seed's majority ID.
pub struct BubbleIndex {
    mask: GrayImage,
    bubbles: [Option<BubbleGeometry>; 256],
}

impl BubbleIndex {
    /// Build the index. `mask` must have each detected bubble painted
    /// with a unique non-zero u8 ID (0 = outside any bubble).
    #[must_use]
    pub fn new(mask: GrayImage) -> Self {
        let w = mask.width();
        let h = mask.height();
        // Accumulator: id → (min_x, min_y, max_x, max_y).
        let mut extents = [None::<[i32; 4]>; 256];
        for y in 0..h {
            for x in 0..w {
                let id = mask.get_pixel(x, y).0[0];
                if id == 0 {
                    continue;
                }
                let e =
                    extents[id as usize].get_or_insert([x as i32, y as i32, x as i32, y as i32]);
                if (x as i32) < e[0] {
                    e[0] = x as i32;
                }
                if (y as i32) < e[1] {
                    e[1] = y as i32;
                }
                if (x as i32) > e[2] {
                    e[2] = x as i32;
                }
                if (y as i32) > e[3] {
                    e[3] = y as i32;
                }
            }
        }
        let mut bubbles = [None; 256];
        for (id, extent) in extents.into_iter().enumerate().skip(1) {
            if let Some(e) = extent {
                let bbox = LayoutBox {
                    x: e[0] as f32,
                    y: e[1] as f32,
                    width: (e[2] - e[0] + 1) as f32,
                    height: (e[3] - e[1] + 1) as f32,
                };
                let field = BubbleDistance::new(&mask, id as u8, bbox);
                bubbles[id] = Some(BubbleGeometry {
                    horizontal_safe: safe_layout_box(&field, bbox, WritingMode::Horizontal),
                    vertical_safe: safe_layout_box(&field, bbox, WritingMode::VerticalRl),
                });
            }
        }
        Self { mask, bubbles }
    }

    /// Find the bubble this text seed belongs to and return its layout rect.
    ///
    /// Assignment rule: count pixels under the seed bbox labelled with each
    /// ID; pick the ID with the most coverage. This handles seeds that
    /// slightly overshoot the detected bubble edge, and overlapping bubble
    /// bboxes (a nested small bubble always wins against an enclosing large
    /// one because the small bubble's ID is what overwrites the overlap
    /// region when the mask is painted smaller-last).
    ///
    /// Returns `None` when the seed bbox has zero coverage over any bubble.
    ///
    /// `writing_mode` controls the edge-distance padding used for the safe
    /// area. Vertical layouts reserve more margin so CJK columns are not
    /// crowded against the balloon outline.
    pub fn lookup(&self, seed: LayoutBox, writing_mode: WritingMode) -> Option<LayoutBox> {
        self.lookup_match(seed, writing_mode)
            .map(|matched| matched.layout_box)
    }

    pub fn lookup_match(&self, seed: LayoutBox, writing_mode: WritingMode) -> Option<BubbleMatch> {
        let w = self.mask.width() as i32;
        let h = self.mask.height() as i32;
        if w <= 0 || h <= 0 {
            return None;
        }
        if seed.width <= 0.0
            || seed.height <= 0.0
            || seed.x >= w as f32
            || seed.y >= h as f32
            || seed.x + seed.width <= 0.0
            || seed.y + seed.height <= 0.0
        {
            return None;
        }
        let sx0 = (seed.x.floor() as i32).clamp(0, w - 1);
        let sy0 = (seed.y.floor() as i32).clamp(0, h - 1);
        let sx1 = ((seed.x + seed.width).ceil() as i32).clamp(sx0 + 1, w);
        let sy1 = ((seed.y + seed.height).ceil() as i32).clamp(sy0 + 1, h);

        let mut counts = [0u32; 256];
        for y in sy0..sy1 {
            for x in sx0..sx1 {
                let id = self.mask.get_pixel(x as u32, y as u32).0[0];
                if id == 0 {
                    continue;
                }
                counts[id as usize] += 1;
            }
        }
        let best_id = (1..256).max_by_key(|&id| counts[id])?;
        if counts[best_id] == 0 {
            return None;
        }
        let bubble = self.bubbles[best_id]?;
        let layout_box = match writing_mode {
            WritingMode::Horizontal => bubble.horizontal_safe,
            WritingMode::VerticalRl | WritingMode::VerticalLr => bubble.vertical_safe,
        };
        Some(BubbleMatch {
            id: best_id as u8,
            layout_box,
        })
    }

    pub fn mask(&self) -> &GrayImage {
        &self.mask
    }
}

struct BubbleDistance {
    background: GrayImage,
    distance: GrayImage,
    x0: u32,
    y0: u32,
    width: u32,
    height: u32,
}

impl BubbleDistance {
    fn new(mask: &GrayImage, bubble_id: u8, bbox: LayoutBox) -> Self {
        let x0 = bbox.x.floor().max(0.0) as u32;
        let y0 = bbox.y.floor().max(0.0) as u32;
        let x1 = (bbox.x + bbox.width).ceil().min(mask.width() as f32) as u32;
        let y1 = (bbox.y + bbox.height).ceil().min(mask.height() as f32) as u32;
        let width = x1.saturating_sub(x0);
        let height = y1.saturating_sub(y0);
        let mut background = GrayImage::from_pixel(width + 2, height + 2, Luma([255u8]));
        for y in 0..height {
            for x in 0..width {
                if mask.get_pixel(x0 + x, y0 + y).0[0] == bubble_id {
                    background.put_pixel(x + 1, y + 1, Luma([0u8]));
                }
            }
        }
        let distance = distance_transform(&background, Norm::L2);
        Self {
            background,
            distance,
            x0,
            y0,
            width,
            height,
        }
    }
}

fn safe_layout_box(
    field: &BubbleDistance,
    bbox: LayoutBox,
    writing_mode: WritingMode,
) -> LayoutBox {
    let desired_padding = (bbox.width.min(bbox.height) * safe_padding_fraction(writing_mode))
        .round()
        .max(1.0) as u8;
    for padding in [
        desired_padding,
        desired_padding.saturating_mul(3) / 4,
        desired_padding / 2,
        desired_padding / 4,
        1,
        0,
    ] {
        if let Some(safe) = safe_layout_box_with_padding(field, padding) {
            return safe;
        }
    }
    inset_layout_box(bbox, writing_mode)
}

fn safe_layout_box_with_padding(field: &BubbleDistance, padding: u8) -> Option<LayoutBox> {
    let width = field.width;
    let height = field.height;
    if width == 0 || height == 0 {
        return None;
    }
    let background = &field.background;
    let distance = &field.distance;

    let safe_threshold = padding.max(1);
    let mut count = 0f32;
    let mut sum_x = 0f32;
    let mut sum_y = 0f32;
    let mut max_dist = 0u8;
    let mut max_point = (0u32, 0u32);

    for y in 0..height {
        for x in 0..width {
            let lx = x + 1;
            let ly = y + 1;
            if background.get_pixel(lx, ly).0[0] != 0 {
                continue;
            }
            let dist = distance.get_pixel(lx, ly).0[0];
            if dist > max_dist {
                max_dist = dist;
                max_point = (x, y);
            }
            if dist >= safe_threshold {
                count += 1.0;
                sum_x += x as f32;
                sum_y += y as f32;
            }
        }
    }

    if count == 0.0 {
        return None;
    }

    let mut cx = (sum_x / count)
        .round()
        .clamp(0.0, width.saturating_sub(1) as f32) as u32;
    let mut cy = (sum_y / count)
        .round()
        .clamp(0.0, height.saturating_sub(1) as f32) as u32;
    let centroid_dist = distance.get_pixel(cx + 1, cy + 1).0[0];
    if max_dist > 0 && (centroid_dist as f32) < (max_dist as f32 * 0.70) {
        (cx, cy) = max_point;
    }
    if !is_safe_pixel(background, distance, cx, cy, safe_threshold) {
        (cx, cy) = nearest_safe_pixel(background, distance, cx, cy, safe_threshold)?;
    }

    let safe = build_safe_map(background, distance, width, height, safe_threshold);
    let largest = largest_safe_rectangle(&safe, width, height, (cx, cy))?;
    let mut selected = largest;

    // A second inscribed rectangle inside the padded pixels wastes almost half of an oval
    // balloon. For dense convex bodies, use a lightly inset bounding box of the safe pixels.
    // Thin tails disappear at the distance threshold, while sparse concave masks retain the
    // conservative all-safe rectangle above.
    let mut safe_bounds = None::<(u32, u32, u32, u32)>;
    let mut safe_count = 0u64;
    for y in 0..height {
        for x in 0..width {
            if !safe[y as usize * width as usize + x as usize] {
                continue;
            }
            safe_count += 1;
            let bounds = safe_bounds.get_or_insert((x, y, x + 1, y + 1));
            bounds.0 = bounds.0.min(x);
            bounds.1 = bounds.1.min(y);
            bounds.2 = bounds.2.max(x + 1);
            bounds.3 = bounds.3.max(y + 1);
        }
    }
    if let Some((left, top, right, bottom)) = safe_bounds {
        let bounds_width = right - left;
        let bounds_height = bottom - top;
        let bounds_area = u64::from(bounds_width) * u64::from(bounds_height);
        if bounds_area > 0 && safe_count as f32 / bounds_area as f32 >= 0.76 {
            let inset_x = ((bounds_width as f32) * 0.08).ceil() as u32;
            let inset_y = ((bounds_height as f32) * 0.08).ceil() as u32;
            let candidate = (
                left + inset_x,
                top + inset_y,
                right.saturating_sub(inset_x),
                bottom.saturating_sub(inset_y),
            );
            let candidate_area = u64::from(candidate.2.saturating_sub(candidate.0))
                * u64::from(candidate.3.saturating_sub(candidate.1));
            let largest_area = u64::from(largest.2 - largest.0) * u64::from(largest.3 - largest.1);
            if candidate.0 < candidate.2
                && candidate.1 < candidate.3
                && candidate_area > largest_area
            {
                selected = candidate;
            }
        }
    }
    let (left, top, right, bottom) = selected;

    Some(LayoutBox {
        x: field.x0 as f32 + left as f32,
        y: field.y0 as f32 + top as f32,
        width: (right - left) as f32,
        height: (bottom - top) as f32,
    })
}

fn build_safe_map(
    background: &GrayImage,
    distance: &GrayImage,
    width: u32,
    height: u32,
    threshold: u8,
) -> Vec<bool> {
    let mut safe = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height {
        for x in 0..width {
            safe.push(is_safe_pixel(background, distance, x, y, threshold));
        }
    }
    safe
}

fn largest_safe_rectangle(
    safe: &[bool],
    width: u32,
    height: u32,
    anchor: (u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    let width = width as usize;
    if width == 0 || height == 0 || safe.len() != width * height as usize {
        return None;
    }

    let mut heights = vec![0u32; width];
    let mut best: Option<(u64, u64, u32, u32, u32, u32)> = None;
    for y in 0..height {
        let row_start = y as usize * width;
        for x in 0..width {
            if safe[row_start + x] {
                heights[x] += 1;
            } else {
                heights[x] = 0;
            }
        }

        let mut stack: Vec<usize> = Vec::with_capacity(width);
        for x in 0..=width {
            let current = if x == width { 0 } else { heights[x] };
            while let Some(&last) = stack.last() {
                if heights[last] <= current {
                    break;
                }
                let bar = stack.pop().expect("stack is non-empty");
                let rect_height = heights[bar];
                if rect_height == 0 {
                    continue;
                }

                let left = stack.last().map_or(0, |&prev| prev + 1);
                let right = x;
                let rect_width = right - left;
                if rect_width == 0 {
                    continue;
                }

                let bottom = y + 1;
                let top = bottom - rect_height;
                let left = left as u32;
                let right = right as u32;
                let area = rect_width as u64 * rect_height as u64;
                let anchor_dist2 = rectangle_anchor_distance2(left, top, right, bottom, anchor);
                if best.is_none_or(|(best_area, best_dist2, _, _, _, _)| {
                    area > best_area || (area == best_area && anchor_dist2 < best_dist2)
                }) {
                    best = Some((area, anchor_dist2, left, top, right, bottom));
                }
            }
            if x < width {
                stack.push(x);
            }
        }
    }

    best.map(|(_, _, left, top, right, bottom)| (left, top, right, bottom))
}

fn rectangle_anchor_distance2(
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
    anchor: (u32, u32),
) -> u64 {
    let rect_cx2 = left as i64 + right as i64;
    let rect_cy2 = top as i64 + bottom as i64;
    let anchor_cx2 = anchor.0 as i64 * 2 + 1;
    let anchor_cy2 = anchor.1 as i64 * 2 + 1;
    let dx = rect_cx2 - anchor_cx2;
    let dy = rect_cy2 - anchor_cy2;
    (dx * dx + dy * dy) as u64
}

fn is_safe_pixel(
    background: &GrayImage,
    distance: &GrayImage,
    x: u32,
    y: u32,
    threshold: u8,
) -> bool {
    let lx = x + 1;
    let ly = y + 1;
    background.get_pixel(lx, ly).0[0] == 0 && distance.get_pixel(lx, ly).0[0] >= threshold
}

fn nearest_safe_pixel(
    background: &GrayImage,
    distance: &GrayImage,
    cx: u32,
    cy: u32,
    threshold: u8,
) -> Option<(u32, u32)> {
    let width = background.width().checked_sub(2)?;
    let height = background.height().checked_sub(2)?;
    let mut best: Option<(u64, u32, u32)> = None;
    for y in 0..height {
        for x in 0..width {
            if !is_safe_pixel(background, distance, x, y, threshold) {
                continue;
            }
            let dx = x.abs_diff(cx) as u64;
            let dy = y.abs_diff(cy) as u64;
            let dist2 = dx * dx + dy * dy;
            if best.is_none_or(|(best_dist2, _, _)| dist2 < best_dist2) {
                best = Some((dist2, x, y));
            }
        }
    }
    best.map(|(_, x, y)| (x, y))
}

fn inset_layout_box(bbox: LayoutBox, writing_mode: WritingMode) -> LayoutBox {
    let frac = safe_padding_fraction(writing_mode);
    let inset_x = bbox.width * frac;
    let inset_y = bbox.height * frac;
    LayoutBox {
        x: bbox.x + inset_x,
        y: bbox.y + inset_y,
        width: (bbox.width - 2.0 * inset_x).max(1.0),
        height: (bbox.height - 2.0 * inset_y).max(1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    fn paint_rect(img: &mut GrayImage, x0: u32, y0: u32, x1: u32, y1: u32, value: u8) {
        for y in y0..y1 {
            for x in x0..x1 {
                img.put_pixel(x, y, Luma([value]));
            }
        }
    }

    fn assert_rect_pixels_match(mask: &GrayImage, rect: LayoutBox, value: u8) {
        let x0 = rect.x.floor().max(0.0) as u32;
        let y0 = rect.y.floor().max(0.0) as u32;
        let x1 = (rect.x + rect.width).ceil().min(mask.width() as f32) as u32;
        let y1 = (rect.y + rect.height).ceil().min(mask.height() as f32) as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                assert_eq!(
                    mask.get_pixel(x, y).0[0],
                    value,
                    "layout rect includes pixel ({x}, {y}) outside the safe bubble body"
                );
            }
        }
    }

    #[test]
    fn lookup_returns_safe_area_for_seed_inside() {
        // One bubble painted at x=20..80, y=30..100 with ID 1.
        let mut mask = GrayImage::from_pixel(200, 200, Luma([0u8]));
        paint_rect(&mut mask, 20, 30, 80, 100, 1);
        let index = BubbleIndex::new(mask);

        let seed = LayoutBox {
            x: 40.0,
            y: 50.0,
            width: 10.0,
            height: 10.0,
        };
        let rect = index
            .lookup(seed, WritingMode::Horizontal)
            .expect("should find bubble");
        // The returned rect stays inside the bubble with edge-distance padding.
        assert!(rect.x >= 20.0);
        assert!(rect.y >= 30.0);
        assert!(rect.x + rect.width <= 80.0);
        assert!(rect.y + rect.height <= 100.0);
        // The safe area reserves a comfortable margin on each side, so the
        // usable area is a meaningful fraction of the bubble but not the whole
        // thing.
        let coverage = (rect.width * rect.height) / ((80.0 - 20.0) * (100.0 - 30.0));
        assert!(coverage > 0.5);
        assert!(coverage < 0.95);
    }

    #[test]
    fn safe_area_ignores_thin_tail_in_bubble_mask() {
        let mut mask = GrayImage::from_pixel(220, 140, Luma([0u8]));
        paint_rect(&mut mask, 20, 20, 120, 100, 1);
        paint_rect(&mut mask, 120, 55, 190, 65, 1);
        let index = BubbleIndex::new(mask);

        let seed = LayoutBox {
            x: 60.0,
            y: 50.0,
            width: 20.0,
            height: 20.0,
        };
        let rect = index
            .lookup(seed, WritingMode::Horizontal)
            .expect("should find bubble");

        assert!(rect.x >= 20.0);
        assert!(rect.y >= 20.0);
        assert!(
            rect.x + rect.width <= 122.0,
            "safe area should stay in the main bubble body, got {rect:?}"
        );
        assert!(rect.y + rect.height <= 100.0);
    }

    #[test]
    fn safe_area_stays_inside_concave_bubble_mask() {
        let mut mask = GrayImage::from_pixel(180, 160, Luma([0u8]));
        paint_rect(&mut mask, 20, 20, 140, 120, 1);
        paint_rect(&mut mask, 80, 20, 140, 80, 0);
        let index = BubbleIndex::new(mask.clone());

        let seed = LayoutBox {
            x: 45.0,
            y: 85.0,
            width: 20.0,
            height: 20.0,
        };
        let rect = index
            .lookup(seed, WritingMode::Horizontal)
            .expect("should find bubble");

        assert_rect_pixels_match(&mask, rect, 1);
        assert!(
            rect.x + rect.width <= 80.0 || rect.y >= 80.0,
            "safe area should not bridge across the missing corner, got {rect:?}"
        );
    }

    #[test]
    fn dense_oval_uses_more_than_the_double_inscribed_rectangle() {
        let mut mask = GrayImage::from_pixel(160, 220, Luma([0u8]));
        let center = (80.0f32, 110.0f32);
        let radii = (50.0f32, 90.0f32);
        for y in 20..200 {
            for x in 30..130 {
                let dx = (x as f32 + 0.5 - center.0) / radii.0;
                let dy = (y as f32 + 0.5 - center.1) / radii.1;
                if dx * dx + dy * dy <= 1.0 {
                    mask.put_pixel(x, y, Luma([1]));
                }
            }
        }
        let index = BubbleIndex::new(mask.clone());
        let rect = index
            .lookup(
                LayoutBox {
                    x: 70.0,
                    y: 100.0,
                    width: 20.0,
                    height: 20.0,
                },
                WritingMode::Horizontal,
            )
            .expect("oval bubble");

        assert!(rect.width >= 60.0, "oval safe width was {rect:?}");
        assert!(rect.height >= 110.0, "oval safe height was {rect:?}");
        assert_rect_pixels_match(&mask, rect, 1);
    }

    #[test]
    fn lookup_picks_seed_majority_id_when_two_bubbles_overlap_seed() {
        // Two bubbles. Seed straddles them but is mostly in bubble 2.
        let mut mask = GrayImage::from_pixel(200, 200, Luma([0u8]));
        paint_rect(&mut mask, 0, 0, 100, 200, 1); // bubble 1 (left half)
        paint_rect(&mut mask, 100, 0, 200, 200, 2); // bubble 2 (right half)
        let index = BubbleIndex::new(mask);

        // Seed mostly in bubble 2 (x ∈ 95..130, 5 px into bubble 1, 30 into bubble 2).
        let seed = LayoutBox {
            x: 95.0,
            y: 90.0,
            width: 35.0,
            height: 20.0,
        };
        let rect = index
            .lookup(seed, WritingMode::Horizontal)
            .expect("should find bubble");
        // Expected: bubble 2's bbox (x ∈ 100..199).
        assert!(rect.x >= 100.0);
    }

    #[test]
    fn lookup_returns_none_when_seed_outside_any_bubble() {
        let mut mask = GrayImage::from_pixel(200, 200, Luma([0u8]));
        paint_rect(&mut mask, 10, 10, 50, 50, 1);
        let index = BubbleIndex::new(mask);

        let seed = LayoutBox {
            x: 120.0,
            y: 120.0,
            width: 20.0,
            height: 20.0,
        };
        assert!(index.lookup(seed, WritingMode::Horizontal).is_none());
    }

    #[test]
    fn lookup_does_not_clamp_an_off_page_seed_onto_a_bubble() {
        let mut mask = GrayImage::from_pixel(40, 40, Luma([0u8]));
        paint_rect(&mut mask, 0, 0, 20, 20, 1);
        let index = BubbleIndex::new(mask);
        let seed = LayoutBox {
            x: -100.0,
            y: -100.0,
            width: 10.0,
            height: 10.0,
        };

        assert!(index.lookup(seed, WritingMode::Horizontal).is_none());
    }

    #[test]
    fn vertical_writing_mode_yields_larger_inset_than_horizontal() {
        let mut mask = GrayImage::from_pixel(200, 200, Luma([0u8]));
        paint_rect(&mut mask, 20, 30, 180, 170, 1);
        let index = BubbleIndex::new(mask);
        let seed = LayoutBox {
            x: 80.0,
            y: 80.0,
            width: 20.0,
            height: 20.0,
        };

        let horizontal = index
            .lookup(seed, WritingMode::Horizontal)
            .expect("horizontal lookup");
        let vertical = index
            .lookup(seed, WritingMode::VerticalRl)
            .expect("vertical lookup");

        // Vertical safe-area padding is strictly greater, so the usable box is
        // smaller on both axes and the top-left corner pushed further inward.
        assert!(vertical.width < horizontal.width);
        assert!(vertical.height < horizontal.height);
        assert!(vertical.x > horizontal.x);
        assert!(vertical.y > horizontal.y);
    }

    #[test]
    fn lookup_match_reports_the_bubble_id() {
        let mut mask = GrayImage::from_pixel(200, 200, Luma([0u8]));
        paint_rect(&mut mask, 10, 10, 90, 90, 1);
        paint_rect(&mut mask, 110, 10, 190, 90, 2);
        let index = BubbleIndex::new(mask);

        let matched = index
            .lookup_match(
                LayoutBox {
                    x: 125.0,
                    y: 25.0,
                    width: 20.0,
                    height: 20.0,
                },
                WritingMode::Horizontal,
            )
            .expect("should find bubble");

        assert_eq!(matched.id, 2);
        assert!(matched.layout_box.x >= 110.0);
        assert!(matched.layout_box.x + matched.layout_box.width <= 190.0);
    }

    #[test]
    fn nested_bubble_wins_over_enclosing_one() {
        // Big bubble (ID 1) with small bubble (ID 2) painted inside it.
        // The engine's paint-smaller-last rule makes this work — we
        // simulate by painting ID 2 on top of ID 1.
        let mut mask = GrayImage::from_pixel(200, 200, Luma([0u8]));
        paint_rect(&mut mask, 10, 10, 190, 190, 1);
        paint_rect(&mut mask, 80, 80, 120, 120, 2);
        let index = BubbleIndex::new(mask);

        // Seed entirely inside the small bubble.
        let seed = LayoutBox {
            x: 90.0,
            y: 90.0,
            width: 20.0,
            height: 20.0,
        };
        let rect = index
            .lookup(seed, WritingMode::Horizontal)
            .expect("should find small bubble");
        // Small bubble bbox is roughly [80, 80] to [120, 120] → the
        // returned rect should be nested inside these bounds, not the
        // enclosing big bubble.
        assert!(rect.x >= 80.0);
        assert!(rect.y >= 80.0);
        assert!(rect.x + rect.width <= 120.0);
        assert!(rect.y + rect.height <= 120.0);
    }
}
