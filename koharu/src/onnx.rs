use std::sync::Arc;

use anyhow::Result;
use koharu_core::image::SerializableDynamicImage;
use koharu_models::comic_text_detector::ComicTextDetector;
use koharu_models::device;
use koharu_models::lama::Lama;
use koharu_models::manga_ocr::MangaOcr;
use tokio::sync::Mutex;

use crate::state::TextBlock;

#[derive(Debug, Clone)]
pub struct Model {
    detector: Arc<Mutex<ComicTextDetector>>,
    ocr: Arc<Mutex<MangaOcr>>,
    lama: Arc<Mutex<Lama>>,
}

impl Model {
    pub async fn new() -> Result<Self> {
        let device = device(false)?;
        Ok(Self {
            detector: Arc::new(Mutex::new(ComicTextDetector::load(device.clone()).await?)),
            ocr: Arc::new(Mutex::new(MangaOcr::load(device.clone()).await?)),
            lama: Arc::new(Mutex::new(Lama::load(device).await?)),
        })
    }

    pub async fn detect(
        &self,
        image: &SerializableDynamicImage,
        conf_threshold: f32,
        nms_threshold: f32,
    ) -> Result<(Vec<TextBlock>, SerializableDynamicImage)> {
        let mut detector = self.detector.lock().await;
        let result = detector.inference(image, conf_threshold, nms_threshold)?;

        let mut text_blocks: Vec<TextBlock> = result
            .bboxes
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

        Ok((text_blocks, result.segment.into()))
    }

    pub async fn ocr(
        &self,
        image: &SerializableDynamicImage,
        blocks: &[TextBlock],
    ) -> Result<Vec<TextBlock>> {
        let ocr = self.ocr.lock().await;

        blocks
            .iter()
            .map(|block| {
                let crop = image.crop_imm(
                    block.x as u32,
                    block.y as u32,
                    block.width as u32,
                    block.height as u32,
                );
                let text = ocr.infer(&crop)?;

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
        dilate_kernel_size: u8,
        erode_distance: u8,
    ) -> Result<SerializableDynamicImage> {
        let mask = imageproc::morphology::grayscale_dilate(
            &mask.to_luma8(),
            &imageproc::morphology::Mask::square(dilate_kernel_size),
        );
        let mask = imageproc::morphology::erode(
            &mask,
            imageproc::distance_transform::Norm::L2,
            erode_distance,
        );

        let lama = self.lama.lock().await;
        let result = lama.inference(image, &image::DynamicImage::ImageLuma8(mask))?;

        Ok(result.into())
    }
}
