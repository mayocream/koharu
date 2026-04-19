//! Speech-bubble segmentation. Produces a `Mask { Bubble }` layer where
//! pixels inside any detected speech balloon are `255` and everything else
//! is `0`. The renderer consumes this mask to grow per-block text layout
//! boxes inside the bubble — so English translations wrap into the full
//! available space instead of being shrunk to fit the detector's
//! (Japanese-shaped) source bbox.
//!
//! We build the mask from the model's **detection bboxes** (one per
//! detected bubble), not from its per-pixel probability map. The model's
//! bbox head is consistently accurate while its mask head under-segments
//! bubble interiors — tight, text-shaped blobs that don't cover the full
//! balloon. A text rect computed from the under-segmented mask would land
//! wherever that small blob is (often off-centre in the actual bubble),
//! which is exactly the "text outside the bubble" artefact. Bbox-filled
//! rectangles trade a tiny bit of accuracy in the balloon's curved corners
//! (handled by the renderer's inset) for guaranteed full coverage.

use anyhow::Result;
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, Luma};
use koharu_core::{MaskRole, Op};
use koharu_ml::speech_bubble_segmentation::SpeechBubbleSegmentation;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{load_source_image, upsert_mask_blob};

pub struct Model(SpeechBubbleSegmentation);

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
        let result = self.0.inference(&image)?;

        let w = result.image_width;
        let h = result.image_height;
        let mut mask = GrayImage::from_pixel(w, h, Luma([0u8]));

        // Each detected bubble gets a unique non-zero grayscale ID in the
        // mask. The renderer reads the ID under a text seed to recover
        // exactly which bubble bbox that seed belongs to — no flood fill,
        // no CC merging, no partition heuristics. Overlapping bboxes stay
        // separable because smaller bubbles overwrite larger ones (painted
        // last, after sorting by descending area), so a text seed inside
        // a nested or embedded small bubble reads the small bubble's ID.
        //
        // Cap at 255 IDs; typical manga pages have well under 20 bubbles.
        let mut regions: Vec<_> = result.regions.iter().collect();
        regions.sort_by(|a, b| b.area.cmp(&a.area));
        for (i, region) in regions.iter().take(255).enumerate() {
            let id = (i + 1) as u8;
            let x0 = region.bbox[0].floor().max(0.0) as u32;
            let y0 = region.bbox[1].floor().max(0.0) as u32;
            let x1 = (region.bbox[2].ceil().max(0.0) as u32).min(w);
            let y1 = (region.bbox[3].ceil().max(0.0) as u32).min(h);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            for y in y0..y1 {
                for x in x0..x1 {
                    mask.put_pixel(x, y, Luma([id]));
                }
            }
        }

        let blob = ctx.blobs.put_webp(&DynamicImage::ImageLuma8(mask))?;
        Ok(vec![upsert_mask_blob(
            ctx.scene,
            ctx.page,
            MaskRole::Bubble,
            blob,
        )])
    }
}

inventory::submit! {
    EngineInfo {
        id: "speech-bubble-segmentation",
        name: "Speech Bubble Segmentation",
        needs: &[],
        produces: &[Artifact::BubbleMask],
        load: |runtime, cpu| Box::pin(async move {
            let m = SpeechBubbleSegmentation::load(runtime, cpu).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}
