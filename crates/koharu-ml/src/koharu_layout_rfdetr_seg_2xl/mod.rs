//! High-resolution manga layout instance segmentation with RF-DETR Seg 2XL.
//!
//! Checkpoint and strict Python loader:
//! https://huggingface.co/mayocream/koharu-layout-rfdetr-seg-2xl-1152/tree/aed55fdb8ca953c6bec33cf6ed6dd52a9b72bfa2
//! RF-DETR 1.7.0 implementation:
//! https://github.com/roboflow/rf-detr/tree/e77de6698d69d09cd9abf2597e2e9a576169a119

mod config;
mod model;
mod processor;

use anyhow::{Context, Result};
use image::DynamicImage;
use koharu_torch::Device;

use crate::backend::TryIntoDevice;

pub use self::{
    config::{KoharuLayoutRFDetrSeg2XLConfig, KoharuLayoutThresholds},
    processor::{
        KoharuLayoutDetection, KoharuLayoutDetections, KoharuLayoutMask,
        KoharuLayoutRFDetrImageProcessor,
    },
};

use self::model::Model;

crate::model_repository!("mayocream/koharu-layout-rfdetr-seg-2xl-1152" @ "aed55fdb8ca953c6bec33cf6ed6dd52a9b72bfa2" {
    CONFIG = "inference_config.json",
    WEIGHTS = "model.safetensors",
});

#[derive(Debug)]
pub struct KoharuLayoutRFDetrSeg2XL {
    device: Device,
    model: Model,
    processor: KoharuLayoutRFDetrImageProcessor,
}

impl KoharuLayoutRFDetrSeg2XL {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let device: Device = device.try_into_device()?;
        let config_path = CONFIG
            .resolve()
            .await
            .context("failed to resolve KoharuLayout RF-DETR inference config")?;
        let weights_path = WEIGHTS
            .resolve()
            .await
            .context("failed to resolve KoharuLayout RF-DETR weights")?;
        let config = KoharuLayoutRFDetrSeg2XLConfig::from_file(&config_path)?;
        let processor = KoharuLayoutRFDetrImageProcessor::new(&config)?;
        let mut model = Model::new(device);
        model
            .load(&weights_path)
            .with_context(|| format!("failed to load {}", weights_path.display()))?;
        Ok(Self {
            device,
            model,
            processor,
        })
    }

    pub fn inference(&self, image: &DynamicImage) -> Result<KoharuLayoutDetections> {
        self.inference_with_thresholds(image, self.processor.recommended_thresholds())
    }

    pub fn inference_with_thresholds(
        &self,
        image: &DynamicImage,
        thresholds: KoharuLayoutThresholds,
    ) -> Result<KoharuLayoutDetections> {
        koharu_torch::no_grad(|| {
            let pixel_values = self.processor.preprocess(image, self.device)?;
            let output = self.model.forward(&pixel_values);
            self.processor
                .postprocess(&output, image.width(), image.height(), thresholds)
        })
    }

    pub fn recommended_thresholds(&self) -> KoharuLayoutThresholds {
        self.processor.recommended_thresholds()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use anyhow::Result;

    use super::KoharuLayoutRFDetrSeg2XL;

    #[tokio::test]
    #[ignore = "downloads the checkpoint and requires CUDA"]
    async fn checkpoint_matches_rfdetr_1_7_structured_output() -> Result<()> {
        crate::init().await?;
        let image = image::open(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("benches/fixtures/object_detection/1.jpg"),
        )?;
        let model = KoharuLayoutRFDetrSeg2XL::load(crate::Device::cuda(0)).await?;
        let result = model.inference(&image)?;

        // RF-DETR 1.7.0, CUDA BF16, shape=(1152, 1152). SDPA kernels vary slightly
        // across LibTorch releases, so compare stable structured geometry and
        // mask area in addition to bounded confidence differences.
        let best = &result.detections[0];
        assert_eq!(best.label, "bubble");
        assert!((best.score - 0.972_656_25).abs() < 0.01);
        for (actual, expected) in best
            .bbox
            .into_iter()
            .zip([571.484_4, 548.437_5, 691.796_9, 717.187_5])
        {
            assert!((actual - expected).abs() < 0.5);
        }
        assert!(best.area.abs_diff(18_882) < 100);

        let lower_panel = result
            .detections
            .iter()
            .find(|detection| detection.label == "panel" && detection.bbox[1] > 700.0)
            .expect("lower page panel");
        let middle_panel = result
            .detections
            .iter()
            .find(|detection| detection.label == "panel" && detection.bbox[1] < 700.0)
            .expect("middle page panel");
        for (actual, expected) in lower_panel
            .bbox
            .into_iter()
            .zip([70.683_59, 801.562_5, 703.828_1, 1012.5])
        {
            assert!((actual - expected).abs() < 3.0);
        }
        for (actual, expected) in middle_panel
            .bbox
            .into_iter()
            .zip([70.683_59, 556.875, 697.812_5, 784.687_5])
        {
            assert!((actual - expected).abs() < 3.0);
        }
        assert!(lower_panel.area.abs_diff(136_941) < 1_000);
        assert!(middle_panel.area.abs_diff(142_957) < 1_000);
        Ok(())
    }
}
