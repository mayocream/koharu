//! Anime Text YOLO detector. Emits `AddNode` ops for each detected text region.

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::{Op, TextData};
use koharu_ml::anime_text::AnimeTextDetector;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{
    clear_text_nodes_ops, load_source_image, new_text_node, page_node_count,
    sort_manga_reading_order, text_region_to_pair,
};

const DETECTOR_NAME: &str = "anime-text";

pub struct Model(AnimeTextDetector);

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
        sort_manga_reading_order(&mut pairs, ctx.options.reading_order.unwrap_or_default());

        let mut ops = clear_text_nodes_ops(ctx.scene, ctx.page);
        let removed = ops.len();
        let insertion_start = page_node_count(ctx.scene, ctx.page).saturating_sub(removed);
        ops.reserve(pairs.len());
        for (at, (bbox, text)) in (insertion_start..).zip(pairs) {
            let node = new_text_node(bbox, text);
            ops.push(Op::AddNode {
                page: ctx.page,
                node,
                at,
            });
        }
        Ok(ops)
    }
}

inventory::submit! {
    EngineInfo {
        id: "anime-text",
        name: "Anime Text YOLO (N)",
        needs: &[],
        produces: &[Artifact::TextBoxes],
        load: |runtime, cpu| Box::pin(async move {
            let m = AnimeTextDetector::load(runtime, cpu).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}
