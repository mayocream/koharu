//! YuzuMarker font detection. Takes each text node's bbox on the source
//! image, runs the ML model, attaches a `FontPrediction` to the node.

use anyhow::Result;
use async_trait::async_trait;
use image::DynamicImage;
use koharu_core::{FontPrediction, NodeDataPatch, NodePatch, Op, TextDataPatch};
use koharu_ml::font_detector::FontDetector;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{load_source_image, text_nodes};

pub struct Model(FontDetector);

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let texts = text_nodes(ctx.scene, ctx.page);
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
        let crops: Vec<DynamicImage> = texts
            .iter()
            .map(|(_, t, _)| {
                image.crop_imm(
                    t.x.max(0.0) as u32,
                    t.y.max(0.0) as u32,
                    t.width.max(1.0) as u32,
                    t.height.max(1.0) as u32,
                )
            })
            .collect();

        let mut preds = self.0.inference(&crops, 1)?;
        for p in &mut preds {
            normalize_font_prediction(p);
        }

        let mut ops = Vec::with_capacity(texts.len());
        for ((node_id, _, _), pred) in texts.iter().zip(preds) {
            ops.push(Op::UpdateNode {
                page: ctx.page,
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
        Ok(ops)
    }
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
