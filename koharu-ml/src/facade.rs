use std::sync::Mutex;

use anyhow::Result;
use image::{DynamicImage, GrayImage};
use koharu_types::{Document, FontPrediction, SerializableDynamicImage, TextBlock, TextDirection};

use crate::comic_text_detector::extract_text_block_regions;
use crate::font_detector::{self, FontDetector};
use crate::lama::{self, Lama};
use crate::manga_text_segmentation_2025::{self, MangaTextSegmentation, ProbabilityMap};
use crate::paddleocr_vl::{self, PaddleOcrVl, PaddleOcrVlTask};
use crate::pp_doclayout_v3::{self, LayoutRegion, PPDocLayoutV3};

const NEAR_BLACK_THRESHOLD: u8 = 12;
const GRAY_NEAR_BLACK_THRESHOLD: u8 = 60;
const NEAR_WHITE_THRESHOLD: u8 = 12;
const GRAY_NEAR_WHITE_THRESHOLD: u8 = 60;
const GRAY_TOLERANCE: u8 = 10;
const SIMILAR_COLOR_MAX_DIFF: u8 = 16;
const PP_DOCLAYOUT_THRESHOLD: f32 = 0.25;
const SEGMENTATION_THRESHOLD: f32 = 0.1;
const VERTICAL_ASPECT_RATIO_THRESHOLD: f32 = 1.15;
const BLOCK_MASK_PADDING_RATIO: f32 = 0.18;
const BLOCK_MASK_MIN_PADDING: f32 = 6.0;
const BLOCK_OVERLAP_DEDUPE_THRESHOLD: f32 = 0.9;
const OCR_MAX_NEW_TOKENS: usize = 512;

fn clamp_near_black(color: [u8; 3]) -> [u8; 3] {
    let max_channel = *color.iter().max().unwrap_or(&0);
    let min_channel = *color.iter().min().unwrap_or(&0);
    let is_grayish = max_channel.saturating_sub(min_channel) <= GRAY_TOLERANCE;
    let threshold = if is_grayish {
        GRAY_NEAR_BLACK_THRESHOLD
    } else {
        NEAR_BLACK_THRESHOLD
    };

    if color[0] <= threshold && color[1] <= threshold && color[2] <= threshold {
        [0, 0, 0]
    } else {
        color
    }
}

fn clamp_near_white(color: [u8; 3]) -> [u8; 3] {
    let max_channel = *color.iter().max().unwrap_or(&0);
    let min_channel = *color.iter().min().unwrap_or(&0);
    let is_grayish = max_channel.saturating_sub(min_channel) <= GRAY_TOLERANCE;
    let threshold = if is_grayish {
        GRAY_NEAR_WHITE_THRESHOLD
    } else {
        NEAR_WHITE_THRESHOLD
    };

    let min_white = 255u8.saturating_sub(threshold);
    if color[0] >= min_white && color[1] >= min_white && color[2] >= min_white {
        [255, 255, 255]
    } else {
        color
    }
}

fn colors_similar(a: [u8; 3], b: [u8; 3]) -> bool {
    a[0].abs_diff(b[0]) <= SIMILAR_COLOR_MAX_DIFF
        && a[1].abs_diff(b[1]) <= SIMILAR_COLOR_MAX_DIFF
        && a[2].abs_diff(b[2]) <= SIMILAR_COLOR_MAX_DIFF
}

fn normalize_font_prediction(prediction: &mut FontPrediction) {
    prediction.text_color = clamp_near_white(clamp_near_black(prediction.text_color));
    prediction.stroke_color = clamp_near_white(clamp_near_black(prediction.stroke_color));

    if prediction.stroke_width_px > 0.0
        && colors_similar(prediction.text_color, prediction.stroke_color)
    {
        prediction.stroke_width_px = 0.0;
        prediction.stroke_color = prediction.text_color;
    }
}

pub struct Model {
    layout_detector: PPDocLayoutV3,
    segmenter: MangaTextSegmentation,
    ocr: Mutex<PaddleOcrVl>,
    lama: Lama,
    font_detector: FontDetector,
}

impl Model {
    pub async fn new(cpu: bool) -> Result<Self> {
        Ok(Self {
            layout_detector: PPDocLayoutV3::load(cpu).await?,
            segmenter: MangaTextSegmentation::load(cpu).await?,
            ocr: Mutex::new(PaddleOcrVl::load(cpu).await?),
            lama: Lama::load(cpu).await?,
            font_detector: FontDetector::load(cpu).await?,
        })
    }

    /// Detect text blocks and fonts in a document.
    /// Sets `doc.text_blocks` (with font predictions/styles) and `doc.segment`.
    pub async fn detect(&self, doc: &mut Document) -> Result<()> {
        let layout = self
            .layout_detector
            .inference_one(&doc.image, PP_DOCLAYOUT_THRESHOLD)?;
        doc.text_blocks = build_text_blocks(&layout.regions);

        let probability_map = self.segmenter.inference(&doc.image)?;
        let mask = build_segment_mask(&probability_map, &doc.text_blocks)?;
        doc.segment = Some(DynamicImage::ImageLuma8(mask).into());

        if !doc.text_blocks.is_empty() {
            let images: Vec<DynamicImage> = doc
                .text_blocks
                .iter()
                .map(|block| {
                    doc.image.crop_imm(
                        block.x as u32,
                        block.y as u32,
                        block.width as u32,
                        block.height as u32,
                    )
                })
                .collect();

            let font_predictions = self.detect_fonts(&images, 1).await?;
            for (block, prediction) in doc.text_blocks.iter_mut().zip(font_predictions) {
                block.font_prediction = Some(prediction);
                block.style = None;
            }
        }

        Ok(())
    }

    /// Run OCR on all text blocks in the document.
    /// Updates `doc.text_blocks` with recognized text.
    pub async fn ocr(&self, doc: &mut Document) -> Result<()> {
        if doc.text_blocks.is_empty() {
            return Ok(());
        }

        let mut regions = Vec::new();
        let mut block_indices = Vec::new();
        for (block_index, block) in doc.text_blocks.iter().enumerate() {
            for region in extract_text_block_regions(&doc.image, block) {
                regions.push(region);
                block_indices.push(block_index);
            }
        }

        if regions.is_empty() {
            return Ok(());
        }

        let mut ocr = self
            .ocr
            .lock()
            .map_err(|_| anyhow::anyhow!("PaddleOCR-VL mutex poisoned"))?;
        let outputs = ocr.inference_images(&regions, PaddleOcrVlTask::Ocr, OCR_MAX_NEW_TOKENS)?;

        let mut grouped = vec![Vec::<String>::new(); doc.text_blocks.len()];
        for (output, block_index) in outputs.into_iter().zip(block_indices) {
            grouped[block_index].push(output.text);
        }

        for (block_index, texts) in grouped.into_iter().enumerate() {
            if let Some(block) = doc.text_blocks.get_mut(block_index) {
                block.text = Some(join_ocr_texts(&texts));
            }
        }

        Ok(())
    }

    /// Inpaint text regions in the document.
    /// Uses the current `doc.segment` mask as the inpaint source, sets `doc.inpainted`.
    pub async fn inpaint(&self, doc: &mut Document) -> Result<()> {
        let mask = doc
            .segment
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Segment image not found"))?;
        let result = self
            .lama
            .inference_with_blocks(&doc.image, mask, Some(&doc.text_blocks))?;
        doc.inpainted = Some(result.into());

        Ok(())
    }

    /// Low-level inpaint: inpaint a specific image region with a mask.
    pub async fn inpaint_raw(
        &self,
        image: &SerializableDynamicImage,
        mask: &SerializableDynamicImage,
        text_blocks: Option<&[koharu_types::TextBlock]>,
    ) -> Result<SerializableDynamicImage> {
        let result = self.lama.inference_with_blocks(image, mask, text_blocks)?;
        Ok(result.into())
    }

    pub async fn detect_font(&self, image: &DynamicImage, top_k: usize) -> Result<FontPrediction> {
        let mut results = self
            .detect_fonts(std::slice::from_ref(image), top_k)
            .await?;
        Ok(results.pop().unwrap_or_default())
    }

    pub async fn detect_fonts(
        &self,
        images: &[DynamicImage],
        top_k: usize,
    ) -> Result<Vec<FontPrediction>> {
        if images.is_empty() {
            return Ok(Vec::new());
        }

        let mut predictions = self.font_detector.inference(images, top_k)?;
        for prediction in &mut predictions {
            normalize_font_prediction(prediction);
        }
        Ok(predictions)
    }
}

pub async fn prefetch() -> Result<()> {
    pp_doclayout_v3::prefetch().await?;
    manga_text_segmentation_2025::prefetch().await?;
    paddleocr_vl::prefetch().await?;
    lama::prefetch().await?;
    font_detector::prefetch().await?;

    Ok(())
}

fn build_text_blocks(regions: &[LayoutRegion]) -> Vec<TextBlock> {
    let mut blocks = regions
        .iter()
        .filter(|region| is_text_layout_label(&region.label))
        .filter_map(layout_region_to_text_block)
        .collect::<Vec<_>>();
    dedupe_text_blocks(&mut blocks);
    blocks
}

fn is_text_layout_label(label: &str) -> bool {
    let label = label.to_ascii_lowercase();
    label == "content" || label.contains("text") || label.contains("title")
}

fn layout_region_to_text_block(region: &LayoutRegion) -> Option<TextBlock> {
    let x1 = region.bbox[0].min(region.bbox[2]).max(0.0);
    let y1 = region.bbox[1].min(region.bbox[3]).max(0.0);
    let x2 = region.bbox[0].max(region.bbox[2]).max(x1 + 1.0);
    let y2 = region.bbox[1].max(region.bbox[3]).max(y1 + 1.0);
    let width = (x2 - x1).max(1.0);
    let height = (y2 - y1).max(1.0);

    if width < 6.0 || height < 6.0 || width * height < 48.0 {
        return None;
    }

    let source_direction = infer_text_direction(width, height);
    let line_quad = [[x1, y1], [x2, y1], [x2, y2], [x1, y2]];

    Some(TextBlock {
        x: x1,
        y: y1,
        width,
        height,
        confidence: region.score,
        line_polygons: Some(vec![line_quad]),
        source_direction: Some(source_direction),
        source_language: Some("unknown".to_string()),
        rotation_deg: Some(0.0),
        detected_font_size_px: Some(width.min(height).max(1.0)),
        detector: Some("pp-doclayout-v3".to_string()),
        ..Default::default()
    })
}

fn infer_text_direction(width: f32, height: f32) -> TextDirection {
    if height >= width * VERTICAL_ASPECT_RATIO_THRESHOLD {
        TextDirection::Vertical
    } else {
        TextDirection::Horizontal
    }
}

fn dedupe_text_blocks(blocks: &mut Vec<TextBlock>) {
    if blocks.len() < 2 {
        return;
    }

    let mut deduped = Vec::with_capacity(blocks.len());
    for block in std::mem::take(blocks) {
        let area = (block.width * block.height).max(1.0);
        let overlaps_existing = deduped.iter().any(|existing: &TextBlock| {
            let existing_area = (existing.width * existing.height).max(1.0);
            let overlap = overlap_area(block_bbox(&block), block_bbox(existing));
            overlap / area >= BLOCK_OVERLAP_DEDUPE_THRESHOLD
                || overlap / existing_area >= BLOCK_OVERLAP_DEDUPE_THRESHOLD
        });
        if !overlaps_existing {
            deduped.push(block);
        }
    }
    *blocks = deduped;
}

fn block_bbox(block: &TextBlock) -> [f32; 4] {
    [
        block.x,
        block.y,
        block.x + block.width,
        block.y + block.height,
    ]
}

fn overlap_area(a: [f32; 4], b: [f32; 4]) -> f32 {
    let x1 = a[0].max(b[0]);
    let y1 = a[1].max(b[1]);
    let x2 = a[2].min(b[2]);
    let y2 = a[3].min(b[3]);
    if x2 <= x1 || y2 <= y1 {
        0.0
    } else {
        (x2 - x1) * (y2 - y1)
    }
}

fn build_segment_mask(probability_map: &ProbabilityMap, blocks: &[TextBlock]) -> Result<GrayImage> {
    let thresholded = probability_map.threshold(SEGMENTATION_THRESHOLD)?;
    if blocks.is_empty() {
        return Ok(thresholded);
    }

    let mut filtered = GrayImage::new(thresholded.width(), thresholded.height());
    let max_x = thresholded.width().saturating_sub(1) as f32;
    let max_y = thresholded.height().saturating_sub(1) as f32;
    for block in blocks {
        let pad_x = (block.width * BLOCK_MASK_PADDING_RATIO).max(BLOCK_MASK_MIN_PADDING);
        let pad_y = (block.height * BLOCK_MASK_PADDING_RATIO).max(BLOCK_MASK_MIN_PADDING);
        let x1 = (block.x - pad_x).floor().clamp(0.0, max_x) as u32;
        let y1 = (block.y - pad_y).floor().clamp(0.0, max_y) as u32;
        let x2 = (block.x + block.width + pad_x)
            .ceil()
            .clamp(x1 as f32 + 1.0, thresholded.width() as f32) as u32;
        let y2 = (block.y + block.height + pad_y)
            .ceil()
            .clamp(y1 as f32 + 1.0, thresholded.height() as f32) as u32;

        for y in y1..y2 {
            for x in x1..x2 {
                let pixel = thresholded.get_pixel(x, y);
                if pixel[0] > 0 {
                    filtered.put_pixel(x, y, *pixel);
                }
            }
        }
    }

    if filtered.pixels().any(|pixel| pixel[0] > 0) {
        Ok(filtered)
    } else {
        Ok(thresholded)
    }
}

fn join_ocr_texts(texts: &[String]) -> String {
    texts
        .iter()
        .map(|text| normalize_ocr_text(text))
        .collect::<Vec<_>>()
        .join("")
}

fn normalize_ocr_text(text: &str) -> String {
    text.chars()
        .filter(|&ch| ch != '\n' && ch != '\r')
        .collect()
}

#[cfg(test)]
mod tests {
    use image::Luma;

    use super::*;

    fn test_region(order: usize, label: &str, bbox: [f32; 4]) -> LayoutRegion {
        LayoutRegion {
            order,
            label_id: 0,
            label: label.to_string(),
            score: 0.9,
            bbox,
            polygon_points: vec![],
        }
    }

    #[test]
    fn build_text_blocks_keeps_textlike_regions_and_dedupes_overlaps() {
        let blocks = build_text_blocks(&[
            test_region(0, "text", [10.0, 10.0, 40.0, 40.0]),
            test_region(1, "image", [0.0, 0.0, 128.0, 128.0]),
            test_region(2, "aside_text", [12.0, 12.0, 39.0, 39.0]),
            test_region(3, "doc_title", [60.0, 8.0, 90.0, 24.0]),
        ]);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].detector.as_deref(), Some("pp-doclayout-v3"));
        assert_eq!(blocks[0].line_polygons.as_ref().map(Vec::len), Some(1));
        assert_eq!(blocks[1].source_direction, Some(TextDirection::Horizontal));
    }

    #[test]
    fn build_text_blocks_marks_tall_regions_as_vertical() {
        let blocks = build_text_blocks(&[test_region(0, "text", [5.0, 5.0, 20.0, 60.0])]);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].source_direction, Some(TextDirection::Vertical));
    }

    #[test]
    fn build_segment_mask_prefers_pixels_within_detected_blocks() -> Result<()> {
        let probability_map = ProbabilityMap {
            width: 8,
            height: 8,
            values: (0..64)
                .map(|index| {
                    let x = index % 8;
                    let y = index / 8;
                    if (2..5).contains(&x) && (2..5).contains(&y) {
                        1.0
                    } else {
                        0.0
                    }
                })
                .collect(),
        };
        let blocks = vec![TextBlock {
            x: 2.0,
            y: 2.0,
            width: 3.0,
            height: 3.0,
            ..Default::default()
        }];

        let mask = build_segment_mask(&probability_map, &blocks)?;
        assert_eq!(mask.get_pixel(0, 0), &Luma([0]));
        assert_eq!(mask.get_pixel(3, 3), &Luma([255]));
        Ok(())
    }

    #[test]
    fn normalize_ocr_text_removes_newlines() {
        assert_eq!(
            join_ocr_texts(&["ab\n".to_string(), "c\r\nd".to_string()]),
            "abcd"
        );
    }
}
