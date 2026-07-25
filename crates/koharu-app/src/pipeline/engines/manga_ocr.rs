//! Manga OCR. Each text node's bbox is cropped and sent through a small
//! CRNN; the recognised text is written back via `UpdateNode`.

use anyhow::Result;
use async_trait::async_trait;
use image::DynamicImage;
use koharu_core::{NodeDataPatch, NodePatch, Op, TextDataPatch};
use koharu_ml::comic_text_detector::crop_text_block_bbox;
use koharu_ml::manga_ocr::MangaOcr;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{ConcurrencyHint, Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{load_source_image, text_node_to_region, text_nodes};

/// Upper bound on pages folded into one forward pass.
///
/// The real unit here is *crops*, not pages — `MangaOcr::inference` cats every
/// crop into one `[N,3,H,W]` tensor, and a single page already yields 10-30 of
/// them. So the batch is capped by [`MAX_BATCH_CROPS`] and this only bounds
/// how far ahead the stage will look.
const MAX_BATCH_PAGES: usize = 4;

/// Crop budget for one forward pass. Activation memory scales linearly with
/// this, so it is the actual guard against OOM on smaller GPUs.
const MAX_BATCH_CROPS: usize = 64;

pub struct Model(MangaOcr);

/// Crops for one page, paired with the nodes they came from.
struct PageCrops {
    nodes: Vec<koharu_core::NodeId>,
    crops: Vec<DynamicImage>,
}

impl Model {
    fn page_crops(&self, ctx: &EngineCtx<'_>) -> Result<PageCrops> {
        let texts = text_nodes(ctx.scene, ctx.page);
        if texts.is_empty() {
            return Ok(PageCrops {
                nodes: Vec::new(),
                crops: Vec::new(),
            });
        }
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
        let mut nodes = Vec::with_capacity(texts.len());
        let mut crops = Vec::with_capacity(texts.len());
        for (node_id, transform, text) in &texts {
            let region = text_node_to_region(transform, text);
            crops.push(crop_text_block_bbox(&image, &region));
            nodes.push(*node_id);
        }
        Ok(PageCrops { nodes, crops })
    }

    /// One inference call per page — the fallback whenever a combined forward
    /// pass isn't safe or didn't work.
    fn infer_each(
        &self,
        ctxs: &[EngineCtx<'_>],
        per_page: Vec<Result<PageCrops>>,
    ) -> Vec<Result<Vec<Op>>> {
        ctxs.iter()
            .zip(per_page)
            .map(|(ctx, page)| match page {
                Ok(PageCrops { nodes, crops }) if !crops.is_empty() => self
                    .0
                    .inference(&crops)
                    .map(|texts| ocr_ops(ctx.page, &nodes, texts)),
                Ok(_) => Ok(Vec::new()),
                Err(err) => Err(err),
            })
            .collect()
    }
}

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let PageCrops { nodes, crops } = self.page_crops(&ctx)?;
        if crops.is_empty() {
            return Ok(Vec::new());
        }
        let recognised = self.0.inference(&crops)?;
        Ok(ocr_ops(ctx.page, &nodes, recognised))
    }

    /// `MangaOcr::inference` is a genuine tensor batch — every crop is cat'd
    /// into one tensor and put through a single forward pass — so crops from
    /// several pages cost far less together than separately.
    fn max_batch(&self, hint: &ConcurrencyHint) -> usize {
        MAX_BATCH_PAGES.min(hint.max_batch_pages)
    }

    async fn run_batch(&self, ctxs: Vec<EngineCtx<'_>>) -> Vec<Result<Vec<Op>>> {
        // Crop each page independently so one unreadable page fails alone.
        let per_page: Vec<Result<PageCrops>> =
            ctxs.iter().map(|ctx| self.page_crops(ctx)).collect();

        let flat: Vec<DynamicImage> = per_page
            .iter()
            .flatten()
            .flat_map(|page| page.crops.iter().cloned())
            .collect();

        // Over the crop budget, or nothing to batch: one call per page.
        if flat.is_empty() || flat.len() > MAX_BATCH_CROPS {
            return self.infer_each(&ctxs, per_page);
        }

        let recognised = match self.0.inference(&flat) {
            Ok(texts) => texts,
            // The batched forward failed as a unit; retry singly so only a
            // genuinely bad page ends up reported.
            Err(_) => return self.infer_each(&ctxs, per_page),
        };

        // Split the flat output back out by each page's crop count. Pages
        // that errored during cropping consumed none of it.
        let mut cursor = 0;
        ctxs.iter()
            .zip(per_page)
            .map(|(ctx, page)| match page {
                Ok(PageCrops { nodes, crops }) => {
                    let texts = recognised[cursor..cursor + crops.len()].to_vec();
                    cursor += crops.len();
                    Ok(ocr_ops(ctx.page, &nodes, texts))
                }
                Err(err) => Err(err),
            })
            .collect()
    }
}

fn ocr_ops(
    page: koharu_core::PageId,
    nodes: &[koharu_core::NodeId],
    texts: Vec<String>,
) -> Vec<Op> {
    let mut ops = Vec::with_capacity(nodes.len());
    for (node_id, text) in nodes.iter().zip(texts) {
        ops.push(Op::UpdateNode {
            page,
            id: *node_id,
            patch: NodePatch {
                data: Some(NodeDataPatch::Text(TextDataPatch {
                    text: Some(Some(text)),
                    ..Default::default()
                })),
                transform: None,
                visible: None,
            },
            prev: NodePatch::default(),
        });
    }
    ops
}

inventory::submit! {
    EngineInfo {
        id: "manga-ocr",
        name: "Manga OCR",
        needs: &[Artifact::TextBoxes],
        produces: &[Artifact::OcrText],
        load: |runtime, cpu| Box::pin(async move {
            let m = MangaOcr::load(runtime, cpu).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}
