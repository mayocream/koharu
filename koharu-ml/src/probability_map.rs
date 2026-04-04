use anyhow::{Context, Result};
use image::GrayImage;

#[derive(Debug, Clone)]
pub struct ProbabilityMap {
    pub width: u32,
    pub height: u32,
    pub values: Vec<f32>,
}

impl ProbabilityMap {
    pub fn zeros(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            values: vec![0.0; (width * height) as usize],
        }
    }

    pub fn to_gray_image(&self) -> Result<GrayImage> {
        let bytes = self
            .values
            .iter()
            .copied()
            .map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect::<Vec<_>>();
        GrayImage::from_raw(self.width, self.height, bytes)
            .context("failed to build probability map image")
    }

    pub fn threshold(&self, threshold: f32) -> Result<GrayImage> {
        let bytes = self
            .values
            .iter()
            .copied()
            .map(|value| if value >= threshold { 255 } else { 0 })
            .collect::<Vec<_>>();
        GrayImage::from_raw(self.width, self.height, bytes).context("failed to build mask image")
    }

    pub fn max_value(&self) -> f32 {
        self.values.iter().copied().fold(0.0, f32::max)
    }
}
