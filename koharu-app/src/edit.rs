use image::DynamicImage;
use image::GenericImageView;
use imageproc::distance_transform::Norm;
use koharu_core::Region;
use koharu_core::parse::parse_hex_color;
use koharu_core::views::{TextBlockInfo, to_block_info};
use koharu_core::{SerializableDynamicImage, TextBlock, TextStyle};
use tracing::instrument;

use crate::AppResources;
use crate::utils::{InpaintRegionExt, blank_rgba};

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
    document_id: &str,
    text_blocks: Vec<TextBlock>,
) -> anyhow::Result<()> {
    state
        .storage
        .update_page(document_id, |page| {
            page.text_blocks = text_blocks;
        })
        .await
}

pub struct UpdateTextBlockArgs {
    pub text_block_index: usize,
    pub translation: Option<String>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub font_families: Option<Vec<String>>,
    pub font_size: Option<f32>,
    pub color: Option<String>,
    pub shader_effect: Option<String>,
}

pub async fn update_text_block(
    state: AppResources,
    document_id: &str,
    args: UpdateTextBlockArgs,
) -> anyhow::Result<TextBlockInfo> {
    let mut info = None;
    state
        .storage
        .update_page(document_id, |page| {
            let block = match page.text_blocks.get_mut(args.text_block_index) {
                Some(b) => b,
                None => return,
            };
            let mut geometry_changed = false;

            if let Some(translation) = &args.translation {
                block.translation = Some(translation.clone());
            }
            if let Some(x) = args.x {
                block.x = x;
                geometry_changed = true;
            }
            if let Some(y) = args.y {
                block.y = y;
                geometry_changed = true;
            }
            if let Some(width) = args.width {
                block.width = width;
                geometry_changed = true;
                block.lock_layout_box = true;
            }
            if let Some(height) = args.height {
                block.height = height;
                geometry_changed = true;
                block.lock_layout_box = true;
            }
            if geometry_changed {
                block.set_layout_seed(block.x, block.y, block.width, block.height);
            }

            if args.font_families.is_some()
                || args.font_size.is_some()
                || args.color.is_some()
                || args.shader_effect.is_some()
            {
                let style = block.style.get_or_insert_with(|| TextStyle {
                    font_families: Vec::new(),
                    font_size: None,
                    color: [0, 0, 0, 255],
                    effect: None,
                    stroke: None,
                    text_align: None,
                });

                if let Some(ref families) = args.font_families {
                    style.font_families = families.clone();
                }
                if let Some(font_size) = args.font_size {
                    style.font_size = Some(font_size);
                }
                if let Some(ref hex) = args.color
                    && let Ok(c) = parse_hex_color(hex)
                {
                    style.color = c;
                }
                if let Some(ref effect) = args.shader_effect
                    && let Ok(e) = effect.parse()
                {
                    style.effect = Some(e);
                }
            }

            block.rendered = None;
            block.rendered_direction = None;
            info = Some(to_block_info(args.text_block_index, block));
        })
        .await?;
    info.ok_or_else(|| anyhow::anyhow!("Text block {} not found", args.text_block_index))
}

pub async fn add_text_block(
    state: AppResources,
    document_id: &str,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> anyhow::Result<usize> {
    let mut count = 0;
    state
        .storage
        .update_page(document_id, |page| {
            let mut block = TextBlock {
                x,
                y,
                width,
                height,
                confidence: 1.0,
                ..Default::default()
            };
            block.set_layout_seed(block.x, block.y, block.width, block.height);
            page.text_blocks.push(block);
            count = page.text_blocks.len() - 1;
        })
        .await?;
    Ok(count)
}

pub async fn remove_text_block(
    state: AppResources,
    document_id: &str,
    text_block_index: usize,
) -> anyhow::Result<usize> {
    let mut count = 0;
    let mut found = false;
    state
        .storage
        .update_page(document_id, |page| {
            if text_block_index >= page.text_blocks.len() {
                return;
            }
            page.text_blocks.remove(text_block_index);
            count = page.text_blocks.len();
            found = true;
        })
        .await?;
    if !found {
        anyhow::bail!("Text block {} not found", text_block_index);
    }
    Ok(count)
}

pub async fn dilate_mask(state: AppResources, document_id: &str, radius: u8) -> anyhow::Result<()> {
    if radius == 0 || radius > 50 {
        anyhow::bail!("Radius must be 1-50");
    }

    let doc = state.storage.page(document_id).await?;

    let segment_ref = doc
        .segment
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No segment mask. Run detect first."))?;
    let segment = state.storage.images.load(segment_ref)?;

    let gray = segment.to_luma8();
    let dilated = imageproc::morphology::dilate(&gray, Norm::LInf, radius);
    let new_ref = state
        .storage
        .images
        .store_webp(&DynamicImage::ImageLuma8(dilated))?;

    state
        .storage
        .update_page(document_id, |page| {
            page.segment = Some(new_ref);
        })
        .await
}

pub async fn erode_mask(state: AppResources, document_id: &str, radius: u8) -> anyhow::Result<()> {
    if radius == 0 || radius > 50 {
        anyhow::bail!("Radius must be 1-50");
    }

    let doc = state.storage.page(document_id).await?;

    let segment_ref = doc
        .segment
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No segment mask. Run detect first."))?;
    let segment = state.storage.images.load(segment_ref)?;

    let gray = segment.to_luma8();
    let eroded = imageproc::morphology::erode(&gray, Norm::LInf, radius);
    let new_ref = state
        .storage
        .images
        .store_webp(&DynamicImage::ImageLuma8(eroded))?;

    state
        .storage
        .update_page(document_id, |page| {
            page.segment = Some(new_ref);
        })
        .await
}

pub async fn update_inpaint_mask(
    state: AppResources,
    document_id: &str,
    mask: &[u8],
    region: Option<Region>,
) -> anyhow::Result<()> {
    let doc = state.storage.page(document_id).await?;

    let update_image = image::load_from_memory(mask)?;
    let (doc_width, doc_height) = (doc.width, doc.height);

    let mut base_mask = doc
        .segment
        .as_ref()
        .map(|r| state.storage.images.load(r))
        .transpose()?
        .unwrap_or_else(|| blank_rgba(doc_width, doc_height, image::Rgba([0, 0, 0, 255])).into())
        .to_rgba8();

    match region {
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

    let new_ref = state
        .storage
        .images
        .store_webp(&image::DynamicImage::ImageRgba8(base_mask))?;

    state
        .storage
        .update_page(document_id, |page| {
            page.segment = Some(new_ref);
        })
        .await
}

pub async fn update_brush_layer(
    state: AppResources,
    document_id: &str,
    patch: &[u8],
    brush_region: Region,
) -> anyhow::Result<()> {
    let doc = state.storage.page(document_id).await?;

    let (img_width, img_height) = (doc.width, doc.height);
    let Some((x0, y0, width, height)) = brush_region.clamp(img_width, img_height) else {
        return Ok(());
    };

    let patch_image = image::load_from_memory(patch)?;
    let (patch_width, patch_height) = patch_image.dimensions();

    if patch_width != brush_region.width || patch_height != brush_region.height {
        anyhow::bail!(
            "Brush patch size mismatch: expected {}x{}, got {}x{}",
            brush_region.width,
            brush_region.height,
            patch_width,
            patch_height
        );
    }

    let brush_rgba = patch_image.to_rgba8();
    let mut brush_layer = doc
        .brush_layer
        .as_ref()
        .map(|r| state.storage.images.load(r))
        .transpose()?
        .unwrap_or_else(|| blank_rgba(img_width, img_height, image::Rgba([0, 0, 0, 0])).into())
        .to_rgba8();

    for y in 0..height {
        for x in 0..width {
            brush_layer.put_pixel(x0 + x, y0 + y, *brush_rgba.get_pixel(x, y));
        }
    }

    let new_ref = state
        .storage
        .images
        .store_webp(&image::DynamicImage::ImageRgba8(brush_layer))?;

    state
        .storage
        .update_page(document_id, |page| {
            page.brush_layer = Some(new_ref);
        })
        .await
}

#[instrument(level = "info", skip_all)]
pub async fn inpaint_partial(
    state: AppResources,
    document_id: &str,
    inpaint_region: Region,
) -> anyhow::Result<()> {
    let doc = state.storage.page(document_id).await?;

    let segment_ref = doc
        .segment
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Segment image not found"))?;
    let mask_image = state.storage.images.load(segment_ref)?;
    let source_image = state.storage.images.load(&doc.source)?;

    if inpaint_region.width == 0 || inpaint_region.height == 0 {
        return Ok(());
    }

    let (img_width, img_height) = (doc.width, doc.height);
    let x0 = inpaint_region.x.min(img_width.saturating_sub(1));
    let y0 = inpaint_region.y.min(img_height.saturating_sub(1));
    let x1 = inpaint_region
        .x
        .saturating_add(inpaint_region.width)
        .min(img_width);
    let y1 = inpaint_region
        .y
        .saturating_add(inpaint_region.height)
        .min(img_height);
    let crop_width = x1.saturating_sub(x0);
    let crop_height = y1.saturating_sub(y0);

    if crop_width == 0 || crop_height == 0 {
        return Ok(());
    }

    let localized_blocks =
        localize_inpaint_text_blocks(&doc.text_blocks, x0, y0, crop_width, crop_height);
    if localized_blocks.is_empty() {
        return Ok(());
    }

    let image_crop =
        SerializableDynamicImage(source_image.crop_imm(x0, y0, crop_width, crop_height));
    let mask_crop = SerializableDynamicImage(mask_image.crop_imm(x0, y0, crop_width, crop_height));

    let lama = state
        .registry
        .model("lama", || async {
            koharu_ml::lama::Lama::load(
                &state.runtime,
                matches!(state.device, koharu_ml::Device::Cpu),
            )
            .await
        })
        .await?;
    let result = lama.inference_with_blocks(&image_crop, &mask_crop, Some(&localized_blocks))?;
    let inpainted_crop = SerializableDynamicImage(result);

    let inpainted_base = doc
        .inpainted
        .as_ref()
        .map(|r| state.storage.images.load(r))
        .transpose()?
        .unwrap_or_else(|| source_image.clone());
    let mut stitched = inpainted_base.to_rgba8();

    let patch = inpainted_crop.to_rgba8();
    paste_crop(&mut stitched, &patch, x0, y0);

    let new_ref = state
        .storage
        .images
        .store_webp(&image::DynamicImage::ImageRgba8(stitched))?;

    state
        .storage
        .update_page(document_id, |page| {
            page.inpainted = Some(new_ref);
        })
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
