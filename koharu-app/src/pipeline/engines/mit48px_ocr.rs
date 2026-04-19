//! MIT 48px OCR. Runs recognition per-text-block; the ML layer handles line
//! segmentation internally. Writes `text` back via `UpdateNode`.

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::{NodeDataPatch, NodePatch, Op, TextDataPatch};
use koharu_ml::mit48px_ocr::Mit48pxOcr;
use koharu_ml::types::TextRegion;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{load_source_image, text_node_to_region, text_nodes};

pub struct Model(Mit48pxOcr);

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let texts = text_nodes(ctx.scene, ctx.page);
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
        let regions: Vec<TextRegion> = texts
            .iter()
            .map(|(_, transform, text)| text_node_to_region(transform, text))
            .collect();
        let predictions = self.0.inference_text_blocks(&image, &regions)?;

        let mut ops = Vec::with_capacity(predictions.len());
        for prediction in predictions {
            if let Some((node_id, _, _)) = texts.get(prediction.block_index) {
                ops.push(Op::UpdateNode {
                    page: ctx.page,
                    id: *node_id,
                    patch: NodePatch {
                        data: Some(NodeDataPatch::Text(TextDataPatch {
                            text: Some(Some(prediction.text)),
                            ..Default::default()
                        })),
                        transform: None,
                        visible: None,
                    },
                    prev: NodePatch::default(),
                });
            }
        }
        Ok(ops)
    }
}

inventory::submit! {
    EngineInfo {
        id: "mit48px-ocr",
        name: "MIT 48px OCR",
        needs: &[Artifact::TextBoxes],
        produces: &[Artifact::OcrText],
        load: |runtime, cpu| Box::pin(async move {
            let m = Mit48pxOcr::load(runtime, cpu).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}
