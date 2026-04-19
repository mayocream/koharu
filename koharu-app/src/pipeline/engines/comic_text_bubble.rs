//! Comic Text & Bubble Detector (ogkalu RT-DETR). Emits `AddNode` ops for
//! each detected text region. Bubble detections are currently discarded at
//! the scene layer — bubble geometry is derived from the detected text
//! regions and the segmentation mask.

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::{Op, TextData};
use koharu_ml::comic_text_bubble_detector::ComicTextBubbleDetector;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{
    clear_text_nodes_ops, load_source_image, new_text_node, page_node_count,
    sort_manga_reading_order, text_region_to_pair,
};

const DETECTOR_NAME: &str = "comic-text-bubble-detector";

pub struct Model(ComicTextBubbleDetector);

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
        let det = self.0.inference(&image)?;

        let mut pairs: Vec<([f32; 4], TextData)> = det
            .text_blocks
            .into_iter()
            .map(|r| text_region_to_pair(r, DETECTOR_NAME))
            .collect();
        sort_manga_reading_order(&mut pairs);

        let mut ops = clear_text_nodes_ops(ctx.scene, ctx.page);
        let removed = ops.len();
        let mut running_len = page_node_count(ctx.scene, ctx.page).saturating_sub(removed);
        ops.reserve(pairs.len());
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
        id: "comic-text-bubble-detector",
        name: "Comic Text & Bubble Detector",
        needs: &[],
        produces: &[Artifact::TextBoxes],
        load: |runtime, cpu| Box::pin(async move {
            let m = ComicTextBubbleDetector::load(runtime, cpu).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}
