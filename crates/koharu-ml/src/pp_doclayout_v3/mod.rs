mod config;
mod model;
mod processor;

use anyhow::{Context, Result};
use image::DynamicImage;
use koharu_runtime::package::huggingface;
use koharu_torch::Device;

pub use self::{
    config::{HGNetV2Config, PPDocLayoutV3Config},
    processor::{
        PPDocLayoutV3Detections, PPDocLayoutV3ImageProcessor, PPDocLayoutV3Region, SizeDict,
    },
};

use self::model::Model;

koharu_runtime::huggingface! {
    CONFIG => "PaddlePaddle/PP-DocLayoutV3_safetensors" => "config.json",
    WEIGHTS => "PaddlePaddle/PP-DocLayoutV3_safetensors" => "model.safetensors",
    PROCESSOR => "PaddlePaddle/PP-DocLayoutV3_safetensors" => "preprocessor_config.json",
}

#[derive(Debug)]
pub struct PPDocLayoutV3 {
    device: Device,
    model: Model,
    processor: PPDocLayoutV3ImageProcessor,
}

impl PPDocLayoutV3 {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let device: Device = device.try_into()?;

        let config_path = huggingface::resolve(CONFIG)
            .await
            .context("failed to resolve PP-DocLayout-V3 config")?;
        let weights_path = huggingface::resolve(WEIGHTS)
            .await
            .context("failed to resolve PP-DocLayout-V3 weights")?;
        let processor_path = huggingface::resolve(PROCESSOR)
            .await
            .context("failed to resolve PP-DocLayout-V3 image processor")?;

        let mut config = PPDocLayoutV3Config::from_file(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let labels = config.labels();
        let processor = PPDocLayoutV3ImageProcessor::from_file(&processor_path)
            .with_context(|| format!("failed to read {}", processor_path.display()))?
            .with_labels(labels);

        // The processor always produces this resolution, so the model can reuse the
        // fixed RT-DETR anchors instead of rebuilding and uploading them per page.
        config.anchor_image_size = Some(vec![processor.size.height, processor.size.width]);

        let mut model = Model::new(config, device);
        model
            .load_safetensors(&weights_path)
            .with_context(|| format!("failed to load {}", weights_path.display()))?;

        Ok(Self {
            device,
            model,
            processor,
        })
    }

    pub fn inference(
        &self,
        image: &DynamicImage,
        threshold: f32,
    ) -> Result<PPDocLayoutV3Detections> {
        koharu_torch::no_grad(|| {
            let pixel_values = self.processor.preprocess(image, self.device)?;
            let outputs = self.model.forward(&pixel_values);
            self.processor.postprocess(&outputs, image, threshold)
        })
    }
}
