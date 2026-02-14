use koharu_types::TextBlock;
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct DocumentInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub has_segment: bool,
    pub has_inpainted: bool,
    pub has_rendered: bool,
    pub text_blocks: Vec<TextBlockInfo>,
}

#[derive(Serialize)]
pub(crate) struct TextBlockInfo {
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

#[derive(Serialize)]
pub(crate) struct TextStyleInfo {
    pub font_families: Vec<String>,
    pub font_size: Option<f32>,
    pub color: [u8; 4],
    pub effect: Option<String>,
}

pub(crate) fn to_block_info(i: usize, b: &TextBlock) -> TextBlockInfo {
    TextBlockInfo {
        index: i,
        x: b.x,
        y: b.y,
        width: b.width,
        height: b.height,
        confidence: b.confidence,
        text: b.text.clone(),
        translation: b.translation.clone(),
        direction: b
            .font_prediction
            .as_ref()
            .map(|fp| format!("{:?}", fp.direction)),
        font_size_px: b.font_prediction.as_ref().map(|fp| fp.font_size_px),
        text_color: b.font_prediction.as_ref().map(|fp| fp.text_color),
        style: b.style.as_ref().map(|s| TextStyleInfo {
            font_families: s.font_families.clone(),
            font_size: s.font_size,
            color: s.color,
            effect: s.effect.map(|e| format!("{:?}", e)),
        }),
    }
}

pub(crate) fn to_doc_info(doc: &koharu_types::Document) -> DocumentInfo {
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
            .map(|(i, b)| to_block_info(i, b))
            .collect(),
    }
}
