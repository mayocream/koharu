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

pub async fn update_text_blocks(
    state: AppResources,
    payload: UpdateTextBlocksPayload,
) -> anyhow::Result<()> {
    state_tx::mutate_doc(&state.state, payload.index, |document| {
        document.text_blocks = payload.text_blocks;
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

        if let Some(translation) = payload.translation {
            block.translation = Some(translation);
        }
        if let Some(x) = payload.x {
            block.x = x;
        }
        if let Some(y) = payload.y {
            block.y = y;
        }
        if let Some(width) = payload.width {
            block.width = width;
        }
        if let Some(height) = payload.height {
            block.height = height;
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
        let block = TextBlock {
            x: payload.x,
            y: payload.y,
            width: payload.width,
            height: payload.height,
            confidence: 1.0,
            ..Default::default()
        };
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

    let patch_x1 = x0 + crop_width;
    let patch_y1 = y0 + crop_height;

    let overlaps_text = snapshot.text_blocks.iter().any(|block| {
        let bx0 = block.x.max(0.0);
        let by0 = block.y.max(0.0);
        let bx1 = (block.x + block.width).max(bx0);
        let by1 = (block.y + block.height).max(by0);
        bx0 < patch_x1 as f32 && by0 < patch_y1 as f32 && bx1 > x0 as f32 && by1 > y0 as f32
    });

    if !overlaps_text {
        return Ok(());
    }

    let image_crop =
        SerializableDynamicImage(snapshot.image.crop_imm(x0, y0, crop_width, crop_height));
    let mask_crop = SerializableDynamicImage(mask_image.crop_imm(x0, y0, crop_width, crop_height));

    let inpainted_crop = state.ml.inpaint_raw(&image_crop, &mask_crop).await?;

    let mut stitched = snapshot
        .inpainted
        .as_ref()
        .unwrap_or(&snapshot.image)
        .to_rgba8();

    let patch = inpainted_crop.to_rgba8();
    let original = image_crop.to_rgba8();
    let mask_rgba = mask_crop.to_rgba8();

    for y in 0..crop_height {
        for x in 0..crop_width {
            let mask_pixel = mask_rgba.get_pixel(x, y);
            let is_masked = mask_pixel.0[0] > 0 || mask_pixel.0[1] > 0 || mask_pixel.0[2] > 0;
            let pixel = if is_masked {
                patch.get_pixel(x, y)
            } else {
                original.get_pixel(x, y)
            };
            stitched.put_pixel(x0 + x, y0 + y, *pixel);
        }
    }

    let mut updated = snapshot;
    updated.inpainted = Some(image::DynamicImage::ImageRgba8(stitched).into());

    state_tx::update_doc(&state.state, payload.index, updated).await
}
