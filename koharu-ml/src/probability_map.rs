use anyhow::{Context, Result, bail};
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

    pub fn stitch_max(&mut self, src: &ProbabilityMap, dst_y: u32) -> Result<()> {
        if self.width != src.width {
            bail!(
                "cannot stitch probability maps with different widths: {} vs {}",
                self.width,
                src.width
            );
        }

        let height = src.height.min(self.height.saturating_sub(dst_y));
        let width = self.width as usize;
        for y in 0..height as usize {
            let dst_row = (dst_y as usize + y) * width;
            let src_row = y * width;
            for x in 0..width {
                let src_value = src.values[src_row + x];
                let dst_value = &mut self.values[dst_row + x];
                if src_value > *dst_value {
                    *dst_value = src_value;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ProbabilityMap;

    #[test]
    fn stitch_max_uses_max_blend() -> anyhow::Result<()> {
        let mut dst = ProbabilityMap::zeros(2, 4);
        let src = ProbabilityMap {
            width: 2,
            height: 2,
            values: vec![0.1, 0.8, 0.4, 0.2],
        };
        dst.values[2] = 0.9;

        dst.stitch_max(&src, 1)?;

        assert_eq!(dst.values, vec![0.0, 0.0, 0.9, 0.8, 0.4, 0.2, 0.0, 0.0]);
        Ok(())
    }
}
