use koharu_types::{Document, TextBlock};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub has_segment: bool,
    pub has_inpainted: bool,
    pub has_rendered: bool,
    pub text_blocks: Vec<TextBlockInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextBlockInfo {
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub confidence: f32,
    pub text: Option<String>,
    pub translation: Option<String>,
    pub direction: Option<String>,
    pub font_size_px: Option<f32>,
    pub text_color: Option<[u8; 3]>,
    pub style: Option<TextStyleInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStyleInfo {
    pub font_families: Vec<String>,
    pub font_size: Option<f32>,
    pub color: [u8; 4],
    pub effect: Option<String>,
}

pub fn to_block_info(i: usize, block: &TextBlock) -> TextBlockInfo {
    TextBlockInfo {
        index: i,
        x: block.x,
        y: block.y,
        width: block.width,
        height: block.height,
        confidence: block.confidence,
        text: block.text.clone(),
        translation: block.translation.clone(),
        direction: block
            .font_prediction
            .as_ref()
            .map(|fp| format!("{:?}", fp.direction)),
        font_size_px: block.font_prediction.as_ref().map(|fp| fp.font_size_px),
        text_color: block.font_prediction.as_ref().map(|fp| fp.text_color),
        style: block.style.as_ref().map(|s| TextStyleInfo {
            font_families: s.font_families.clone(),
            font_size: s.font_size,
            color: s.color,
            effect: s.effect.map(|e| e.to_string()),
        }),
    }
}

pub fn to_doc_info(doc: &Document) -> DocumentInfo {
    DocumentInfo {
        name: doc.name.clone(),
        width: doc.width,
        height: doc.height,
        has_segment: doc.segment.is_some(),
        has_inpainted: doc.inpainted.is_some(),
        has_rendered: doc.rendered.is_some(),
        text_blocks: doc
            .text_blocks
            .iter()
            .enumerate()
            .map(|(i, block)| to_block_info(i, block))
            .collect(),
    }
}
