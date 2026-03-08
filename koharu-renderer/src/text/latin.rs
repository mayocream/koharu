use std::collections::{HashMap, VecDeque};

use image::{GrayImage, Luma};
use imageproc::{
    contrast::{ThresholdType, adaptive_threshold, otsu_level, threshold},
    distance_transform::Norm,
    morphology::{dilate, erode},
    region_labelling::{Connectivity, connected_components},
};
use koharu_types::TextBlock;

use crate::layout::LayoutRun;

pub const LATIN_OVERFLOW_FACTOR: f32 = 1.15;
pub const LATIN_EXPANDED_OVERFLOW_FACTOR: f32 = 1.06;
pub const LATIN_MIN_LEGIBLE_FONT_SIZE: f32 = 13.5;
pub const LATIN_MIN_HEIGHT_FILL_RATIO: f32 = 0.55;

const MIN_EXPANDABLE_BLOCK_SIZE_PX: i32 = 8;

#[derive(Clone, Copy)]
struct ExpandProfile {
    max_expand_x_factor: f32,
    max_expand_x_px: i32,
    max_expand_y_factor: f32,
    max_expand_y_px: i32,
    border_dark_scale: f32,
    border_dark_max: u8,
    border_barrier_radius: u8,
    border_max_area_factor: f32,
    border_min_seed_passable: f32,
    border_edge_min_density: f32,
    min_global_threshold: u8,
    adaptive_radius_divisor: f32,
    adaptive_delta: i32,
    close_radius: u8,
    open_radius: u8,
    edge_min_density: f32,
    min_area_gain: f32,
    max_width_factor: f32,
    max_height_factor: f32,
    min_component_fill: f32,
}

const STRICT_PROFILE: ExpandProfile = ExpandProfile {
    max_expand_x_factor: 0.34,
    max_expand_x_px: 88,
    max_expand_y_factor: 0.20,
    max_expand_y_px: 44,
    border_dark_scale: 0.58,
    border_dark_max: 116,
    border_barrier_radius: 2,
    border_max_area_factor: 12.0,
    border_min_seed_passable: 0.02,
    border_edge_min_density: 0.14,
    min_global_threshold: 150,
    adaptive_radius_divisor: 6.5,
    adaptive_delta: 6,
    close_radius: 2,
    open_radius: 1,
    edge_min_density: 0.62,
    min_area_gain: 1.08,
    max_width_factor: 2.7,
    max_height_factor: 2.2,
    min_component_fill: 0.4,
};

const RELAXED_PROFILE: ExpandProfile = ExpandProfile {
    max_expand_x_factor: 0.52,
    max_expand_x_px: 136,
    max_expand_y_factor: 0.30,
    max_expand_y_px: 64,
    border_dark_scale: 0.64,
    border_dark_max: 132,
    border_barrier_radius: 1,
    border_max_area_factor: 18.0,
    border_min_seed_passable: 0.015,
    border_edge_min_density: 0.10,
    min_global_threshold: 136,
    adaptive_radius_divisor: 8.0,
    adaptive_delta: 12,
    close_radius: 2,
    open_radius: 1,
    edge_min_density: 0.52,
    min_area_gain: 1.03,
    max_width_factor: 3.2,
    max_height_factor: 2.8,
    min_component_fill: 0.28,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IntRect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl IntRect {
    fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Option<Self> {
        (x1 > x0 && y1 > y0).then_some(Self { x0, y0, x1, y1 })
    }

    fn from_layout_box(layout_box: LayoutBox) -> Self {
        Self {
            x0: layout_box.x.floor() as i32,
            y0: layout_box.y.floor() as i32,
            x1: (layout_box.x + layout_box.width).ceil() as i32,
            y1: (layout_box.y + layout_box.height).ceil() as i32,
        }
    }

    fn width(self) -> i32 {
        self.x1 - self.x0
    }

    fn height(self) -> i32 {
        self.y1 - self.y0
    }

    fn area(self) -> i32 {
        self.width().max(0) * self.height().max(0)
    }

    fn area_f32(self) -> f32 {
        self.area() as f32
    }

    fn to_layout_box(self) -> LayoutBox {
        LayoutBox {
            x: self.x0 as f32,
            y: self.y0 as f32,
            width: self.width() as f32,
            height: self.height() as f32,
        }
    }

    fn clamp_to(self, max_w: i32, max_h: i32) -> Option<Self> {
        let x0 = self.x0.clamp(0, max_w.saturating_sub(1));
        let y0 = self.y0.clamp(0, max_h.saturating_sub(1));
        let x1 = self.x1.clamp(x0 + 1, max_w);
        let y1 = self.y1.clamp(y0 + 1, max_h);
        Self::new(x0, y0, x1, y1)
    }

    fn expand(self, dx: i32, dy: i32, max_w: i32, max_h: i32) -> Self {
        Self {
            x0: (self.x0 - dx).max(0),
            y0: (self.y0 - dy).max(0),
            x1: (self.x1 + dx).min(max_w),
            y1: (self.y1 + dy).min(max_h),
        }
    }

    fn translate(self, dx: i32, dy: i32) -> Self {
        Self {
            x0: self.x0 + dx,
            y0: self.y0 + dy,
            x1: self.x1 + dx,
            y1: self.y1 + dy,
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    fn intersection_area(self, other: Self) -> i32 {
        let ix0 = self.x0.max(other.x0);
        let iy0 = self.y0.max(other.y0);
        let ix1 = self.x1.min(other.x1);
        let iy1 = self.y1.min(other.y1);
        (ix1 - ix0).max(0) * (iy1 - iy0).max(0)
    }

    fn clamp_expansion(
        self,
        original: Self,
        max_expand_x: i32,
        max_expand_y: i32,
        max_w: i32,
        max_h: i32,
    ) -> Self {
        Self {
            x0: self.x0.max(original.x0 - max_expand_x).max(0),
            y0: self.y0.max(original.y0 - max_expand_y).max(0),
            x1: self.x1.min(original.x1 + max_expand_x).min(max_w),
            y1: self.y1.min(original.y1 + max_expand_y).min(max_h),
        }
    }
}

pub fn layout_box_from_block(block: &TextBlock) -> LayoutBox {
    LayoutBox {
        x: block.x,
        y: block.y,
        width: block.width,
        height: block.height,
    }
}

pub fn is_expanded_layout_box(layout_box: LayoutBox, original: LayoutBox) -> bool {
    layout_box.width > original.width * 1.05 || layout_box.height > original.height * 1.05
}

pub fn layout_box_area(layout_box: LayoutBox) -> f32 {
    layout_box.width.max(0.0) * layout_box.height.max(0.0)
}

pub fn latin_height_fill(layout: &LayoutRun<'_>, container_height: f32) -> f32 {
    if !container_height.is_finite() || container_height <= 0.0 {
        return 1.0;
    }
    (layout.height / container_height).clamp(0.0, 1.0)
}

pub fn latin_layout_underfilled(layout: &LayoutRun<'_>, container_height: f32) -> bool {
    layout.font_size < LATIN_MIN_LEGIBLE_FONT_SIZE
        || latin_height_fill(layout, container_height) < LATIN_MIN_HEIGHT_FILL_RATIO
}

pub fn latin_width_overflow_factor(expanded_box: bool, allow_expanded_overflow: bool) -> f32 {
    if expanded_box {
        if allow_expanded_overflow {
            LATIN_EXPANDED_OVERFLOW_FACTOR
        } else {
            1.0
        }
    } else {
        LATIN_OVERFLOW_FACTOR
    }
}

pub fn pick_better_latin_candidate<'a>(
    current_layout: &LayoutRun<'a>,
    relaxed_candidate: Option<(LayoutRun<'a>, LayoutBox)>,
    overflow_candidate: Option<(LayoutRun<'a>, LayoutBox)>,
) -> Option<(LayoutRun<'a>, LayoutBox)> {
    let mut best: Option<(LayoutRun<'a>, LayoutBox)> = None;

    for candidate in [relaxed_candidate, overflow_candidate] {
        let Some((layout, layout_box)) = candidate else {
            continue;
        };

        if layout.font_size < current_layout.font_size + 0.25 {
            continue;
        }

        match &best {
            Some((best_layout, _)) if layout.font_size <= best_layout.font_size => {}
            _ => best = Some((layout, layout_box)),
        }
    }

    best
}

pub fn expand_latin_layout_box_strict(block: &TextBlock, bubble_map: &GrayImage) -> LayoutBox {
    expand_latin_layout_box_with_profile(block, bubble_map, STRICT_PROFILE)
}

pub fn expand_latin_layout_box_relaxed(block: &TextBlock, bubble_map: &GrayImage) -> LayoutBox {
    expand_latin_layout_box_with_profile(block, bubble_map, RELAXED_PROFILE)
}

fn expand_latin_layout_box_with_profile(
    block: &TextBlock,
    bubble_map: &GrayImage,
    profile: ExpandProfile,
) -> LayoutBox {
    let fallback = layout_box_from_block(block);
    let map_w = bubble_map.width() as i32;
    let map_h = bubble_map.height() as i32;
    if map_w <= 1 || map_h <= 1 {
        return fallback;
    }

    let Some(original) = clamped_bounds(fallback, map_w, map_h) else {
        return fallback;
    };

    let base_w = original.width();
    let base_h = original.height();
    if base_w < MIN_EXPANDABLE_BLOCK_SIZE_PX || base_h < MIN_EXPANDABLE_BLOCK_SIZE_PX {
        return fallback;
    }

    let max_expand_x = ((base_w as f32) * profile.max_expand_x_factor)
        .round()
        .clamp(8.0, profile.max_expand_x_px as f32) as i32;
    let max_expand_y = ((base_h as f32) * profile.max_expand_y_factor)
        .round()
        .clamp(4.0, profile.max_expand_y_px as f32) as i32;

    let roi_bounds = original.expand(max_expand_x, max_expand_y, map_w, map_h);
    let roi_w = roi_bounds.width() as usize;
    let roi_h = roi_bounds.height() as usize;
    if roi_w == 0 || roi_h == 0 {
        return fallback;
    }
    let roi = extract_roi_gray(
        bubble_map,
        roi_bounds.x0,
        roi_bounds.y0,
        roi_bounds.x1,
        roi_bounds.y1,
    );
    let seed = SeedRect {
        x0: (original.x0 - roi_bounds.x0).max(0),
        y0: (original.y0 - roi_bounds.y0).max(0),
        x1: (original.x1 - roi_bounds.x0).max(0),
        y1: (original.y1 - roi_bounds.y0).max(0),
    };

    if let Some((candidate_bounds, flooded_area)) =
        border_guided_expand_bounds(&roi, roi_bounds, seed, profile)
        && let Some(layout_box) =
            layout_box_from_candidate(candidate_bounds, original, flooded_area, profile)
    {
        return layout_box;
    }

    let global_threshold = otsu_level(&roi).max(profile.min_global_threshold);
    let global_bin = threshold(&roi, global_threshold, ThresholdType::Binary);
    let adaptive_radius = (((base_w.min(base_h) as f32) / profile.adaptive_radius_divisor)
        .round()
        .clamp(2.0, 18.0)) as u32;
    let adaptive_bin = adaptive_threshold(&roi, adaptive_radius, profile.adaptive_delta);

    let mut mask = intersect_binary_masks(&global_bin, &adaptive_bin);
    mask = morph_cleanup(mask, profile.close_radius, profile.open_radius);
    let labels = connected_components(&mask, Connectivity::Eight, Luma([0u8]));

    let Some(selected_component) = pick_best_component(&labels, seed) else {
        return fallback;
    };
    let component = selected_component.stats;

    let component_bounds = IntRect {
        x0: roi_bounds.x0 + component.min_x,
        y0: roi_bounds.y0 + component.min_y,
        x1: roi_bounds.x0 + component.max_x + 1,
        y1: roi_bounds.y0 + component.max_y + 1,
    };
    let overlap_area = original.intersection_area(component_bounds) as f32;
    let orig_area = original.area_f32();
    let component_area = component.area as f32;
    let overlap_ratio = if orig_area > 0.0 {
        overlap_area / orig_area
    } else {
        0.0
    };
    let component_ratio = if orig_area > 0.0 {
        component_area / orig_area
    } else {
        0.0
    };
    let seed_looks_oversized = component_ratio < 0.74 && overlap_ratio < 0.9;

    let mut expanded_bounds = if seed_looks_oversized {
        // Recover from previously over-expanded boxes by trusting the current bubble component.
        component_bounds
    } else {
        component_bounds.union(original)
    };

    expanded_bounds =
        expanded_bounds.clamp_expansion(original, max_expand_x, max_expand_y, map_w, map_h);

    expanded_bounds = tighten_expanded_bounds(
        &labels,
        selected_component.label,
        roi_bounds,
        expanded_bounds,
        original,
        profile.edge_min_density,
    );

    let out_w = expanded_bounds.width();
    let out_h = expanded_bounds.height();
    if out_w <= 0 || out_h <= 0 {
        return fallback;
    }

    let out_area = expanded_bounds.area_f32();
    if seed_looks_oversized {
        if out_area < orig_area * 0.24 {
            return fallback;
        }
    } else {
        if out_area < orig_area * profile.min_area_gain {
            return fallback;
        }
        if out_w as f32 > base_w as f32 * profile.max_width_factor
            || out_h as f32 > base_h as f32 * profile.max_height_factor
        {
            return fallback;
        }
    }

    let component_fill = component.area as f32 / out_area;
    if component_fill < profile.min_component_fill {
        return fallback;
    }

    expanded_bounds.to_layout_box()
}

#[derive(Clone, Copy)]
struct SeedRect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl SeedRect {
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }

    fn center(self) -> (f32, f32) {
        (
            (self.x0 as f32 + (self.x1 - 1).max(self.x0) as f32) * 0.5,
            (self.y0 as f32 + (self.y1 - 1).max(self.y0) as f32) * 0.5,
        )
    }
}

#[derive(Clone, Copy, Debug)]
struct ComponentStats {
    area: u32,
    overlap: u32,
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
    sum_x: u64,
    sum_y: u64,
}

impl ComponentStats {
    fn new(x: i32, y: i32) -> Self {
        Self {
            area: 0,
            overlap: 0,
            min_x: x,
            min_y: y,
            max_x: x,
            max_y: y,
            sum_x: 0,
            sum_y: 0,
        }
    }

    fn add_pixel(&mut self, x: i32, y: i32, overlaps_seed: bool) {
        self.area += 1;
        if overlaps_seed {
            self.overlap += 1;
        }
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
        self.sum_x += x as u64;
        self.sum_y += y as u64;
    }

    fn centroid(self) -> (f32, f32) {
        if self.area == 0 {
            return (self.min_x as f32, self.min_y as f32);
        }
        (
            self.sum_x as f32 / self.area as f32,
            self.sum_y as f32 / self.area as f32,
        )
    }
}

fn extract_roi_gray(map: &GrayImage, x0: i32, y0: i32, x1: i32, y1: i32) -> GrayImage {
    let w = (x1 - x0).max(1) as u32;
    let h = (y1 - y0).max(1) as u32;
    let mut roi = GrayImage::from_pixel(w, h, Luma([0u8]));
    for ry in 0..h {
        for rx in 0..w {
            let src_x = x0 + rx as i32;
            let src_y = y0 + ry as i32;
            roi.put_pixel(rx, ry, *map.get_pixel(src_x as u32, src_y as u32));
        }
    }
    roi
}

fn intersect_binary_masks(a: &GrayImage, b: &GrayImage) -> GrayImage {
    let w = a.width().min(b.width());
    let h = a.height().min(b.height());
    let mut out = GrayImage::from_pixel(w, h, Luma([0u8]));
    for y in 0..h {
        for x in 0..w {
            let on = a.get_pixel(x, y).0[0] > 0 && b.get_pixel(x, y).0[0] > 0;
            out.put_pixel(x, y, if on { Luma([255u8]) } else { Luma([0u8]) });
        }
    }
    out
}

fn morph_cleanup(mut mask: GrayImage, close_radius: u8, open_radius: u8) -> GrayImage {
    if close_radius > 0 {
        mask = dilate(&mask, Norm::LInf, close_radius);
        mask = erode(&mask, Norm::LInf, close_radius);
    }
    if open_radius > 0 {
        mask = erode(&mask, Norm::LInf, open_radius);
        mask = dilate(&mask, Norm::LInf, open_radius);
    }
    mask
}

#[derive(Clone, Copy)]
enum EdgeSide {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug)]
struct SelectedComponent {
    label: u32,
    stats: ComponentStats,
}

fn tighten_expanded_bounds(
    labels: &image::ImageBuffer<Luma<u32>, Vec<u32>>,
    label: u32,
    roi_bounds: IntRect,
    bounds: IntRect,
    original: IntRect,
    min_edge_density: f32,
) -> IntRect {
    if min_edge_density <= 0.0 {
        return bounds;
    }

    let labels_w = labels.width() as i32;
    let labels_h = labels.height() as i32;
    if labels_w <= 1 || labels_h <= 1 {
        return bounds;
    }

    let mut roi_relative = bounds.translate(-roi_bounds.x0, -roi_bounds.y0);
    roi_relative = IntRect {
        x0: roi_relative.x0.clamp(0, labels_w.saturating_sub(1)),
        y0: roi_relative.y0.clamp(0, labels_h.saturating_sub(1)),
        x1: roi_relative.x1.clamp(roi_relative.x0 + 1, labels_w),
        y1: roi_relative.y1.clamp(roi_relative.y0 + 1, labels_h),
    };

    let original_relative = IntRect {
        x0: (original.x0 - roi_bounds.x0).clamp(0, labels_w.saturating_sub(1)),
        y0: (original.y0 - roi_bounds.y0).clamp(0, labels_h.saturating_sub(1)),
        x1: (original.x1 - roi_bounds.x0).clamp(0, labels_w),
        y1: (original.y1 - roi_bounds.y0).clamp(0, labels_h),
    };

    let strip = ((roi_relative.width().min(roi_relative.height()) as f32) * 0.06)
        .round()
        .clamp(1.0, 4.0) as i32;

    while roi_relative.x0 < original_relative.x0 {
        if edge_density(labels, label, roi_relative, EdgeSide::Left, strip) >= min_edge_density {
            break;
        }
        roi_relative.x0 += 1;
    }
    while roi_relative.x1 > original_relative.x1 {
        if edge_density(labels, label, roi_relative, EdgeSide::Right, strip) >= min_edge_density {
            break;
        }
        roi_relative.x1 -= 1;
    }
    while roi_relative.y0 < original_relative.y0 {
        if edge_density(labels, label, roi_relative, EdgeSide::Top, strip) >= min_edge_density {
            break;
        }
        roi_relative.y0 += 1;
    }
    while roi_relative.y1 > original_relative.y1 {
        if edge_density(labels, label, roi_relative, EdgeSide::Bottom, strip) >= min_edge_density {
            break;
        }
        roi_relative.y1 -= 1;
    }

    roi_relative.translate(roi_bounds.x0, roi_bounds.y0)
}

fn edge_density(
    labels: &image::ImageBuffer<Luma<u32>, Vec<u32>>,
    label: u32,
    bounds: IntRect,
    side: EdgeSide,
    strip: i32,
) -> f32 {
    if bounds.width() <= 0 || bounds.height() <= 0 {
        return 1.0;
    }

    let strip = strip.max(1);
    let (sx0, sy0, sx1, sy1) = match side {
        EdgeSide::Left => (
            bounds.x0,
            bounds.y0,
            (bounds.x0 + strip).min(bounds.x1),
            bounds.y1,
        ),
        EdgeSide::Right => (
            (bounds.x1 - strip).max(bounds.x0),
            bounds.y0,
            bounds.x1,
            bounds.y1,
        ),
        EdgeSide::Top => (
            bounds.x0,
            bounds.y0,
            bounds.x1,
            (bounds.y0 + strip).min(bounds.y1),
        ),
        EdgeSide::Bottom => (
            bounds.x0,
            (bounds.y1 - strip).max(bounds.y0),
            bounds.x1,
            bounds.y1,
        ),
    };

    let width = (sx1 - sx0).max(0);
    let height = (sy1 - sy0).max(0);
    let total = (width * height) as u32;
    if total == 0 {
        return 1.0;
    }

    let mut count = 0u32;
    for y in sy0..sy1 {
        for x in sx0..sx1 {
            if labels.get_pixel(x as u32, y as u32).0[0] == label {
                count += 1;
            }
        }
    }
    count as f32 / total as f32
}

fn border_guided_expand_bounds(
    roi: &GrayImage,
    roi_bounds: IntRect,
    seed: SeedRect,
    profile: ExpandProfile,
) -> Option<(IntRect, u32)> {
    let w = roi.width() as i32;
    let h = roi.height() as i32;
    if w <= 1 || h <= 1 {
        return None;
    }

    let sx0 = seed.x0.clamp(0, w.saturating_sub(1));
    let sy0 = seed.y0.clamp(0, h.saturating_sub(1));
    let sx1 = seed.x1.clamp(sx0 + 1, w);
    let sy1 = seed.y1.clamp(sy0 + 1, h);
    let seed_w = (sx1 - sx0).max(1);
    let seed_h = (sy1 - sy0).max(1);
    let seed_total = (seed_w * seed_h) as u32;

    let mut seed_sum = 0u64;
    let mut seed_count = 0u64;
    for y in sy0..sy1 {
        for x in sx0..sx1 {
            seed_sum += roi.get_pixel(x as u32, y as u32).0[0] as u64;
            seed_count += 1;
        }
    }
    if seed_count == 0 {
        return None;
    }
    let seed_mean = seed_sum as f32 / seed_count as f32;
    let dark_threshold = (seed_mean * profile.border_dark_scale)
        .round()
        .clamp(18.0, profile.border_dark_max as f32) as u8;

    let mut barrier = GrayImage::from_pixel(roi.width(), roi.height(), Luma([0u8]));
    for y in 0..roi.height() {
        for x in 0..roi.width() {
            let lum = roi.get_pixel(x, y).0[0];
            if lum <= dark_threshold {
                barrier.put_pixel(x, y, Luma([255u8]));
            }
        }
    }
    if profile.border_barrier_radius > 0 {
        barrier = dilate(&barrier, Norm::LInf, profile.border_barrier_radius);
    }

    let mut visited = vec![0u8; (w as usize) * (h as usize)];
    let mut queue: VecDeque<(i32, i32)> = VecDeque::new();
    let mut seed_passable = 0u32;
    for y in sy0..sy1 {
        for x in sx0..sx1 {
            if barrier.get_pixel(x as u32, y as u32).0[0] != 0 {
                continue;
            }
            seed_passable += 1;
            let idx = (y as usize) * (w as usize) + (x as usize);
            if visited[idx] == 0 {
                visited[idx] = 1;
                queue.push_back((x, y));
            }
        }
    }

    let seed_passable_ratio = seed_passable as f32 / seed_total.max(1) as f32;
    if seed_passable_ratio < profile.border_min_seed_passable {
        return None;
    }

    if queue.is_empty() {
        let cx = ((sx0 + sx1) / 2).clamp(0, w.saturating_sub(1));
        let cy = ((sy0 + sy1) / 2).clamp(0, h.saturating_sub(1));
        if barrier.get_pixel(cx as u32, cy as u32).0[0] != 0 {
            return None;
        }
        let idx = (cy as usize) * (w as usize) + (cx as usize);
        visited[idx] = 1;
        queue.push_back((cx, cy));
    }

    let max_area =
        ((seed_total as f32) * profile.border_max_area_factor).max(seed_total as f32 + 16.0) as u32;
    let mut area = 0u32;
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0i32;
    let mut max_y = 0i32;

    while let Some((x, y)) = queue.pop_front() {
        area += 1;
        if area > max_area {
            return None;
        }
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);

        for ny in (y - 1)..=(y + 1) {
            for nx in (x - 1)..=(x + 1) {
                if nx == x && ny == y {
                    continue;
                }
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let idx = (ny as usize) * (w as usize) + (nx as usize);
                if visited[idx] != 0 {
                    continue;
                }
                if barrier.get_pixel(nx as u32, ny as u32).0[0] != 0 {
                    continue;
                }
                visited[idx] = 1;
                queue.push_back((nx, ny));
            }
        }
    }

    if area == 0 {
        return None;
    }

    let mut bx0 = min_x;
    let mut by0 = min_y;
    let mut bx1 = max_x + 1;
    let mut by1 = max_y + 1;
    tighten_flood_bounds(
        &visited,
        w,
        h,
        seed,
        &mut bx0,
        &mut by0,
        &mut bx1,
        &mut by1,
        profile.border_edge_min_density,
    );
    if bx1 <= bx0 || by1 <= by0 {
        return None;
    }

    Some((
        IntRect {
            x0: roi_bounds.x0 + bx0,
            y0: roi_bounds.y0 + by0,
            x1: roi_bounds.x0 + bx1,
            y1: roi_bounds.y0 + by1,
        },
        area,
    ))
}

fn layout_box_from_candidate(
    candidate_bounds: IntRect,
    original: IntRect,
    candidate_area: u32,
    profile: ExpandProfile,
) -> Option<LayoutBox> {
    let out_w = candidate_bounds.width();
    let out_h = candidate_bounds.height();
    if out_w <= 0 || out_h <= 0 {
        return None;
    }

    let orig_area = original.area_f32();
    let out_area = candidate_bounds.area_f32();
    if orig_area <= 0.0 || out_area <= 0.0 {
        return None;
    }

    let overlap_area = original.intersection_area(candidate_bounds) as f32;
    let overlap_ratio = overlap_area / orig_area;
    let candidate_ratio = candidate_area as f32 / orig_area;
    let seed_looks_oversized = candidate_ratio < 0.74 && overlap_ratio < 0.9;

    if seed_looks_oversized {
        if out_area < orig_area * 0.24 {
            return None;
        }
    } else {
        if out_area < orig_area * profile.min_area_gain {
            return None;
        }
        if out_w as f32 > original.width() as f32 * profile.max_width_factor
            || out_h as f32 > original.height() as f32 * profile.max_height_factor
        {
            return None;
        }
    }

    let fill = candidate_area as f32 / out_area;
    let min_fill = (profile.min_component_fill * 0.55).clamp(0.12, 0.9);
    if fill < min_fill {
        return None;
    }

    Some(candidate_bounds.to_layout_box())
}

#[allow(clippy::too_many_arguments)]
fn tighten_flood_bounds(
    visited: &[u8],
    w: i32,
    h: i32,
    seed: SeedRect,
    x0: &mut i32,
    y0: &mut i32,
    x1: &mut i32,
    y1: &mut i32,
    min_edge_density: f32,
) {
    if min_edge_density <= 0.0 || *x1 <= *x0 || *y1 <= *y0 {
        return;
    }

    let sx0 = seed.x0.clamp(0, w.saturating_sub(1));
    let sy0 = seed.y0.clamp(0, h.saturating_sub(1));
    let sx1 = seed.x1.clamp(sx0 + 1, w);
    let sy1 = seed.y1.clamp(sy0 + 1, h);
    let strip = (((*x1 - *x0).min(*y1 - *y0) as f32) * 0.06)
        .round()
        .clamp(1.0, 4.0) as i32;

    while *x0 < sx0 {
        if flood_edge_density(
            visited,
            w,
            IntRect {
                x0: *x0,
                y0: *y0,
                x1: *x1,
                y1: *y1,
            },
            EdgeSide::Left,
            strip,
        ) >= min_edge_density
        {
            break;
        }
        *x0 += 1;
        if *x1 <= *x0 {
            return;
        }
    }
    while *x1 > sx1 {
        if flood_edge_density(
            visited,
            w,
            IntRect {
                x0: *x0,
                y0: *y0,
                x1: *x1,
                y1: *y1,
            },
            EdgeSide::Right,
            strip,
        ) >= min_edge_density
        {
            break;
        }
        *x1 -= 1;
        if *x1 <= *x0 {
            return;
        }
    }
    while *y0 < sy0 {
        if flood_edge_density(
            visited,
            w,
            IntRect {
                x0: *x0,
                y0: *y0,
                x1: *x1,
                y1: *y1,
            },
            EdgeSide::Top,
            strip,
        ) >= min_edge_density
        {
            break;
        }
        *y0 += 1;
        if *y1 <= *y0 {
            return;
        }
    }
    while *y1 > sy1 {
        if flood_edge_density(
            visited,
            w,
            IntRect {
                x0: *x0,
                y0: *y0,
                x1: *x1,
                y1: *y1,
            },
            EdgeSide::Bottom,
            strip,
        ) >= min_edge_density
        {
            break;
        }
        *y1 -= 1;
        if *y1 <= *y0 {
            return;
        }
    }
}

fn flood_edge_density(visited: &[u8], w: i32, bounds: IntRect, side: EdgeSide, strip: i32) -> f32 {
    if bounds.width() <= 0 || bounds.height() <= 0 || w <= 0 {
        return 1.0;
    }
    let strip = strip.max(1);
    let (sx0, sy0, sx1, sy1) = match side {
        EdgeSide::Left => (
            bounds.x0,
            bounds.y0,
            (bounds.x0 + strip).min(bounds.x1),
            bounds.y1,
        ),
        EdgeSide::Right => (
            (bounds.x1 - strip).max(bounds.x0),
            bounds.y0,
            bounds.x1,
            bounds.y1,
        ),
        EdgeSide::Top => (
            bounds.x0,
            bounds.y0,
            bounds.x1,
            (bounds.y0 + strip).min(bounds.y1),
        ),
        EdgeSide::Bottom => (
            bounds.x0,
            (bounds.y1 - strip).max(bounds.y0),
            bounds.x1,
            bounds.y1,
        ),
    };
    let width = (sx1 - sx0).max(0);
    let height = (sy1 - sy0).max(0);
    let total = (width * height) as u32;
    if total == 0 {
        return 1.0;
    }

    let mut count = 0u32;
    for y in sy0..sy1 {
        for x in sx0..sx1 {
            let idx = (y as usize) * (w as usize) + (x as usize);
            if visited.get(idx).copied().unwrap_or(0) != 0 {
                count += 1;
            }
        }
    }
    count as f32 / total as f32
}

fn pick_best_component(
    labels: &image::ImageBuffer<Luma<u32>, Vec<u32>>,
    seed: SeedRect,
) -> Option<SelectedComponent> {
    let mut by_label: HashMap<u32, ComponentStats> = HashMap::new();
    for y in 0..labels.height() as i32 {
        for x in 0..labels.width() as i32 {
            let label = labels.get_pixel(x as u32, y as u32).0[0];
            if label == 0 {
                continue;
            }
            let entry = by_label
                .entry(label)
                .or_insert_with(|| ComponentStats::new(x, y));
            entry.add_pixel(x, y, seed.contains(x, y));
        }
    }
    if by_label.is_empty() {
        return None;
    }

    let (seed_cx, seed_cy) = seed.center();
    let mut best: Option<(f32, SelectedComponent)> = None;
    for (&label, &component) in &by_label {
        let (cx, cy) = component.centroid();
        let dist2 = (cx - seed_cx).powi(2) + (cy - seed_cy).powi(2);
        let score = if component.overlap > 0 {
            component.overlap as f32 * 10_000.0 + component.area as f32 - dist2 * 0.1
        } else {
            component.area as f32 - dist2 * 0.35
        };
        match best {
            Some((best_score, _)) if score <= best_score => {}
            _ => {
                best = Some((
                    score,
                    SelectedComponent {
                        label,
                        stats: component,
                    },
                ))
            }
        }
    }

    best.map(|(_, component)| component)
}

fn clamped_bounds(layout_box: LayoutBox, map_w: i32, map_h: i32) -> Option<IntRect> {
    IntRect::from_layout_box(layout_box).clamp_to(map_w, map_h)
}

#[cfg(test)]
mod tests {
    use image::{GrayImage, Luma};

    use super::{
        LATIN_OVERFLOW_FACTOR, LayoutBox, TextBlock, expand_latin_layout_box_relaxed,
        expand_latin_layout_box_strict, is_expanded_layout_box, latin_width_overflow_factor,
        layout_box_area,
    };

    fn synthetic_bubble_map() -> GrayImage {
        let mut img = GrayImage::from_pixel(96, 96, Luma([38]));
        for y in 20..76 {
            for x in 20..76 {
                img.put_pixel(x, y, Luma([232]));
            }
        }
        // dark bubble border
        for x in 20..76 {
            img.put_pixel(x, 20, Luma([26]));
            img.put_pixel(x, 75, Luma([26]));
        }
        for y in 20..76 {
            img.put_pixel(20, y, Luma([26]));
            img.put_pixel(75, y, Luma([26]));
        }
        // text-like dark noise inside
        for y in (28..68).step_by(6) {
            for x in 34..62 {
                img.put_pixel(x, y, Luma([60]));
            }
        }
        img
    }

    fn synthetic_bubble_with_thin_tail() -> GrayImage {
        let mut img = GrayImage::from_pixel(128, 96, Luma([34]));
        for y in 20..76 {
            for x in 20..84 {
                img.put_pixel(x, y, Luma([232]));
            }
        }
        // open thin tail to the right, connected to the main bubble
        for y in 44..49 {
            for x in 84..112 {
                img.put_pixel(x, y, Luma([232]));
            }
        }
        // inner dark text-like strokes
        for y in (30..68).step_by(7) {
            for x in 34..72 {
                img.put_pixel(x, y, Luma([58]));
            }
        }
        img
    }

    fn synthetic_ellipse_bubble_map() -> GrayImage {
        let w = 148u32;
        let h = 136u32;
        let mut img = GrayImage::from_pixel(w, h, Luma([214]));
        let cx = 76.0f32;
        let cy = 68.0f32;
        let rx = 20.0f32;
        let ry = 48.0f32;

        for y in 0..h {
            for x in 0..w {
                let nx = (x as f32 - cx) / rx;
                let ny = (y as f32 - cy) / ry;
                let d = nx * nx + ny * ny;
                if d <= 1.0 {
                    img.put_pixel(x, y, Luma([238]));
                }
                if (d - 1.0).abs() <= 0.045 {
                    img.put_pixel(x, y, Luma([18]));
                }
            }
        }

        // Halftone-like outside noise.
        for y in 0..h {
            for x in 0..w {
                if img.get_pixel(x, y).0[0] >= 230 && ((x + 2 * y) % 9 == 0) {
                    img.put_pixel(x, y, Luma([222]));
                }
            }
        }
        img
    }

    #[test]
    fn strict_expansion_grows_without_crossing_border() {
        let map = synthetic_bubble_map();
        let block = TextBlock {
            x: 38.0,
            y: 36.0,
            width: 12.0,
            height: 10.0,
            ..Default::default()
        };
        let expanded = expand_latin_layout_box_strict(&block, &map);

        assert!(expanded.width > block.width);
        assert!(expanded.height > block.height);
        assert!(expanded.x >= 20.0);
        assert!(expanded.y >= 20.0);
        assert!(expanded.x + expanded.width <= 76.0);
        assert!(expanded.y + expanded.height <= 76.0);
    }

    #[test]
    fn relaxed_candidate_not_smaller_than_strict_in_area() {
        let map = synthetic_bubble_map();
        let block = TextBlock {
            x: 38.0,
            y: 36.0,
            width: 12.0,
            height: 10.0,
            ..Default::default()
        };
        let strict = expand_latin_layout_box_strict(&block, &map);
        let relaxed = expand_latin_layout_box_relaxed(&block, &map);
        assert!(layout_box_area(relaxed) >= layout_box_area(strict));
    }

    #[test]
    fn overflow_factor_respects_expanded_state() {
        assert_eq!(
            latin_width_overflow_factor(false, false),
            LATIN_OVERFLOW_FACTOR
        );
        assert_eq!(latin_width_overflow_factor(true, false), 1.0);
        assert!(latin_width_overflow_factor(true, true) > 1.0);
    }

    #[test]
    fn expanded_layout_box_detection_works() {
        let original = LayoutBox {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 60.0,
        };
        let expanded = LayoutBox {
            x: 8.0,
            y: 8.0,
            width: 108.0,
            height: 64.0,
        };
        assert!(is_expanded_layout_box(expanded, original));
    }

    #[test]
    fn strict_expansion_avoids_thin_tail_overflow() {
        let map = synthetic_bubble_with_thin_tail();
        let block = TextBlock {
            x: 36.0,
            y: 34.0,
            width: 40.0,
            height: 22.0,
            ..Default::default()
        };
        let expanded = expand_latin_layout_box_strict(&block, &map);

        // Thin protrusions should not drag the box far to the right.
        assert!(expanded.x + expanded.width <= 86.0);
        assert!(expanded.width > block.width);
    }

    #[test]
    fn strict_expansion_can_shrink_oversized_seed_box() {
        let map = synthetic_bubble_map();
        let block = TextBlock {
            x: 8.0,
            y: 8.0,
            width: 74.0,
            height: 70.0,
            ..Default::default()
        };
        let adjusted = expand_latin_layout_box_strict(&block, &map);

        assert!(adjusted.width < block.width);
        assert!(adjusted.height < block.height);
        assert!(adjusted.x >= 20.0);
        assert!(adjusted.y >= 20.0);
        assert!(adjusted.x + adjusted.width <= 76.0);
        assert!(adjusted.y + adjusted.height <= 76.0);
    }

    #[test]
    fn strict_expansion_stops_at_ellipse_border() {
        let map = synthetic_ellipse_bubble_map();
        let block = TextBlock {
            x: 71.0,
            y: 44.0,
            width: 10.0,
            height: 34.0,
            ..Default::default()
        };
        let expanded = expand_latin_layout_box_strict(&block, &map);

        assert!(expanded.width > block.width);
        assert!(expanded.height > block.height);
        assert!(expanded.x >= 55.0);
        assert!(expanded.y >= 20.0);
        assert!(expanded.x + expanded.width <= 97.0);
        assert!(expanded.y + expanded.height <= 117.0);
    }
}
