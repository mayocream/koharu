//! Latin text layout helpers.
//!
//! Two pieces of public API:
//! - [`LayoutBox`] + [`layout_box_from_block`]: the axis-aligned box a text
//!   block lays out into. The layout engine treats this as a hard boundary.
//! - [`bubble_bbox_for_seed`]: given the bubble-segmentation mask (where
//!   each detected bubble is painted with a unique non-zero grayscale ID)
//!   and a text seed's bbox, pick the bubble that contains the seed and
//!   return that bubble's bbox — this is exactly the region the model
//!   detected, so it's the correct layout rect for the seed's text.

use std::collections::HashMap;

use image::GrayImage;

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

pub fn layout_box_from_block(block: &RenderBlock) -> LayoutBox {
    LayoutBox {
        x: block.x,
        y: block.y,
        width: block.width,
        height: block.height,
    }
}

/// Fraction of each axis trimmed off the bubble's bounding box to keep
/// text comfortably inside the balloon. 12% per side → ~76% of the
/// detected bbox is usable layout space — leaves breathing room around
/// the text instead of pushing lettering flush against the bubble
/// outline, and absorbs slight bbox imprecision from the segmenter.
const BUBBLE_INSET_FRAC: f32 = 0.12;

/// Pre-built index over a bubble-segmentation mask.
///
/// The mask encodes one bubble per non-zero grayscale value (IDs 1..=N).
/// This index scans the mask once to extract each ID's bounding box, so
/// repeated [`BubbleIndex::lookup`] calls on successive text seeds cost
/// only an O(seed_bbox_area) pixel-count to find the seed's majority ID.
pub struct BubbleIndex {
    mask: GrayImage,
    bboxes: HashMap<u8, LayoutBox>,
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
        let bboxes = extents
            .into_iter()
            .map(|(id, e)| {
                (
                    id,
                    LayoutBox {
                        x: e[0] as f32,
                        y: e[1] as f32,
                        width: (e[2] - e[0] + 1) as f32,
                        height: (e[3] - e[1] + 1) as f32,
                    },
                )
            })
            .collect();
        Self { mask, bboxes }
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
    pub fn lookup(&self, seed: LayoutBox) -> Option<LayoutBox> {
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
        let bbox = self.bboxes.get(&best_id)?;

        let inset_x = bbox.width * BUBBLE_INSET_FRAC;
        let inset_y = bbox.height * BUBBLE_INSET_FRAC;
        let width = (bbox.width - 2.0 * inset_x).max(1.0);
        let height = (bbox.height - 2.0 * inset_y).max(1.0);
        Some(LayoutBox {
            x: bbox.x + inset_x,
            y: bbox.y + inset_y,
            width,
            height,
        })
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

    #[test]
    fn layout_box_from_block_preserves_rect() {
        let block = RenderBlock {
            x: 12.0,
            y: 18.0,
            width: 40.0,
            height: 20.0,
            text: "hello".into(),
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
    fn lookup_returns_bubble_bbox_for_seed_inside() {
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
        let rect = index.lookup(seed).expect("should find bubble");
        // The returned rect is the bubble bbox with a small inset.
        assert!(rect.x >= 20.0);
        assert!(rect.y >= 30.0);
        assert!(rect.x + rect.width <= 80.0);
        assert!(rect.y + rect.height <= 100.0);
        // Padded inset reserves a comfortable margin on each side, so the
        // usable area is a meaningful fraction of the bubble but not the
        // whole thing.
        let coverage = rect.area() / ((80.0 - 20.0) * (100.0 - 30.0));
        assert!(coverage > 0.5);
        assert!(coverage < 0.95);
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
        let rect = index.lookup(seed).expect("should find bubble");
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
        assert!(index.lookup(seed).is_none());
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
        let rect = index.lookup(seed).expect("should find small bubble");
        // Small bubble bbox is roughly [80, 80] to [120, 120] → the
        // returned rect should be nested inside these bounds, not the
        // enclosing big bubble.
        assert!(rect.x >= 80.0);
        assert!(rect.y >= 80.0);
        assert!(rect.x + rect.width <= 120.0);
        assert!(rect.y + rect.height <= 120.0);
    }
}
