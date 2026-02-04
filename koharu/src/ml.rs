use anyhow::Result;
use image::{DynamicImage, Rgba};
use koharu_ml::comic_text_detector::{self, ComicTextDetector};
use koharu_ml::font_detector::{self, FontDetector};
use koharu_ml::lama::{self, Lama};
use koharu_ml::manga_ocr::{self, MangaOcr};

use crate::image::SerializableDynamicImage;
use crate::state::{Document, TextBlock, TextStyle};

const NEAR_BLACK_THRESHOLD: u8 = 12;
const GRAY_NEAR_BLACK_THRESHOLD: u8 = 60;
const NEAR_WHITE_THRESHOLD: u8 = 12;
const GRAY_NEAR_WHITE_THRESHOLD: u8 = 60;
const GRAY_TOLERANCE: u8 = 10;
const SIMILAR_COLOR_MAX_DIFF: u8 = 16;

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

fn normalize_font_prediction(prediction: &mut font_detector::FontPrediction) {
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
    dialog_detector: ComicTextDetector,
    ocr: MangaOcr,
    lama: Lama,
    font_detector: FontDetector,
}

impl Model {
    pub async fn new(use_cpu: bool) -> Result<Self> {
        Ok(Self {
            dialog_detector: ComicTextDetector::load(use_cpu).await?,
            ocr: MangaOcr::load(use_cpu).await?,
            lama: Lama::load(use_cpu).await?,
            font_detector: FontDetector::load(use_cpu).await?,
        })
    }

    /// Detect text blocks and fonts in a document.
    /// Sets `doc.text_blocks` (with font predictions/styles) and `doc.segment`.
    pub async fn detect(&self, doc: &mut Document) -> Result<()> {
        let (bboxes, segment) = self.dialog_detector.inference(&doc.image)?;

        let mut text_blocks: Vec<TextBlock> = bboxes
            .into_iter()
            .map(|bbox| TextBlock {
                x: bbox.xmin,
                y: bbox.ymin,
                width: bbox.xmax - bbox.xmin,
                height: bbox.ymax - bbox.ymin,
                confidence: bbox.confidence,
                ..Default::default()
            })
            .collect();

        text_blocks.sort_unstable_by(|a, b| {
            (a.y + a.height / 2.0)
                .partial_cmp(&(b.y + b.height / 2.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        doc.text_blocks = text_blocks;
        doc.segment = Some(DynamicImage::ImageLuma8(segment).into());

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
                let color = prediction.text_color;
                let font_size = (prediction.font_size_px > 0.0).then_some(prediction.font_size_px);
                block.font_prediction = Some(prediction);
                block.style = Some(TextStyle {
                    font_size,
                    color: [color[0], color[1], color[2], 255],
                    ..Default::default()
                });
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

        let crops: Vec<DynamicImage> = doc
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
        let texts = self.ocr.inference(&crops)?;

        for (block, text) in doc.text_blocks.iter_mut().zip(texts) {
            block.text = text.into();
        }

        Ok(())
    }

    /// Inpaint text regions in the document.
    /// Builds mask from `doc.segment` + `doc.text_blocks`, sets `doc.inpainted`.
    pub async fn inpaint(&self, doc: &mut Document) -> Result<()> {
        let segment = doc
            .segment
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Segment image not found"))?;
        let mut segment_data = segment.to_rgba8();
        let (seg_width, seg_height) = segment_data.dimensions();

        for y in 0..seg_height {
            for x in 0..seg_width {
                let pixel = segment_data.get_pixel_mut(x, y);
                if pixel.0 != [0, 0, 0, 255] {
                    let inside_any_block = doc.text_blocks.iter().any(|block| {
                        x >= block.x as u32
                            && x < (block.x + block.width) as u32
                            && y >= block.y as u32
                            && y < (block.y + block.height) as u32
                    });
                    if !inside_any_block {
                        *pixel = Rgba([0, 0, 0, 255]);
                    }
                }
            }
        }

        let mask = SerializableDynamicImage::from(DynamicImage::ImageRgba8(segment_data));
        let result = self.lama.inference(&doc.image, &mask)?;
        doc.inpainted = Some(result.into());

        Ok(())
    }

    /// Low-level inpaint: inpaint a specific image region with a mask.
    pub async fn inpaint_raw(
        &self,
        image: &SerializableDynamicImage,
        mask: &SerializableDynamicImage,
    ) -> Result<SerializableDynamicImage> {
        let result = self.lama.inference(image, mask)?;
        Ok(result.into())
    }

    pub async fn detect_font(
        &self,
        image: &DynamicImage,
        top_k: usize,
    ) -> Result<font_detector::FontPrediction> {
        let mut results = self
            .detect_fonts(std::slice::from_ref(image), top_k)
            .await?;
        Ok(results.pop().unwrap_or_default())
    }

    pub async fn detect_fonts(
        &self,
        images: &[DynamicImage],
        top_k: usize,
    ) -> Result<Vec<font_detector::FontPrediction>> {
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
    comic_text_detector::prefetch().await?;
    manga_ocr::prefetch().await?;
    lama::prefetch().await?;
    font_detector::prefetch().await?;

    Ok(())
}
