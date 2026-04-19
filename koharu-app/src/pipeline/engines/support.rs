//! Shared helpers used by multiple engine implementations.
//!
//! The patterns here map `koharu-ml` / `koharu-llm` outputs (plain
//! `TextRegion`s, `DynamicImage`s) into `Op` sequences that mutate the scene.

use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView};
use koharu_core::{
    BlobRef, ImageData, ImageRole, MaskData, MaskRole, Node, NodeDataPatch, NodeId, NodeKind, Op,
    PageId, Region, Scene, TextData, Transform,
};

use crate::blobs::BlobStore;

// ---------------------------------------------------------------------------
// Read helpers
// ---------------------------------------------------------------------------

/// Find the Source image node on `page`. Returns `(node_id, image_data)`.
/// Every valid page has exactly one; absence means the page is malformed.
pub fn source_node(scene: &Scene, page: PageId) -> Result<(NodeId, &ImageData)> {
    let page = scene
        .page(page)
        .with_context(|| format!("page {} not found", page))?;
    for (id, node) in page.nodes.iter() {
        if let NodeKind::Image(img) = &node.kind
            && img.role == ImageRole::Source
        {
            return Ok((*id, img));
        }
    }
    anyhow::bail!("page has no Source image node")
}

/// Load the source image bytes + decoded image for `page`.
pub fn load_source_image(scene: &Scene, page: PageId, blobs: &BlobStore) -> Result<DynamicImage> {
    let (_, img_data) = source_node(scene, page)?;
    blobs.load_image(&img_data.blob)
}

/// Find a node of `Image { role }` on `page`, if any.
pub fn find_image_node(scene: &Scene, page: PageId, role: ImageRole) -> Option<(NodeId, BlobRef)> {
    let page = scene.page(page)?;
    page.nodes.iter().find_map(|(id, node)| match &node.kind {
        NodeKind::Image(img) if img.role == role => Some((*id, img.blob.clone())),
        _ => None,
    })
}

/// Find a node of `Mask { role }` on `page`, if any.
pub fn find_mask_node(scene: &Scene, page: PageId, role: MaskRole) -> Option<(NodeId, BlobRef)> {
    let page = scene.page(page)?;
    page.nodes.iter().find_map(|(id, node)| match &node.kind {
        NodeKind::Mask(mask) if mask.role == role => Some((*id, mask.blob.clone())),
        _ => None,
    })
}

/// Collect `(NodeId, &Transform, &TextData)` for every text node on `page`,
/// in stacking order.
pub fn text_nodes(scene: &Scene, page: PageId) -> Vec<(NodeId, &Transform, &TextData)> {
    let Some(page) = scene.page(page) else {
        return Vec::new();
    };
    page.nodes
        .iter()
        .filter_map(|(id, node)| match &node.kind {
            NodeKind::Text(t) => Some((*id, &node.transform, t)),
            _ => None,
        })
        .collect()
}

/// Convert a scene `(Transform, TextData)` pair into a `koharu-ml` `TextRegion`
/// for passing back through detector helpers that need geometry + language
/// hints (e.g. CTD's `refine_segmentation_mask`, OCR's `extract_text_block_regions`).
pub fn text_node_to_region(transform: &Transform, text: &TextData) -> koharu_ml::types::TextRegion {
    koharu_ml::types::TextRegion {
        x: transform.x,
        y: transform.y,
        width: transform.width,
        height: transform.height,
        confidence: text.confidence,
        line_polygons: text.line_polygons.clone(),
        source_direction: text.source_direction.map(core_text_direction_to_ml),
        rotation_deg: text.rotation_deg,
        detected_font_size_px: text.detected_font_size_px,
        detector: text.detector.clone(),
    }
}

/// Wrap a raw pixel `Region` as a `TextRegion` with no text hints. Used when
/// an inpainter engine receives a region override (repair-brush path).
pub fn region_to_text_region(r: &Region) -> koharu_ml::types::TextRegion {
    koharu_ml::types::TextRegion {
        x: r.x as f32,
        y: r.y as f32,
        width: r.width as f32,
        height: r.height as f32,
        confidence: 1.0,
        line_polygons: None,
        source_direction: None,
        rotation_deg: None,
        detected_font_size_px: None,
        detector: None,
    }
}

/// Inverse of `ml_text_direction_to_core`.
pub fn core_text_direction_to_ml(d: koharu_core::TextDirection) -> koharu_ml::types::TextDirection {
    match d {
        koharu_core::TextDirection::Horizontal => koharu_ml::types::TextDirection::Horizontal,
        koharu_core::TextDirection::Vertical => koharu_ml::types::TextDirection::Vertical,
    }
}

// ---------------------------------------------------------------------------
// Op constructors
// ---------------------------------------------------------------------------

/// Build an `AddNode` for a new `Image { role }` layer.
#[allow(clippy::too_many_arguments)]
pub fn add_image_node_op(
    page: PageId,
    role: ImageRole,
    blob: BlobRef,
    natural_width: u32,
    natural_height: u32,
    transform: Transform,
    visible: bool,
    at: usize,
) -> Op {
    let node = Node {
        id: NodeId::new(),
        transform,
        visible,
        kind: NodeKind::Image(ImageData {
            role,
            blob,
            opacity: 1.0,
            natural_width,
            natural_height,
            name: None,
        }),
    };
    Op::AddNode { page, node, at }
}

/// Build an `AddNode` for a new `Mask { role }` layer.
pub fn add_mask_node_op(
    page: PageId,
    role: MaskRole,
    blob: BlobRef,
    transform: Transform,
    visible: bool,
    at: usize,
) -> Op {
    let node = Node {
        id: NodeId::new(),
        transform,
        visible,
        kind: NodeKind::Mask(MaskData { role, blob }),
    };
    Op::AddNode { page, node, at }
}

/// Replace or add an `Image { role }` blob for `page`. If a node already
/// exists with that role, emits an `UpdateNode` with `ImageDataPatch`.
/// Otherwise emits `AddNode` at the top of the stack (renderer role) or
/// after Source (inpainted/custom role).
pub fn upsert_image_blob(
    scene: &Scene,
    page: PageId,
    role: ImageRole,
    blob: BlobRef,
    natural_width: u32,
    natural_height: u32,
) -> Op {
    if let Some((node_id, _)) = find_image_node(scene, page, role) {
        Op::UpdateNode {
            page,
            id: node_id,
            patch: koharu_core::NodePatch {
                data: Some(NodeDataPatch::Image(koharu_core::ImageDataPatch {
                    blob: Some(blob),
                    opacity: None,
                    name: None,
                    natural_width: Some(natural_width),
                    natural_height: Some(natural_height),
                })),
                transform: None,
                visible: None,
            },
            prev: koharu_core::NodePatch::default(),
        }
    } else {
        let at = {
            let page_ref = scene.page(page);
            let base = page_ref.map(|p| p.nodes.len()).unwrap_or(0);
            match role {
                // Rendered on top.
                ImageRole::Rendered => base,
                // Inpainted directly after source (index 1 if source is present).
                ImageRole::Inpainted => 1.min(base),
                // Custom / Source → append.
                _ => base,
            }
        };
        add_image_node_op(
            page,
            role,
            blob,
            natural_width,
            natural_height,
            Transform::default(),
            role != ImageRole::Rendered, // hide Rendered by default; make a toggle explicit
            at,
        )
    }
}

/// Replace or add a `Mask { role }` blob for `page`.
pub fn upsert_mask_blob(scene: &Scene, page: PageId, role: MaskRole, blob: BlobRef) -> Op {
    if let Some((node_id, _)) = find_mask_node(scene, page, role) {
        Op::UpdateNode {
            page,
            id: node_id,
            patch: koharu_core::NodePatch {
                data: Some(NodeDataPatch::Mask(koharu_core::MaskDataPatch {
                    blob: Some(blob),
                })),
                transform: None,
                visible: None,
            },
            prev: koharu_core::NodePatch::default(),
        }
    } else {
        let at = scene.page(page).map(|p| p.nodes.len()).unwrap_or(0);
        let visible = matches!(role, MaskRole::BrushInpaint);
        add_mask_node_op(page, role, blob, Transform::default(), visible, at)
    }
}

/// Build a `Node` ready to be added for a new Text region.
pub fn new_text_node(bbox: [f32; 4], text_data: TextData) -> Node {
    Node {
        id: NodeId::new(),
        transform: Transform {
            x: bbox[0],
            y: bbox[1],
            width: bbox[2] - bbox[0],
            height: bbox[3] - bbox[1],
            rotation_deg: text_data.rotation_deg.unwrap_or(0.0),
        },
        visible: true,
        kind: NodeKind::Text(text_data),
    }
}

/// Small helper: decoded image dimensions.
pub fn image_dimensions(image: &DynamicImage) -> (u32, u32) {
    image.dimensions()
}

/// Translate the `koharu-ml` `TextDirection` primitive into the scene-layer one.
pub fn ml_text_direction_to_core(d: koharu_ml::types::TextDirection) -> koharu_core::TextDirection {
    match d {
        koharu_ml::types::TextDirection::Horizontal => koharu_core::TextDirection::Horizontal,
        koharu_ml::types::TextDirection::Vertical => koharu_core::TextDirection::Vertical,
    }
}

/// Translate a `koharu-ml::TextRegion` (detector output) into a scene-layer
/// `(bbox, TextData)` pair ready for `new_text_node`.
pub fn text_region_to_pair(
    r: koharu_ml::types::TextRegion,
    default_detector: &'static str,
) -> ([f32; 4], TextData) {
    let bbox = [r.x, r.y, r.x + r.width, r.y + r.height];
    let data = TextData {
        confidence: r.confidence,
        source_direction: r.source_direction.map(ml_text_direction_to_core),
        line_polygons: r.line_polygons,
        rotation_deg: r.rotation_deg,
        detected_font_size_px: r.detected_font_size_px,
        detector: r.detector.or_else(|| Some(default_detector.to_string())),
        ..Default::default()
    };
    (bbox, data)
}

/// Current node count on `page`, or 0 if the page doesn't exist.
pub fn page_node_count(scene: &Scene, page: PageId) -> usize {
    scene.page(page).map(|p| p.nodes.len()).unwrap_or(0)
}

/// Emit `RemoveNode` ops for every text node currently on `page`. Detectors
/// prepend these so a re-detect replaces the previous blocks instead of
/// layering on top. `prev_node` / `prev_index` are the best snapshot we have
/// at emission time — `ops::apply` overwrites them with the live state for
/// undo anyway.
pub fn clear_text_nodes_ops(scene: &Scene, page: PageId) -> Vec<Op> {
    let Some(page_ref) = scene.page(page) else {
        return Vec::new();
    };
    page_ref
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, (_, node))| matches!(&node.kind, NodeKind::Text(_)))
        .map(|(idx, (id, node))| Op::RemoveNode {
            page,
            id: *id,
            prev_node: node.clone(),
            prev_index: idx,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Manga reading-order sort (Recursive XY-Cut)
//
// Right-to-left columns, top-to-bottom within each column. Shared by every
// detector that emits text blocks (CTD, comic-text-bubble, PP-DocLayout).
// ---------------------------------------------------------------------------

/// Sort `(bbox, data)` pairs in manga reading order (right-to-left, top-to-bottom).
pub fn sort_manga_reading_order<T>(blocks: &mut [([f32; 4], T)]) {
    #[derive(Debug, PartialEq, Clone, Copy)]
    enum Axis {
        X,
        Y,
    }

    if blocks.len() <= 1 {
        return;
    }

    let mut widths: Vec<f32> = blocks.iter().map(|(b, _)| b[2] - b[0]).collect();
    let mut heights: Vec<f32> = blocks.iter().map(|(b, _)| b[3] - b[1]).collect();
    widths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median_w = widths[widths.len() / 2].max(1.0);
    let median_h = heights[heights.len() / 2].max(1.0);
    let min_gap_x = (median_w * 0.15).max(10.0);
    let min_gap_y = (median_h * 0.10).max(8.0);

    fn xy_cut_recursive<T>(blocks: &mut [([f32; 4], T)], min_gap_x: f32, min_gap_y: f32) {
        use std::cmp::Ordering;
        if blocks.len() <= 1 {
            return;
        }
        let cut = find_best_cut(blocks, min_gap_x, min_gap_y);
        let Some((axis, gap)) = cut else {
            let row_height = min_gap_y * 4.0;
            blocks.sort_by(|a, b| {
                let row_a = (a.0[1] / row_height).floor();
                let row_b = (b.0[1] / row_height).floor();
                row_a
                    .partial_cmp(&row_b)
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| b.0[0].partial_cmp(&a.0[0]).unwrap_or(Ordering::Equal))
            });
            return;
        };

        let cut_coord = (gap.0 + gap.1) / 2.0;
        blocks.sort_by_key(|(b, _)| {
            if axis == Axis::X {
                // Right partition first (reading order): items whose center is LEFT of cut go second.
                (b[0] + (b[2] - b[0]) * 0.5) < cut_coord
            } else {
                // Top partition first: items whose center is BELOW cut go second.
                (b[1] + (b[3] - b[1]) * 0.5) > cut_coord
            }
        });

        let group1_len = blocks
            .iter()
            .filter(|(b, _)| {
                if axis == Axis::X {
                    (b[0] + (b[2] - b[0]) * 0.5) >= cut_coord
                } else {
                    (b[1] + (b[3] - b[1]) * 0.5) <= cut_coord
                }
            })
            .count();

        if group1_len == 0 || group1_len == blocks.len() {
            blocks.sort_by(|a, b| b.0[0].partial_cmp(&a.0[0]).unwrap_or(Ordering::Equal));
            return;
        }

        let (left, right) = blocks.split_at_mut(group1_len);
        xy_cut_recursive(left, min_gap_x, min_gap_y);
        xy_cut_recursive(right, min_gap_x, min_gap_y);
    }

    fn find_best_cut<T>(
        blocks: &[([f32; 4], T)],
        min_gap_x: f32,
        min_gap_y: f32,
    ) -> Option<(Axis, (f32, f32))> {
        let mut x_intervals: Vec<(f32, f32)> = blocks.iter().map(|(b, _)| (b[0], b[2])).collect();
        let mut y_intervals: Vec<(f32, f32)> = blocks.iter().map(|(b, _)| (b[1], b[3])).collect();
        x_intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        y_intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let gap_x = find_largest_gap(&x_intervals, min_gap_x);
        let gap_y = find_largest_gap(&y_intervals, min_gap_y);
        match (gap_x, gap_y) {
            (Some(gx), Some(gy)) => {
                let width_y = gy.1 - gy.0;
                let width_x = gx.1 - gx.0;
                if width_y > 12.0 || width_y > (width_x * 0.4) {
                    Some((Axis::Y, gy))
                } else {
                    Some((Axis::X, gx))
                }
            }
            (None, Some(gy)) => Some((Axis::Y, gy)),
            (Some(gx), None) => Some((Axis::X, gx)),
            (None, None) => None,
        }
    }

    fn find_largest_gap(intervals: &[(f32, f32)], min_gap: f32) -> Option<(f32, f32)> {
        if intervals.is_empty() {
            return None;
        }
        let mut largest: Option<(f32, f32)> = None;
        let mut current_max_end = intervals[0].1;
        for interval in intervals.iter().skip(1) {
            if interval.0 > current_max_end {
                let gap = interval.0 - current_max_end;
                if gap >= min_gap
                    && match largest {
                        Some(best) => gap > best.1 - best.0,
                        None => true,
                    }
                {
                    largest = Some((current_max_end, interval.0));
                }
            }
            current_max_end = current_max_end.max(interval.1);
        }
        largest
    }

    xy_cut_recursive(blocks, min_gap_x, min_gap_y);
}
