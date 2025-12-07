use anyhow::Result;
use futures::try_join;
use image::DynamicImage;
use koharu_ml::comic_text_detector::{self, ComicTextDetector};
use koharu_ml::device;
use koharu_ml::lama::{self, Lama};
use koharu_ml::manga_ocr::{self, MangaOcr};

use crate::image::SerializableDynamicImage;
use crate::state::TextBlock;

pub struct Model {
    detector: ComicTextDetector,
    ocr: MangaOcr,
    lama: Lama,
}

impl Model {
    pub async fn new() -> Result<Self> {
        let device = device(false)?;
        Ok(Self {
            detector: ComicTextDetector::load(device.clone()).await?,
            ocr: MangaOcr::load(device.clone()).await?,
            lama: Lama::load(device.clone()).await?,
        })
    }

    pub async fn detect(
        &self,
        image: &SerializableDynamicImage,
    ) -> Result<(Vec<TextBlock>, SerializableDynamicImage)> {
        let (bboxes, segment) = self.detector.inference(image)?;

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
        blocks
            .iter()
            .map(|block| {
                let crop = image.crop_imm(
                    block.x as u32,
                    block.y as u32,
                    block.width as u32,
                    block.height as u32,
                );
                let text = self.ocr.inference(&crop)?;

                Ok(TextBlock {
                    text: text.into(),
                    ..block.clone()
                })
            })
            .collect()
    }

    pub async fn inpaint(
        &self,
        image: &SerializableDynamicImage,
        mask: &SerializableDynamicImage,
    ) -> Result<SerializableDynamicImage> {
        let result = self.lama.inference(image, mask)?;

        Ok(result.into())
    }
}

pub async fn prefetch() -> Result<()> {
    try_join!(
        comic_text_detector::prefetch(),
        manga_ocr::prefetch(),
        lama::prefetch(),
    )?;

    Ok(())
}
