//! Comic Text Detector (full): text-box detector + UNet-based segmentation
//! mask. Emits `AddNode` ops for each detected text region plus an
//! `AddNode` / `UpdateNode` for the `Mask { Segment }` layer.

use anyhow::Result;
use async_trait::async_trait;
use image::DynamicImage;
use koharu_core::{Op, TextData};
use koharu_ml::comic_text_detector::ComicTextDetector;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{
    clear_text_nodes_ops, load_source_image, new_text_node, page_node_count,
    sort_manga_reading_order, text_region_to_pair, upsert_mask_blob,
};

const DETECTOR_NAME: &str = "ctd";

pub struct Model(ComicTextDetector);

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
        let det = self.0.inference(&image)?;

        // Segmentation mask blob.
        let mask_blob = ctx.blobs.put_webp(&DynamicImage::ImageLuma8(det.mask))?;

        let mut ops = clear_text_nodes_ops(ctx.scene, ctx.page);
        let removed = ops.len();
        let mut running_len = page_node_count(ctx.scene, ctx.page).saturating_sub(removed);

        let mask_op = upsert_mask_blob(
            ctx.scene,
            ctx.page,
            koharu_core::MaskRole::Segment,
            mask_blob,
        );
        if matches!(mask_op, Op::AddNode { .. }) {
            running_len += 1;
        }
        ops.push(mask_op);

        let mut pairs: Vec<([f32; 4], TextData)> = det
            .text_blocks
            .into_iter()
            .map(|r| text_region_to_pair(r, DETECTOR_NAME))
            .collect();
        sort_manga_reading_order(&mut pairs);
        for (bbox, text) in pairs {
            let node = new_text_node(bbox, text);
            ops.push(Op::AddNode {
                page: ctx.page,
                node,
                at: running_len,
            });
            running_len += 1;
        }
        Ok(ops)
    }
}

inventory::submit! {
    EngineInfo {
        id: "comic-text-detector",
        name: "Comic Text Detector",
        needs: &[],
        produces: &[Artifact::TextBoxes, Artifact::SegmentMask],
        load: |runtime, cpu| Box::pin(async move {
            let m = ComicTextDetector::load(runtime, cpu).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}
