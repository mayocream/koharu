use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use comic_text_detector::ComicTextDetector;
use lama::Lama;
use manga_ocr::MangaOCR;

use crate::image::SerializableDynamicImage;
use crate::state::TextBlock;

pub struct Inference {
    detector: Arc<Mutex<ComicTextDetector>>,
    ocr: Arc<Mutex<MangaOCR>>,
    lama: Arc<Mutex<Lama>>,
}

impl Inference {
    pub fn new() -> Result<Self> {
        Ok(Self {
            detector: Arc::new(Mutex::new(ComicTextDetector::new()?)),
            ocr: Arc::new(Mutex::new(MangaOCR::new()?)),
            lama: Arc::new(Mutex::new(Lama::new()?)),
        })
    }

    pub fn detect(
        &self,
        image: &SerializableDynamicImage,
    ) -> Result<(Vec<TextBlock>, SerializableDynamicImage)> {
        let mut detector = self.detector.lock().unwrap();
        let result = detector.inference(image, 0.5, 0.5)?;

        let mut text_blocks: Vec<TextBlock> = result
            .bboxes
            .into_iter()
            .map(|bbox| TextBlock {
                x: bbox.xmin.round() as u32,
                y: bbox.ymin.round() as u32,
                width: (bbox.xmax - bbox.xmin).round() as u32,
                height: (bbox.ymax - bbox.ymin).round() as u32,
                confidence: bbox.confidence,
                ..Default::default()
            })
            .collect();

        text_blocks.sort_unstable_by(|a, b| {
            (a.y + a.height / 2)
                .partial_cmp(&(b.y + b.height / 2))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok((text_blocks, result.segment.into()))
    }

    pub fn ocr(
        &self,
        image: &SerializableDynamicImage,
        blocks: &[TextBlock],
    ) -> Result<Vec<TextBlock>> {
        let mut ocr = self.ocr.lock().unwrap();

        blocks
            .iter()
            .map(|block| {
                let crop = image.crop_imm(block.x, block.y, block.width, block.height);
                let text = ocr.inference(&crop)?;

                Ok(TextBlock {
                    text: text.into(),
                    x: block.x,
                    y: block.y,
                    width: block.width,
                    height: block.height,
                    confidence: block.confidence,
                    translation: block.translation.clone(),
                })
            })
            .collect()
    }

    pub fn inpaint(
        &self,
        image: &SerializableDynamicImage,
        mask: &SerializableDynamicImage,
    ) -> Result<SerializableDynamicImage> {
        let mut lama = self.lama.lock().unwrap();
        let result = lama.inference(image, mask)?;

        Ok(result.into())
    }
}
