//! Comic Text Detector (segmentation-only). Needs text boxes from another
//! detector; produces a refined `Mask { Segment }` layer.

use anyhow::Result;
use async_trait::async_trait;
use image::DynamicImage;
use koharu_core::{MaskRole, Op};
use koharu_ml::comic_text_detector::{ComicTextDetector, refine_segmentation_mask};
use koharu_ml::types::TextRegion;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{
    load_source_image, text_node_to_region, text_nodes, upsert_mask_blob,
};

pub struct Model(ComicTextDetector);

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
        let prob_mask = self.0.inference_segmentation(&image)?;

        let regions: Vec<TextRegion> = text_nodes(ctx.scene, ctx.page)
            .iter()
            .map(|(_, transform, text)| text_node_to_region(transform, text))
            .collect();

        let mask = refine_segmentation_mask(&image, &prob_mask, &regions);
        let mask_blob = ctx.blobs.put_webp(&DynamicImage::ImageLuma8(mask))?;

        Ok(vec![upsert_mask_blob(
            ctx.scene,
            ctx.page,
            MaskRole::Segment,
            mask_blob,
        )])
    }
}

inventory::submit! {
    EngineInfo {
        id: "comic-text-detector-seg",
        name: "Comic Text Detector (Segmentation)",
        needs: &[Artifact::TextBoxes],
        produces: &[Artifact::SegmentMask],
        load: |runtime, cpu| Box::pin(async move {
            let m = ComicTextDetector::load_segmentation_only(runtime, cpu).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}
