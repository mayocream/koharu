//! Lama Manga inpainter. Reads source + segmentation mask from the page,
//! runs the model, writes the output as `Image { role: Inpainted }`.
//!
//! When `ctx.options.region` is set (e.g. repair-brush re-inpaint), the
//! engine composites onto the existing `Image { Inpainted }` if present
//! (falling back to `Source`) and processes just that one block. Without
//! a region, behaves as a full-page pass using the scene's text nodes
//! as block hints.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use koharu_core::{ImageRole, MaskRole, Op};
use koharu_ml::lama::Lama;
use koharu_ml::types::TextRegion;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{
    find_image_node, find_mask_node, image_dimensions, load_source_image, region_to_text_region,
    text_node_to_region, text_nodes, upsert_image_blob,
};

pub struct Model(Lama);

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let (_, mask_ref) = find_mask_node(ctx.scene, ctx.page, MaskRole::Segment)
            .ok_or_else(|| anyhow!("no Segment mask on page"))?;
        let mask = ctx.blobs.load_image(&mask_ref)?;

        let (image, text_regions): (_, Vec<TextRegion>) = match ctx.options.region {
            Some(r) => {
                let base = match find_image_node(ctx.scene, ctx.page, ImageRole::Inpainted) {
                    Some((_, blob)) => ctx.blobs.load_image(&blob)?,
                    None => load_source_image(ctx.scene, ctx.page, ctx.blobs)?,
                };
                (base, vec![region_to_text_region(&r)])
            }
            None => {
                let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
                let regions = text_nodes(ctx.scene, ctx.page)
                    .iter()
                    .map(|(_, transform, t)| text_node_to_region(transform, t))
                    .collect();
                (image, regions)
            }
        };

        let regions_ref = (!text_regions.is_empty()).then_some(text_regions.as_slice());
        let result = self.0.inference_with_blocks(&image, &mask, regions_ref)?;
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

inventory::submit! {
    EngineInfo {
        id: "lama-manga",
        name: "Lama Manga",
        needs: &[Artifact::SegmentMask],
        produces: &[Artifact::Inpainted],
        load: |runtime, cpu| Box::pin(async move {
            let m = Lama::load(runtime, cpu).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}
