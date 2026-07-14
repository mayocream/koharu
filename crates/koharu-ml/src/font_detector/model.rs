//! Inference-only port of YuzuMarker's `ResNet50Regressor`.
//!
//! Original implementation:
//! https://github.com/JeffersonQin/YuzuMarker.FontDetection/blob/0a94e165fe2b08d2800b723290eabd120b2d3d58/detector/model.py

use std::path::Path;

use anyhow::Result;
use koharu_torch::{
    Device, Tensor,
    nn::{self, ModuleT},
};

use super::processor::{FONT_COUNT, OUTPUT_DIM, REGRESSION_DIM};

#[derive(Debug)]
pub(super) struct Model {
    vs: nn::VarStore,
    model: Box<dyn ModuleT>,
}

impl Model {
    pub fn new(device: Device) -> Self {
        let mut vs = nn::VarStore::new(device);
        // The published safetensors retain Lightning's `model` wrapper and
        // torch.compile's `_orig_mod` wrapper from the upstream checkpoint.
        let model = koharu_torch::vision::resnet::resnet50(
            &(&vs.root() / "model" / "_orig_mod" / "model"),
            OUTPUT_DIM,
        );
        vs.freeze();
        Self {
            vs,
            model: Box::new(model),
        }
    }

    pub fn load_weights(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.vs.load(path)?;
        Ok(())
    }

    pub fn forward(&self, pixel_values: &Tensor) -> Tensor {
        let output = self.model.forward_t(pixel_values, false);
        // Upstream leaves classification logits untouched and applies sigmoid
        // only to the ten regression values.
        let classification = output.narrow(-1, 0, FONT_COUNT + 2);
        let regression = output.narrow(-1, FONT_COUNT + 2, REGRESSION_DIM).sigmoid();
        Tensor::cat(&[classification, regression], -1)
    }
}
