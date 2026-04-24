//! Flux.2 Klein inpainter. Uses the CTD segment mask to build a looser
//! text-region mask, then runs Flux.2 inpainting on the resulting crop.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, Luma};
use koharu_core::{ImageRole, MaskRole, Op, Region};
use koharu_ml::flux2_klein::{Flux2InpaintOptions, Flux2Klein};
use koharu_ml::inpainting::mask::expand_glyph_mask_for_inpainting;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{
    find_image_node, find_mask_node, image_dimensions, load_source_image, text_node_to_region,
    text_nodes, upsert_image_blob,
};

pub struct Model(Flux2Klein);

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let (_, mask_ref) = find_mask_node(ctx.scene, ctx.page, MaskRole::Segment)
            .ok_or_else(|| anyhow!("no Segment mask on page"))?;
        let (_, bubble_ref) = find_mask_node(ctx.scene, ctx.page, MaskRole::Bubble)
            .ok_or_else(|| anyhow!("no Bubble mask on page"))?;
        let mask = ctx.blobs.load_image(&mask_ref)?;
        let bubble_mask = ctx.blobs.load_image(&bubble_ref)?;

        let (image, mask, bubble_mask) = match ctx.options.region {
            Some(r) => {
                let base = match find_image_node(ctx.scene, ctx.page, ImageRole::Inpainted) {
                    Some((_, blob)) => ctx.blobs.load_image(&blob)?,
                    None => load_source_image(ctx.scene, ctx.page, ctx.blobs)?,
                };
                let clipped_mask = clip_mask_to_region(&mask, &r);
                let clipped_bubble = clip_mask_to_region(&bubble_mask, &r);
                (base, clipped_mask, clipped_bubble)
            }
            None => {
                let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
                (image, mask, bubble_mask)
            }
        };

        let text_blocks = text_nodes(ctx.scene, ctx.page)
            .into_iter()
            .map(|(_, transform, text)| text_node_to_region(transform, text))
            .collect::<Vec<_>>();
        let expanded = expand_glyph_mask_for_inpainting(&mask, &bubble_mask, &text_blocks);
        let mask = match ctx.options.region {
            Some(r) => DynamicImage::ImageLuma8(clip_gray_mask_to_region(&expanded, &r)),
            None => DynamicImage::ImageLuma8(expanded),
        };
        let result =
            self.0
                .inpaint_with_reference(&image, &mask, None, &Flux2InpaintOptions::default())?;
        let (w, h) = image_dimensions(&result);
        let blob = ctx.blobs.put_webp(&result)?;
        Ok(vec![upsert_image_blob(
            ctx.scene,
            ctx.page,
            ImageRole::Inpainted,
            blob,
            w,
            h,
        )])
    }
}

fn clip_mask_to_region(mask: &DynamicImage, region: &Region) -> DynamicImage {
    DynamicImage::ImageLuma8(clip_gray_mask_to_region(&mask.to_luma8(), region))
}

fn clip_gray_mask_to_region(src: &GrayImage, region: &Region) -> GrayImage {
    let (w, h) = src.dimensions();
    let x0 = region.x.min(w);
    let y0 = region.y.min(h);
    let x1 = region.x.saturating_add(region.width).min(w);
    let y1 = region.y.saturating_add(region.height).min(h);

    let mut clipped = GrayImage::new(w, h);
    for y in y0..y1 {
        for x in x0..x1 {
            clipped.put_pixel(x, y, Luma([src.get_pixel(x, y).0[0]]));
        }
    }
    clipped
}

inventory::submit! {
    EngineInfo {
        id: "flux2-klein",
        name: "Flux.2 Klein",
        needs: &[Artifact::SegmentMask, Artifact::BubbleMask],
        produces: &[Artifact::Inpainted],
        load: |runtime, _cpu| Box::pin(async move {
            let m = Flux2Klein::load(runtime).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}
