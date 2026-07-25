//! YuzuMarker font detection. Takes each text node's bbox on the source
//! image, runs the ML model, attaches a `FontPrediction` to the node.

use anyhow::Result;
use async_trait::async_trait;
use image::DynamicImage;
use koharu_core::{FontPrediction, NodeDataPatch, NodePatch, Op, TextDataPatch};
use koharu_ml::font_detector::FontDetector;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{ConcurrencyHint, Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{load_source_image, text_nodes};

/// Upper bound on pages folded into one call. As with manga-ocr the real
/// unit is crops, bounded by [`MAX_BATCH_CROPS`].
const MAX_BATCH_PAGES: usize = 4;

/// Crop budget for one batched call.
const MAX_BATCH_CROPS: usize = 64;

pub struct Model(FontDetector);

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
        for (node_id, t, _) in &texts {
            crops.push(image.crop_imm(
                t.x.max(0.0) as u32,
                t.y.max(0.0) as u32,
                t.width.max(1.0) as u32,
                t.height.max(1.0) as u32,
            ));
            nodes.push(*node_id);
        }
        Ok(PageCrops { nodes, crops })
    }

    fn infer(
        &self,
        page: koharu_core::PageId,
        nodes: &[koharu_core::NodeId],
        crops: &[DynamicImage],
    ) -> Result<Vec<Op>> {
        let mut preds = self.0.inference(crops, 1)?;
        for p in &mut preds {
            normalize_font_prediction(p);
        }
        Ok(font_ops(page, nodes, preds))
    }

    /// One inference call per page — the fallback whenever a combined call
    /// isn't safe or didn't work.
    fn infer_each(
        &self,
        ctxs: &[EngineCtx<'_>],
        per_page: Vec<Result<PageCrops>>,
    ) -> Vec<Result<Vec<Op>>> {
        ctxs.iter()
            .zip(per_page)
            .map(|(ctx, page)| match page {
                Ok(PageCrops { nodes, crops }) if !crops.is_empty() => {
                    self.infer(ctx.page, &nodes, &crops)
                }
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
        self.infer(ctx.page, &nodes, &crops)
    }

    /// `FontDetector::inference` already takes a crop slice and preprocesses
    /// it in parallel before a single batched forward, so crops from several
    /// pages cost less together than separately.
    fn max_batch(&self, hint: &ConcurrencyHint) -> usize {
        MAX_BATCH_PAGES.min(hint.max_batch_pages)
    }

    async fn run_batch(&self, ctxs: Vec<EngineCtx<'_>>) -> Vec<Result<Vec<Op>>> {
        let per_page: Vec<Result<PageCrops>> =
            ctxs.iter().map(|ctx| self.page_crops(ctx)).collect();

        let flat: Vec<DynamicImage> = per_page
            .iter()
            .flatten()
            .flat_map(|page| page.crops.iter().cloned())
            .collect();

        if flat.is_empty() || flat.len() > MAX_BATCH_CROPS {
            return self.infer_each(&ctxs, per_page);
        }

        let mut preds = match self.0.inference(&flat, 1) {
            Ok(preds) => preds,
            Err(_) => return self.infer_each(&ctxs, per_page),
        };
        for p in &mut preds {
            normalize_font_prediction(p);
        }

        let mut cursor = 0;
        ctxs.iter()
            .zip(per_page)
            .map(|(ctx, page)| match page {
                Ok(PageCrops { nodes, crops }) => {
                    let slice = preds[cursor..cursor + crops.len()].to_vec();
                    cursor += crops.len();
                    Ok(font_ops(ctx.page, &nodes, slice))
                }
                Err(err) => Err(err),
            })
            .collect()
    }
}

fn font_ops(
    page: koharu_core::PageId,
    nodes: &[koharu_core::NodeId],
    preds: Vec<koharu_ml::types::FontPrediction>,
) -> Vec<Op> {
    let mut ops = Vec::with_capacity(nodes.len());
    for (node_id, pred) in nodes.iter().zip(preds) {
        ops.push(Op::UpdateNode {
            page,
            id: *node_id,
            patch: NodePatch {
                data: Some(NodeDataPatch::Text(TextDataPatch {
                    font_prediction: Some(Some(ml_prediction_to_core(pred))),
                    // Clear any previous style so the renderer re-derives.
                    style: Some(None),
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
        id: "yuzumarker-font-detection",
        name: "YuzuMarker Font Detection",
        needs: &[Artifact::TextBoxes],
        produces: &[Artifact::FontPredictions],
        load: |runtime, cpu| Box::pin(async move {
            let m = FontDetector::load(runtime, cpu).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}

// ---------------------------------------------------------------------------
// Translate ml FontPrediction → scene FontPrediction
// ---------------------------------------------------------------------------

fn ml_prediction_to_core(p: koharu_ml::types::FontPrediction) -> FontPrediction {
    FontPrediction {
        top_fonts: p
            .top_fonts
            .into_iter()
            .map(|tf| koharu_core::TopFont {
                index: tf.index,
                score: tf.score,
            })
            .collect(),
        named_fonts: p
            .named_fonts
            .into_iter()
            .map(|nf| koharu_core::NamedFontPrediction {
                index: nf.index,
                name: nf.name,
                language: nf.language,
                probability: nf.probability,
                serif: nf.serif,
            })
            .collect(),
        direction: match p.direction {
            koharu_ml::types::TextDirection::Horizontal => koharu_core::TextDirection::Horizontal,
            koharu_ml::types::TextDirection::Vertical => koharu_core::TextDirection::Vertical,
        },
        text_color: p.text_color,
        stroke_color: p.stroke_color,
        font_size_px: p.font_size_px,
        stroke_width_px: p.stroke_width_px,
        line_height: p.line_height,
        angle_deg: p.angle_deg,
    }
}

// ---------------------------------------------------------------------------
// Color normalization (ported from legacy engine.rs)
// ---------------------------------------------------------------------------

fn normalize_font_prediction(p: &mut koharu_ml::types::FontPrediction) {
    p.text_color = clamp_white(clamp_black(p.text_color));
    p.stroke_color = clamp_white(clamp_black(p.stroke_color));
    if p.stroke_width_px > 0.0 && colors_similar(p.text_color, p.stroke_color) {
        p.stroke_width_px = 0.0;
        p.stroke_color = p.text_color;
    }
}

fn clamp_black(c: [u8; 3]) -> [u8; 3] {
    let t = if gray(c) { 60 } else { 12 };
    if c[0] <= t && c[1] <= t && c[2] <= t {
        [0, 0, 0]
    } else {
        c
    }
}

fn clamp_white(c: [u8; 3]) -> [u8; 3] {
    let t = 255 - if gray(c) { 60 } else { 12 };
    if c[0] >= t && c[1] >= t && c[2] >= t {
        [255, 255, 255]
    } else {
        c
    }
}

fn gray(c: [u8; 3]) -> bool {
    c.iter().max().unwrap().abs_diff(*c.iter().min().unwrap()) <= 10
}

fn colors_similar(a: [u8; 3], b: [u8; 3]) -> bool {
    (0..3).all(|i| a[i].abs_diff(b[i]) <= 16)
}
