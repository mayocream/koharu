//! AOT inpainting. Simpler than Lama: direct source + segment → result.
//!
//! With `ctx.options.region`, composites onto the existing `Image { Inpainted }`
//! (falling back to Source) so repair-brush strokes only affect the touched
//! area. AOT inference has no blockwise overload, so we crop the base image
//! and mask to the region, inpaint the crop, and paste back.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use image::{DynamicImage, GenericImage, GenericImageView};
use koharu_core::{ImageRole, MaskRole, Op};
use koharu_ml::aot_inpainting::AotInpainting;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{
    find_image_node, find_mask_node, image_dimensions, load_source_image, upsert_image_blob,
};

pub struct Model(AotInpainting);

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let (_, mask_ref) = find_mask_node(ctx.scene, ctx.page, MaskRole::Segment)
            .ok_or_else(|| anyhow!("no Segment mask on page"))?;
        let mask = ctx.blobs.load_image(&mask_ref)?;

        let result = match ctx.options.region {
            None => {
                let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
                self.0.inference(&image, &mask)?
            }
            Some(r) => {
                let base = match find_image_node(ctx.scene, ctx.page, ImageRole::Inpainted) {
                    Some((_, blob)) => ctx.blobs.load_image(&blob)?,
                    None => load_source_image(ctx.scene, ctx.page, ctx.blobs)?,
                };
                let (w, h) = base.dimensions();
                let x0 = r.x.min(w.saturating_sub(1));
                let y0 = r.y.min(h.saturating_sub(1));
                let rw = r.width.min(w - x0).max(1);
                let rh = r.height.min(h - y0).max(1);
                let image_crop = DynamicImage::ImageRgba8(base.view(x0, y0, rw, rh).to_image());
                let mask_crop =
                    DynamicImage::ImageLuma8(mask.to_luma8().view(x0, y0, rw, rh).to_image());
                let patched = self.0.inference(&image_crop, &mask_crop)?;
                let mut out = base;
                out.copy_from(&patched, x0, y0)?;
                out
            }
        };

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
        id: "aot-inpainting",
        name: "AOT Inpainting",
        needs: &[Artifact::SegmentMask],
        produces: &[Artifact::Inpainted],
        load: |runtime, cpu| Box::pin(async move {
            let m = AotInpainting::load(runtime, cpu).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}
