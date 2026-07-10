mod config;
mod model;
mod processor;

use anyhow::{Context, Result};
use image::DynamicImage;
use koharu_runtime::package::huggingface;
use koharu_torch::Device;

use crate::device;

pub use self::{
    config::{HGNetV2Config, PPDocLayoutV3Config},
    model::{PPDocLayoutV3ForObjectDetection, PPDocLayoutV3ForwardOutput},
    processor::{
        PPDocLayoutV3Detections, PPDocLayoutV3Processor, PPDocLayoutV3Region, ProcessorSize,
    },
};

pub type PPDocLayoutV3Output = PPDocLayoutV3Detections;

koharu_runtime::huggingface! {
    CONFIG => "PaddlePaddle/PP-DocLayoutV3_safetensors" => "config.json",
    WEIGHTS => "PaddlePaddle/PP-DocLayoutV3_safetensors" => "model.safetensors",
    PROCESSOR => "PaddlePaddle/PP-DocLayoutV3_safetensors" => "preprocessor_config.json",
}

#[derive(Debug)]
pub struct PPDocLayoutV3 {
    device: Device,
    model: PPDocLayoutV3ForObjectDetection,
    processor: PPDocLayoutV3Processor,
}

impl PPDocLayoutV3 {
    pub async fn load(cpu: bool) -> Result<Self> {
        let device: Device = device(cpu).try_into()?;

        let config_path = huggingface::resolve(CONFIG)
            .await
            .context("failed to resolve PP-DocLayout-V3 config")?;
        let weights_path = huggingface::resolve(WEIGHTS)
            .await
            .context("failed to resolve PP-DocLayout-V3 weights")?;
        let processor_path = huggingface::resolve(PROCESSOR).await;

        let config = PPDocLayoutV3Config::from_file(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let labels = config.labels();
        let processor = match processor_path {
            Ok(path) => PPDocLayoutV3Processor::from_file(&path)
                .with_context(|| format!("failed to read {}", path.display()))?
                .with_labels(labels),
            Err(_) => PPDocLayoutV3Processor::default().with_labels(labels),
        };

        let mut model = PPDocLayoutV3ForObjectDetection::new(config, device);
        model
            .load_safetensors(&weights_path)
            .with_context(|| format!("failed to load {}", weights_path.display()))?;

        Ok(Self {
            device,
            model,
            processor,
        })
    }

    pub fn inference(&self, image: &DynamicImage, threshold: f32) -> Result<PPDocLayoutV3Output> {
        koharu_torch::no_grad(|| {
            let pixel_values = self.processor.preprocess(image, self.device);
            let outputs = self.model.forward(&pixel_values);
            self.processor.postprocess(&outputs, image, threshold)
        })
    }

    pub fn device(&self) -> Device {
        self.device
    }
}
