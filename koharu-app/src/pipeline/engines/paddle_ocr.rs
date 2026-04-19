//! PaddleOCR-VL. Vision-language OCR driven by llama.cpp + mtmd.
//!
//! Each text node on the page is cropped out of the source image, passed
//! through the multimodal model, and the recognised text is written back
//! via `UpdateNode { TextDataPatch { text } }`.

use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::{NodeDataPatch, NodePatch, Op, TextDataPatch};
use koharu_llm::paddleocr_vl::{PaddleOcrVl, PaddleOcrVlTask};
use koharu_ml::comic_text_detector::crop_text_block_bbox;

use crate::app::shared_llama_backend;
use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{load_source_image, text_node_to_region, text_nodes};

const MAX_NEW_TOKENS: usize = 128;

pub struct Model(Mutex<PaddleOcrVl>);

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let texts = text_nodes(ctx.scene, ctx.page);
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
        let regions: Vec<_> = texts
            .iter()
            .map(|(_, transform, text)| {
                let region = text_node_to_region(transform, text);
                crop_text_block_bbox(&image, &region)
            })
            .collect();

        let outputs = {
            let mut ocr = self
                .0
                .lock()
                .map_err(|_| anyhow::anyhow!("PaddleOCR mutex poisoned"))?;
            ocr.inference_images(&regions, PaddleOcrVlTask::Ocr, MAX_NEW_TOKENS)?
        };

        let mut ops = Vec::with_capacity(texts.len());
        for ((node_id, _, _), out) in texts.iter().zip(outputs) {
            ops.push(Op::UpdateNode {
                page: ctx.page,
                id: *node_id,
                patch: NodePatch {
                    data: Some(NodeDataPatch::Text(TextDataPatch {
                        text: Some(Some(out.text)),
                        ..Default::default()
                    })),
                    transform: None,
                    visible: None,
                },
                prev: NodePatch::default(),
            });
        }
        Ok(ops)
    }
}

inventory::submit! {
    EngineInfo {
        id: "paddle-ocr-vl-1.5",
        name: "PaddleOCR-VL",
        needs: &[Artifact::TextBoxes],
        produces: &[Artifact::OcrText],
        load: |runtime, cpu| Box::pin(async move {
            let backend = shared_llama_backend(runtime)?;
            let m = PaddleOcrVl::load(runtime, cpu, backend).await?;
            Ok(Box::new(Model(Mutex::new(m))) as Box<dyn Engine>)
        }),
    }
}
