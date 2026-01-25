use anyhow::Result;
use image::DynamicImage;
use koharu_ml::comic_text_detector::ComicTextDetector;
use koharu_ml::font_detector::{self, FontDetector};
use koharu_ml::lama::Lama;
use koharu_ml::manga_ocr::MangaOcr;

use crate::image::SerializableDynamicImage;
use crate::state::TextBlock;

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

    pub async fn detect_dialog(
        &self,
        image: &SerializableDynamicImage,
    ) -> Result<(Vec<TextBlock>, SerializableDynamicImage)> {
        let (bboxes, segment) = self.dialog_detector.inference(image)?;

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

        Ok((text_blocks, DynamicImage::ImageLuma8(segment).into()))
    }

    pub async fn ocr(
        &self,
        image: &SerializableDynamicImage,
        blocks: &[TextBlock],
    ) -> Result<Vec<TextBlock>> {
        if blocks.is_empty() {
            return Ok(Vec::new());
        }

        let crops: Vec<DynamicImage> = blocks
            .iter()
            .map(|block| {
                image.crop_imm(
                    block.x as u32,
                    block.y as u32,
                    block.width as u32,
                    block.height as u32,
                )
            })
            .collect();
        let texts = self.ocr.inference(&crops)?;

        Ok(blocks
            .iter()
            .cloned()
            .zip(texts.into_iter())
            .map(|(block, text)| TextBlock {
                text: text.into(),
                ..block
            })
            .collect())
    }

    pub async fn inpaint(
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
