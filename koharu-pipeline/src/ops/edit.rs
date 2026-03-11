use image::DynamicImage;
use image::GenericImageView;
use imageproc::distance_transform::Norm;
use koharu_api::commands::{
    AddTextBlockPayload, InpaintPartialPayload, MaskMorphPayload, RemoveTextBlockPayload,
    UpdateBrushLayerPayload, UpdateInpaintMaskPayload, UpdateTextBlockPayload,
    UpdateTextBlocksPayload,
};
use koharu_api::parse::parse_hex_color;
use koharu_api::views::{TextBlockInfo, to_block_info};
use koharu_types::{SerializableDynamicImage, TextBlock, TextStyle};
use tracing::instrument;

use crate::{AppResources, state_tx};

use super::utils::{InpaintRegionExt, blank_rgba};

const MATCH_GEOMETRY_EPS: f32 = 0.01;
const MATCH_NEAR_GEOMETRY_DELTA: f32 = 4.0;
const MATCH_TEXT_GEOMETRY_DELTA: f32 = 64.0;

fn geometry_delta(a: &TextBlock, b: &TextBlock) -> f32 {
    (a.x - b.x).abs() + (a.y - b.y).abs() + (a.width - b.width).abs() + (a.height - b.height).abs()
}

fn geometry_changed(a: &TextBlock, b: &TextBlock) -> bool {
    geometry_delta(a, b) > MATCH_GEOMETRY_EPS
}

fn size_changed(a: &TextBlock, b: &TextBlock) -> bool {
    (a.width - b.width).abs() > MATCH_GEOMETRY_EPS
        || (a.height - b.height).abs() > MATCH_GEOMETRY_EPS
}

fn seed_from_block(block: &TextBlock) -> Option<(f32, f32, f32, f32)> {
    match (
        block.layout_seed_x,
        block.layout_seed_y,
        block.layout_seed_width,
        block.layout_seed_height,
    ) {
        (Some(x), Some(y), Some(width), Some(height))
            if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 =>
        {
            Some((x, y, width, height))
        }
        _ => None,
    }
}

fn find_matching_previous(
    current: &TextBlock,
    previous: &[TextBlock],
    used_previous: &[bool],
) -> Option<usize> {
    let mut best_idx = None;
    let mut best_delta = f32::INFINITY;

    for (idx, prev) in previous.iter().enumerate() {
        if used_previous[idx] {
            continue;
        }
        let delta = geometry_delta(current, prev);
        if delta < best_delta {
            best_idx = Some(idx);
            best_delta = delta;
        }
    }

    let candidate_idx = best_idx?;
    let candidate = &previous[candidate_idx];
    if best_delta <= MATCH_NEAR_GEOMETRY_DELTA {
        return Some(candidate_idx);
    }

    let same_text = current.text == candidate.text;
    let same_translation = current.translation == candidate.translation;
    if same_text && same_translation && best_delta <= MATCH_TEXT_GEOMETRY_DELTA {
        return Some(candidate_idx);
    }

    None
}

fn rehydrate_runtime_text_block_state(current: &mut TextBlock, previous: Option<&TextBlock>) {
    let Some(prev) = previous else {
        current.lock_layout_box = false;
        current.set_layout_seed(current.x, current.y, current.width, current.height);
        return;
    };

    current.lock_layout_box = if size_changed(current, prev) {
        true
    } else {
        prev.lock_layout_box
    };

    if geometry_changed(current, prev) {
        current.set_layout_seed(current.x, current.y, current.width, current.height);
    } else if let Some((x, y, width, height)) = seed_from_block(prev) {
        current.set_layout_seed(x, y, width, height);
    } else {
        current.set_layout_seed(current.x, current.y, current.width, current.height);
    }
}

fn block_bounds(block: &TextBlock) -> Option<(f32, f32, f32, f32)> {
    let bx0 = block.x.max(0.0);
    let by0 = block.y.max(0.0);
    let bx1 = (block.x + block.width).max(bx0);
    let by1 = (block.y + block.height).max(by0);
    (bx1 > bx0 && by1 > by0).then_some((bx0, by0, bx1, by1))
}

fn localize_line_polygons(
    polygons: &Option<Vec<[[f32; 2]; 4]>>,
    x0: u32,
    y0: u32,
    crop_width: u32,
    crop_height: u32,
) -> Option<Vec<[[f32; 2]; 4]>> {
    polygons.as_ref().map(|polygons| {
        polygons
            .iter()
            .map(|polygon| {
                let mut localized = *polygon;
                for point in &mut localized {
                    point[0] = (point[0] - x0 as f32).clamp(0.0, crop_width as f32);
                    point[1] = (point[1] - y0 as f32).clamp(0.0, crop_height as f32);
                }
                localized
            })
            .collect()
    })
}

fn localize_inpaint_text_blocks(
    text_blocks: &[TextBlock],
    x0: u32,
    y0: u32,
    crop_width: u32,
    crop_height: u32,
) -> Vec<TextBlock> {
    let crop_x1 = x0 + crop_width;
    let crop_y1 = y0 + crop_height;

    text_blocks
        .iter()
        .filter_map(|block| {
            let (bx0, by0, bx1, by1) = block_bounds(block)?;
            let ix0 = bx0.max(x0 as f32);
            let iy0 = by0.max(y0 as f32);
            let ix1 = bx1.min(crop_x1 as f32);
            let iy1 = by1.min(crop_y1 as f32);
            if ix1 <= ix0 || iy1 <= iy0 {
                return None;
            }

            let mut localized = block.clone();
            localized.x = ix0 - x0 as f32;
            localized.y = iy0 - y0 as f32;
            localized.width = ix1 - ix0;
            localized.height = iy1 - iy0;
            localized.line_polygons =
                localize_line_polygons(&block.line_polygons, x0, y0, crop_width, crop_height);
            Some(localized)
        })
        .collect()
}

fn paste_crop(stitched: &mut image::RgbaImage, patch: &image::RgbaImage, x0: u32, y0: u32) {
    image::imageops::replace(stitched, patch, i64::from(x0), i64::from(y0));
}

pub async fn update_text_blocks(
    state: AppResources,
    payload: UpdateTextBlocksPayload,
) -> anyhow::Result<()> {
    state_tx::mutate_doc(&state.state, payload.index, |document| {
        let previous = std::mem::take(&mut document.text_blocks);
        document.text_blocks = payload.text_blocks;

        let mut used_previous = vec![false; previous.len()];
        for block in &mut document.text_blocks {
            let matched_idx = find_matching_previous(block, &previous, &used_previous);
            if let Some(idx) = matched_idx {
                used_previous[idx] = true;
                rehydrate_runtime_text_block_state(block, Some(&previous[idx]));
            } else {
                rehydrate_runtime_text_block_state(block, None);
            }
        }
        Ok(())
    })
    .await
}

pub async fn update_text_block(
    state: AppResources,
    payload: UpdateTextBlockPayload,
) -> anyhow::Result<TextBlockInfo> {
    state_tx::mutate_doc(&state.state, payload.index, |document| {
        let block = document
            .text_blocks
            .get_mut(payload.text_block_index)
            .ok_or_else(|| anyhow::anyhow!("Text block {} not found", payload.text_block_index))?;
        let mut geometry_changed = false;

        if let Some(translation) = payload.translation {
            block.translation = Some(translation);
        }
        if let Some(x) = payload.x {
            block.x = x;
            geometry_changed = true;
        }
        if let Some(y) = payload.y {
            block.y = y;
            geometry_changed = true;
        }
        if let Some(width) = payload.width {
            block.width = width;
            geometry_changed = true;
            block.lock_layout_box = true;
        }
        if let Some(height) = payload.height {
            block.height = height;
            geometry_changed = true;
            block.lock_layout_box = true;
        }
        if geometry_changed {
            block.set_layout_seed(block.x, block.y, block.width, block.height);
        }

        if payload.font_families.is_some()
            || payload.font_size.is_some()
            || payload.color.is_some()
            || payload.shader_effect.is_some()
        {
            let style = block.style.get_or_insert_with(|| TextStyle {
                font_families: Vec::new(),
                font_size: None,
                color: [0, 0, 0, 255],
                effect: None,
                stroke: None,
                text_align: None,
            });

            if let Some(families) = payload.font_families {
                style.font_families = families;
            }
            if let Some(font_size) = payload.font_size {
                style.font_size = Some(font_size);
            }
            if let Some(hex) = payload.color {
                style.color = parse_hex_color(&hex)?;
            }
            if let Some(effect) = payload.shader_effect {
                style.effect = Some(effect.parse()?);
            }
        }

        block.rendered = None;
        Ok(to_block_info(payload.text_block_index, block))
    })
    .await
}

pub async fn add_text_block(
    state: AppResources,
    payload: AddTextBlockPayload,
) -> anyhow::Result<usize> {
    state_tx::mutate_doc(&state.state, payload.index, |document| {
        let mut block = TextBlock {
            x: payload.x,
            y: payload.y,
            width: payload.width,
            height: payload.height,
            confidence: 1.0,
            ..Default::default()
        };
        block.set_layout_seed(block.x, block.y, block.width, block.height);
        document.text_blocks.push(block);
        Ok(document.text_blocks.len() - 1)
    })
    .await
}

pub async fn remove_text_block(
    state: AppResources,
    payload: RemoveTextBlockPayload,
) -> anyhow::Result<usize> {
    state_tx::mutate_doc(&state.state, payload.index, |document| {
        if payload.text_block_index >= document.text_blocks.len() {
            anyhow::bail!("Text block {} not found", payload.text_block_index);
        }
        document.text_blocks.remove(payload.text_block_index);
        Ok(document.text_blocks.len())
    })
    .await
}

pub async fn dilate_mask(state: AppResources, payload: MaskMorphPayload) -> anyhow::Result<()> {
    if payload.radius == 0 || payload.radius > 50 {
        anyhow::bail!("Radius must be 1-50");
    }

    state_tx::mutate_doc(&state.state, payload.index, |document| {
        let segment = document
            .segment
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No segment mask. Run detect first."))?;

        let gray = segment.to_luma8();
        let dilated = imageproc::morphology::dilate(&gray, Norm::LInf, payload.radius);
        document.segment = Some(SerializableDynamicImage(DynamicImage::ImageLuma8(dilated)));
        Ok(())
    })
    .await
}

pub async fn erode_mask(state: AppResources, payload: MaskMorphPayload) -> anyhow::Result<()> {
    if payload.radius == 0 || payload.radius > 50 {
        anyhow::bail!("Radius must be 1-50");
    }

    state_tx::mutate_doc(&state.state, payload.index, |document| {
        let segment = document
            .segment
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No segment mask. Run detect first."))?;

        let gray = segment.to_luma8();
        let eroded = imageproc::morphology::erode(&gray, Norm::LInf, payload.radius);
        document.segment = Some(SerializableDynamicImage(DynamicImage::ImageLuma8(eroded)));
        Ok(())
    })
    .await
}

pub async fn update_inpaint_mask(
    state: AppResources,
    payload: UpdateInpaintMaskPayload,
) -> anyhow::Result<()> {
    let snapshot = state_tx::read_doc(&state.state, payload.index).await?;

    let update_image = image::load_from_memory(&payload.mask)?;
    let (doc_width, doc_height) = (snapshot.width, snapshot.height);

    let mut base_mask = snapshot
        .segment
        .clone()
        .unwrap_or_else(|| blank_rgba(doc_width, doc_height, image::Rgba([0, 0, 0, 255])))
        .to_rgba8();

    match payload.region {
        Some(region) => {
            let (patch_width, patch_height) = update_image.dimensions();
            if patch_width != region.width || patch_height != region.height {
                anyhow::bail!(
                    "Mask patch size mismatch: expected {}x{}, got {}x{}",
                    region.width,
                    region.height,
                    patch_width,
                    patch_height
                );
            }

            let x0 = region.x.min(doc_width.saturating_sub(1));
            let y0 = region.y.min(doc_height.saturating_sub(1));
            let x1 = region.x.saturating_add(region.width).min(doc_width);
            let y1 = region.y.saturating_add(region.height).min(doc_height);

            if x1 <= x0 || y1 <= y0 {
                return Ok(());
            }

            let patch_rgba = update_image.to_rgba8();
            for y in 0..(y1 - y0) {
                for x in 0..(x1 - x0) {
                    base_mask.put_pixel(x0 + x, y0 + y, *patch_rgba.get_pixel(x, y));
                }
            }
        }
        None => {
            let (mask_width, mask_height) = update_image.dimensions();
            if mask_width != doc_width || mask_height != doc_height {
                anyhow::bail!(
                    "Mask size mismatch: expected {}x{}, got {}x{}",
                    doc_width,
                    doc_height,
                    mask_width,
                    mask_height
                );
            }
            base_mask = update_image.to_rgba8();
        }
    }

    let mut updated = snapshot;
    updated.segment = Some(image::DynamicImage::ImageRgba8(base_mask).into());
    state_tx::update_doc(&state.state, payload.index, updated).await
}

pub async fn update_brush_layer(
    state: AppResources,
    payload: UpdateBrushLayerPayload,
) -> anyhow::Result<()> {
    let snapshot = state_tx::read_doc(&state.state, payload.index).await?;

    let (img_width, img_height) = (snapshot.width, snapshot.height);
    let Some((x0, y0, width, height)) = payload.region.clamp(img_width, img_height) else {
        return Ok(());
    };

    let patch_image = image::load_from_memory(&payload.patch)?;
    let (patch_width, patch_height) = patch_image.dimensions();

    if patch_width != payload.region.width || patch_height != payload.region.height {
        anyhow::bail!(
            "Brush patch size mismatch: expected {}x{}, got {}x{}",
            payload.region.width,
            payload.region.height,
            patch_width,
            patch_height
        );
    }

    let brush_rgba = patch_image.to_rgba8();
    let mut brush_layer = snapshot
        .brush_layer
        .clone()
        .unwrap_or_else(|| blank_rgba(img_width, img_height, image::Rgba([0, 0, 0, 0])))
        .to_rgba8();

    for y in 0..height {
        for x in 0..width {
            brush_layer.put_pixel(x0 + x, y0 + y, *brush_rgba.get_pixel(x, y));
        }
    }

    let mut updated = snapshot;
    updated.brush_layer = Some(image::DynamicImage::ImageRgba8(brush_layer).into());

    state_tx::update_doc(&state.state, payload.index, updated).await
}

#[instrument(level = "info", skip_all)]
pub async fn inpaint_partial(
    state: AppResources,
    payload: InpaintPartialPayload,
) -> anyhow::Result<()> {
    let snapshot = state_tx::read_doc(&state.state, payload.index).await?;

    let mask_image = snapshot
        .segment
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Segment image not found"))?;

    if payload.region.width == 0 || payload.region.height == 0 {
        return Ok(());
    }

    let (img_width, img_height) = (snapshot.width, snapshot.height);
    let x0 = payload.region.x.min(img_width.saturating_sub(1));
    let y0 = payload.region.y.min(img_height.saturating_sub(1));
    let x1 = payload
        .region
        .x
        .saturating_add(payload.region.width)
        .min(img_width);
    let y1 = payload
        .region
        .y
        .saturating_add(payload.region.height)
        .min(img_height);
    let crop_width = x1.saturating_sub(x0);
    let crop_height = y1.saturating_sub(y0);

    if crop_width == 0 || crop_height == 0 {
        return Ok(());
    }

    let localized_blocks =
        localize_inpaint_text_blocks(&snapshot.text_blocks, x0, y0, crop_width, crop_height);
    if localized_blocks.is_empty() {
        return Ok(());
    }

    let image_crop =
        SerializableDynamicImage(snapshot.image.crop_imm(x0, y0, crop_width, crop_height));
    let mask_crop = SerializableDynamicImage(mask_image.crop_imm(x0, y0, crop_width, crop_height));

    let inpainted_crop = state
        .ml
        .inpaint_raw(&image_crop, &mask_crop, Some(&localized_blocks))
        .await?;

    let mut stitched = snapshot
        .inpainted
        .as_ref()
        .unwrap_or(&snapshot.image)
        .to_rgba8();

    let patch = inpainted_crop.to_rgba8();
    paste_crop(&mut stitched, &patch, x0, y0);

    let mut updated = snapshot;
    updated.inpainted = Some(image::DynamicImage::ImageRgba8(stitched).into());

    state_tx::update_doc(&state.state, payload.index, updated).await
}

#[cfg(test)]
mod tests {
    use super::{localize_inpaint_text_blocks, paste_crop, rehydrate_runtime_text_block_state};
    use image::{Rgba, RgbaImage};
    use koharu_types::TextBlock;

    #[test]
    fn resized_block_locks_layout_box() {
        let previous = TextBlock {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 80.0,
            ..Default::default()
        };
        let mut current = TextBlock {
            x: 10.0,
            y: 20.0,
            width: 72.0,
            height: 80.0,
            ..Default::default()
        };

        rehydrate_runtime_text_block_state(&mut current, Some(&previous));

        assert!(current.lock_layout_box);
        assert_eq!(current.seed_layout_box(), (10.0, 20.0, 72.0, 80.0));
    }

    #[test]
    fn unchanged_block_preserves_layout_box_lock_and_seed() {
        let mut previous = TextBlock {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 80.0,
            lock_layout_box: true,
            ..Default::default()
        };
        previous.set_layout_seed(5.0, 6.0, 70.0, 60.0);

        let mut current = TextBlock {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 80.0,
            ..Default::default()
        };

        rehydrate_runtime_text_block_state(&mut current, Some(&previous));

        assert!(current.lock_layout_box);
        assert_eq!(current.seed_layout_box(), (5.0, 6.0, 70.0, 60.0));
    }

    #[test]
    fn partial_inpaint_blocks_are_localized_to_crop() {
        let block = TextBlock {
            x: 40.0,
            y: 30.0,
            width: 40.0,
            height: 30.0,
            line_polygons: Some(vec![[
                [42.0, 32.0],
                [78.0, 32.0],
                [78.0, 40.0],
                [42.0, 40.0],
            ]]),
            ..Default::default()
        };

        let localized = localize_inpaint_text_blocks(&[block], 50, 20, 40, 30);
        assert_eq!(localized.len(), 1);
        assert_eq!(localized[0].x, 0.0);
        assert_eq!(localized[0].y, 10.0);
        assert_eq!(localized[0].width, 30.0);
        assert_eq!(localized[0].height, 20.0);
        assert_eq!(
            localized[0].line_polygons,
            Some(vec![[[0.0, 12.0], [28.0, 12.0], [28.0, 20.0], [0.0, 20.0]]])
        );
    }

    #[test]
    fn partial_inpaint_with_no_overlapping_blocks_returns_empty_list() {
        let block = TextBlock {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            ..Default::default()
        };

        let localized = localize_inpaint_text_blocks(&[block], 50, 20, 40, 30);
        assert!(localized.is_empty());
    }

    #[test]
    fn crop_paste_replaces_entire_returned_patch() {
        let mut stitched = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));
        let patch = RgbaImage::from_pixel(3, 3, Rgba([255, 0, 0, 255]));

        paste_crop(&mut stitched, &patch, 2, 2);

        assert_eq!(stitched.get_pixel(2, 2).0, [255, 0, 0, 255]);
        assert_eq!(stitched.get_pixel(4, 4).0, [255, 0, 0, 255]);
        assert_eq!(stitched.get_pixel(1, 1).0, [0, 0, 0, 255]);
    }
}
