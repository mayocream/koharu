use image::DynamicImage;
use image::GenericImageView;
use imageproc::distance_transform::Norm;
use koharu_core::parse::parse_hex_color;
use koharu_core::views::{TextBlockInfo, to_block_info};
use koharu_core::{SerializableDynamicImage, TextBlock, TextStyle};
use tracing::instrument;

use crate::services::{
    AppResources,
    request::{
        BrushLayerUpdate, CreateTextBlockJob, InpaintMaskUpdate, MaskMorphJob, PartialInpaintJob,
        RemoveTextBlockJob, TextBlockUpdate,
    },
    store::{self, ChangedField},
};

use super::support::{InpaintRegionExt, blank_rgba};

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

pub async fn update_text_block(
    state: AppResources,
    payload: TextBlockUpdate,
) -> anyhow::Result<TextBlockInfo> {
    store::mutate_doc(
        &state.state,
        payload.document_index,
        &[ChangedField::TextBlocks],
        |document| {
            let block = document
                .text_blocks
                .get_mut(payload.text_block_index)
                .ok_or_else(|| {
                    anyhow::anyhow!("Text block {} not found", payload.text_block_index)
                })?;
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
            block.rendered_direction = None;
            Ok(to_block_info(payload.text_block_index, block))
        },
    )
    .await
}

pub async fn add_text_block(
    state: AppResources,
    payload: CreateTextBlockJob,
) -> anyhow::Result<usize> {
    store::mutate_doc(
        &state.state,
        payload.document_index,
        &[ChangedField::TextBlocks],
        |document| {
            let mut block = TextBlock {
                x: payload.block.x,
                y: payload.block.y,
                width: payload.block.width,
                height: payload.block.height,
                confidence: 1.0,
                ..Default::default()
            };
            block.set_layout_seed(block.x, block.y, block.width, block.height);
            document.text_blocks.push(block);
            Ok(document.text_blocks.len() - 1)
        },
    )
    .await
}

pub async fn remove_text_block(
    state: AppResources,
    payload: RemoveTextBlockJob,
) -> anyhow::Result<usize> {
    store::mutate_doc(
        &state.state,
        payload.document_index,
        &[ChangedField::TextBlocks],
        |document| {
            if payload.text_block_index >= document.text_blocks.len() {
                anyhow::bail!("Text block {} not found", payload.text_block_index);
            }
            document.text_blocks.remove(payload.text_block_index);
            Ok(document.text_blocks.len())
        },
    )
    .await
}

pub async fn dilate_mask(state: AppResources, payload: MaskMorphJob) -> anyhow::Result<()> {
    if payload.radius == 0 || payload.radius > 50 {
        anyhow::bail!("Radius must be 1-50");
    }

    store::mutate_doc(
        &state.state,
        payload.document_index,
        &[ChangedField::Segment],
        |document| {
            let segment = document
                .segment
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No segment mask. Run detect first."))?;

            let gray = segment.to_luma8();
            let dilated = imageproc::morphology::dilate(&gray, Norm::LInf, payload.radius);
            document.segment = Some(SerializableDynamicImage(DynamicImage::ImageLuma8(dilated)));
            Ok(())
        },
    )
    .await
}

pub async fn erode_mask(state: AppResources, payload: MaskMorphJob) -> anyhow::Result<()> {
    if payload.radius == 0 || payload.radius > 50 {
        anyhow::bail!("Radius must be 1-50");
    }

    store::mutate_doc(
        &state.state,
        payload.document_index,
        &[ChangedField::Segment],
        |document| {
            let segment = document
                .segment
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No segment mask. Run detect first."))?;

            let gray = segment.to_luma8();
            let eroded = imageproc::morphology::erode(&gray, Norm::LInf, payload.radius);
            document.segment = Some(SerializableDynamicImage(DynamicImage::ImageLuma8(eroded)));
            Ok(())
        },
    )
    .await
}

pub async fn update_inpaint_mask(
    state: AppResources,
    payload: InpaintMaskUpdate,
) -> anyhow::Result<()> {
    let snapshot = store::read_doc(&state.state, payload.document_index).await?;

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
    store::update_doc(
        &state.state,
        payload.document_index,
        updated,
        &[ChangedField::Segment],
    )
    .await
}

pub async fn update_brush_layer(
    state: AppResources,
    payload: BrushLayerUpdate,
) -> anyhow::Result<()> {
    let snapshot = store::read_doc(&state.state, payload.document_index).await?;

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

    store::update_doc(
        &state.state,
        payload.document_index,
        updated,
        &[ChangedField::BrushLayer],
    )
    .await
}

#[instrument(level = "info", skip_all)]
pub async fn inpaint_partial(
    state: AppResources,
    payload: PartialInpaintJob,
) -> anyhow::Result<()> {
    let snapshot = store::read_doc(&state.state, payload.document_index).await?;

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
        .vision
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

    store::update_doc(
        &state.state,
        payload.document_index,
        updated,
        &[ChangedField::Inpainted],
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{localize_inpaint_text_blocks, paste_crop};
    use image::{Rgba, RgbaImage};
    use koharu_core::TextBlock;

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
