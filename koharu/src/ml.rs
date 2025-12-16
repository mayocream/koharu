use anyhow::Result;
use image::DynamicImage;
use koharu_ml::comic_text_detector::{self, ComicTextDetector};
use koharu_ml::font_detector::{self, FontDetector};
use koharu_ml::lama::{self, Lama};
use koharu_ml::manga_ocr::{self, MangaOcr};

use crate::image::SerializableDynamicImage;
use crate::state::TextBlock;

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

        self.font_detector.inference(images, top_k)
    }
}

pub async fn prefetch() -> Result<()> {
    comic_text_detector::prefetch().await?;
    manga_ocr::prefetch().await?;
    lama::prefetch().await?;
    font_detector::prefetch().await?;

    Ok(())
}
