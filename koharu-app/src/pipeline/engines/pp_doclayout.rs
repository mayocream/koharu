//! PP-DocLayout V3 detector. Emits one `AddNode { Text }` per layout region
//! that looks like text, with geometry + detector metadata.

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::{Op, TextData, TextDirection};
use koharu_ml::pp_doclayout_v3::{LayoutRegion, PPDocLayoutV3};

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{clear_text_nodes_ops, load_source_image, new_text_node};

const VERTICAL_ASPECT: f32 = 1.15;
const OVERLAP_THRESHOLD: f32 = 0.9;
const DETECTOR_NAME: &str = "pp-doclayout-v3";
const CONFIDENCE_THRESHOLD: f32 = 0.25;

pub struct Model(PPDocLayoutV3);

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;
        let layout = self.0.inference_one_fast(&image, CONFIDENCE_THRESHOLD)?;
        let blocks = build_text_blocks(&layout.regions);

        let mut ops = clear_text_nodes_ops(ctx.scene, ctx.page);
        let removed = ops.len();
        let base_len = ctx.scene.page(ctx.page).map(|p| p.nodes.len()).unwrap_or(0);
        let mut running_len = base_len.saturating_sub(removed);
        ops.reserve(blocks.len());
        for (bbox, text) in blocks {
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
        id: "pp-doclayout-v3",
        name: "PP-DocLayout V3",
        needs: &[],
        produces: &[Artifact::TextBoxes],
        load: |runtime, cpu| Box::pin(async move {
            let m = PPDocLayoutV3::load(runtime, cpu).await?;
            Ok(Box::new(Model(m)) as Box<dyn Engine>)
        }),
    }
}

// ---------------------------------------------------------------------------
// Region → (bbox, TextData) mapping
// ---------------------------------------------------------------------------

fn build_text_blocks(regions: &[LayoutRegion]) -> Vec<([f32; 4], TextData)> {
    let mut blocks: Vec<([f32; 4], TextData)> = regions
        .iter()
        .filter(|r| {
            let l = r.label.to_ascii_lowercase();
            l == "content" || l.contains("text") || l.contains("title")
        })
        .filter_map(|r| {
            let x1 = r.bbox[0].min(r.bbox[2]).max(0.0);
            let y1 = r.bbox[1].min(r.bbox[3]).max(0.0);
            let w = (r.bbox[0].max(r.bbox[2]).max(x1 + 1.0) - x1).max(1.0);
            let h = (r.bbox[1].max(r.bbox[3]).max(y1 + 1.0) - y1).max(1.0);
            if !(w >= 6.0 && h >= 6.0 && w * h >= 48.0) {
                return None;
            }
            let direction = if h >= w * VERTICAL_ASPECT {
                TextDirection::Vertical
            } else {
                TextDirection::Horizontal
            };
            let text = TextData {
                confidence: r.score,
                source_direction: Some(direction),
                source_lang: Some("unknown".to_string()),
                rotation_deg: Some(0.0),
                detected_font_size_px: Some(w.min(h).max(1.0)),
                detector: Some(DETECTOR_NAME.to_string()),
                ..Default::default()
            };
            Some(([x1, y1, x1 + w, y1 + h], text))
        })
        .collect();
    if blocks.len() >= 2 {
        let mut out = Vec::with_capacity(blocks.len());
        for (bbox, data) in std::mem::take(&mut blocks) {
            let area = ((bbox[2] - bbox[0]) * (bbox[3] - bbox[1])).max(1.0);
            let dup = out.iter().any(|(existing_bbox, _): &([f32; 4], TextData)| {
                let ea = ((existing_bbox[2] - existing_bbox[0])
                    * (existing_bbox[3] - existing_bbox[1]))
                    .max(1.0);
                let ov = overlap(bbox, *existing_bbox);
                ov / area >= OVERLAP_THRESHOLD || ov / ea >= OVERLAP_THRESHOLD
            });
            if !dup {
                out.push((bbox, data));
            }
        }
        blocks = out;
    }
    blocks
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> f32 {
    let w = a[2].min(b[2]) - a[0].max(b[0]);
    let h = a[3].min(b[3]) - a[1].max(b[1]);
    if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_ml::pp_doclayout_v3::LayoutRegion;

    fn region(label: &str, bbox: [f32; 4]) -> LayoutRegion {
        LayoutRegion {
            order: 0,
            label_id: 0,
            label: label.to_string(),
            score: 0.9,
            bbox,
            polygon_points: vec![],
        }
    }

    #[test]
    fn detect_keeps_text_and_dedupes() {
        let blocks = build_text_blocks(&[
            region("text", [10.0, 10.0, 40.0, 40.0]),
            region("image", [0.0, 0.0, 128.0, 128.0]),
            region("aside_text", [12.0, 12.0, 39.0, 39.0]),
            region("doc_title", [60.0, 8.0, 90.0, 24.0]),
        ]);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn tall_region_is_vertical() {
        let blocks = build_text_blocks(&[region("text", [5.0, 5.0, 20.0, 60.0])]);
        assert_eq!(blocks[0].1.source_direction, Some(TextDirection::Vertical));
    }
}
