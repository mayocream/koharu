//! Latin text layout helpers.
//!
//! Two pieces of public API:
//! - [`LayoutBox`] + [`layout_box_from_block`]: the axis-aligned box a text
//!   block lays out into. The layout engine treats this as a hard boundary.
//! - [`BubbleIndex`]: given the bubble-segmentation mask (where
//!   each detected bubble is painted with a unique non-zero grayscale ID)
//!   and a text seed's bbox, pick the bubble that contains the seed and
//!   return a distance-transform safe area inside that bubble.

use std::collections::HashMap;

use image::{GrayImage, Luma};
use imageproc::distance_transform::{Norm, distance_transform};

use crate::layout::WritingMode;
use crate::types::RenderBlock;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl LayoutBox {
    pub fn area(self) -> f32 {
        self.width.max(0.0) * self.height.max(0.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BubbleMatch {
    pub id: u8,
    pub layout_box: LayoutBox,
}

pub fn layout_box_from_block(block: &RenderBlock) -> LayoutBox {
    LayoutBox {
        x: block.x,
        y: block.y,
        width: block.width,
        height: block.height,
    }
}

/// Fraction of the bubble's short side reserved as distance-from-edge
/// padding before choosing an interior safe area. This preserves the old
/// bbox-inset proportions for rectangular bubbles while letting irregular
/// masks steer text away from tails, notches, and thin connectors.
const SAFE_PADDING_FRAC_HORIZONTAL: f32 = 0.12;
/// Vertical CJK columns need more margin so neighboring columns do not crowd
/// the balloon outline.
const SAFE_PADDING_FRAC_VERTICAL: f32 = 0.20;

fn safe_padding_fraction(writing_mode: WritingMode) -> f32 {
    match writing_mode {
        WritingMode::Horizontal => SAFE_PADDING_FRAC_HORIZONTAL,
        WritingMode::VerticalRl => SAFE_PADDING_FRAC_VERTICAL,
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
    bubbles: HashMap<u8, BubbleGeometry>,
}

impl BubbleIndex {
    /// Build the index. `mask` must have each detected bubble painted
    /// with a unique non-zero u8 ID (0 = outside any bubble).
    pub fn new(mask: GrayImage) -> Self {
        let w = mask.width();
        let h = mask.height();
        // Accumulator: id → (min_x, min_y, max_x, max_y).
        let mut extents: HashMap<u8, [i32; 4]> = HashMap::new();
        for y in 0..h {
            for x in 0..w {
                let id = mask.get_pixel(x, y).0[0];
                if id == 0 {
                    continue;
                }
                let e = extents
                    .entry(id)
                    .or_insert([x as i32, y as i32, x as i32, y as i32]);
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
        let bubbles = extents
            .into_iter()
            .map(|(id, e)| {
                let bbox = LayoutBox {
                    x: e[0] as f32,
                    y: e[1] as f32,
                    width: (e[2] - e[0] + 1) as f32,
                    height: (e[3] - e[1] + 1) as f32,
                };
                let horizontal_safe = safe_layout_box(&mask, id, bbox, WritingMode::Horizontal);
                let vertical_safe = safe_layout_box(&mask, id, bbox, WritingMode::VerticalRl);
                (
                    id,
                    BubbleGeometry {
                        horizontal_safe,
                        vertical_safe,
                    },
                )
            })
            .collect();
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
        let sx0 = (seed.x.floor() as i32).max(0).min(w - 1);
        let sy0 = (seed.y.floor() as i32).max(0).min(h - 1);
        let sx1 = ((seed.x + seed.width).ceil() as i32).clamp(sx0 + 1, w);
        let sy1 = ((seed.y + seed.height).ceil() as i32).clamp(sy0 + 1, h);

        let mut counts: HashMap<u8, u32> = HashMap::new();
        for y in sy0..sy1 {
            for x in sx0..sx1 {
                let id = self.mask.get_pixel(x as u32, y as u32).0[0];
                if id == 0 {
                    continue;
                }
                *counts.entry(id).or_insert(0) += 1;
            }
        }
        let (best_id, _) = counts.into_iter().max_by_key(|&(_, c)| c)?;
        let bubble = self.bubbles.get(&best_id)?;
        let layout_box = match writing_mode {
            WritingMode::Horizontal => bubble.horizontal_safe,
            WritingMode::VerticalRl => bubble.vertical_safe,
        };
        Some(BubbleMatch {
            id: best_id,
            layout_box,
        })
    }

    pub fn mask(&self) -> &GrayImage {
        &self.mask
    }
}

fn safe_layout_box(
    mask: &GrayImage,
    bubble_id: u8,
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
        if let Some(safe) = safe_layout_box_with_padding(mask, bubble_id, bbox, padding) {
            return safe;
        }
    }
    inset_layout_box(bbox, writing_mode)
}

fn safe_layout_box_with_padding(
    mask: &GrayImage,
    bubble_id: u8,
    bbox: LayoutBox,
    padding: u8,
) -> Option<LayoutBox> {
    let x0 = bbox.x.floor().max(0.0) as u32;
    let y0 = bbox.y.floor().max(0.0) as u32;
    let x1 = (bbox.x + bbox.width).ceil().min(mask.width() as f32) as u32;
    let y1 = (bbox.y + bbox.height).ceil().min(mask.height() as f32) as u32;
    let width = x1.checked_sub(x0)?;
    let height = y1.checked_sub(y0)?;
    if width == 0 || height == 0 {
        return None;
    }

    // `distance_transform` measures distance to foreground pixels. Build an
    // image where everything outside this bubble is foreground, then read the
    // distance from each bubble pixel to the nearest edge/background pixel.
    let mut background = GrayImage::from_pixel(width + 2, height + 2, Luma([255u8]));
    for y in 0..height {
        for x in 0..width {
            if mask.get_pixel(x0 + x, y0 + y).0[0] == bubble_id {
                background.put_pixel(x + 1, y + 1, Luma([0u8]));
            }
        }
    }
    let distance = distance_transform(&background, Norm::L2);

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
    if !is_safe_pixel(&background, &distance, cx, cy, safe_threshold) {
        (cx, cy) = nearest_safe_pixel(&background, &distance, cx, cy, safe_threshold)?;
    }

    let safe = build_safe_map(&background, &distance, width, height, safe_threshold);
    let (left, top, right, bottom) = largest_safe_rectangle(&safe, width, height, (cx, cy))?;

    Some(LayoutBox {
        x: x0 as f32 + left as f32,
        y: y0 as f32 + top as f32,
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
    fn layout_box_from_block_preserves_rect() {
        let block = RenderBlock {
            x: 12.0,
            y: 18.0,
            width: 40.0,
            height: 20.0,
            text: "hello".into(),
            source_direction: None,
        };
        assert_eq!(
            layout_box_from_block(&block),
            LayoutBox {
                x: 12.0,
                y: 18.0,
                width: 40.0,
                height: 20.0,
            }
        );
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
        let coverage = rect.area() / ((80.0 - 20.0) * (100.0 - 30.0));
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
