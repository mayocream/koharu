#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VerticalSlice {
    pub y: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct VerticalSlicer {
    pub aspect_ratio_threshold: f32,
    pub target_slice_ratio: f32,
    pub overlap_height_ratio: f32,
    pub min_last_slice_ratio: f32,
}

const MIN_SLICE_HEIGHT: u32 = 256;

impl Default for VerticalSlicer {
    fn default() -> Self {
        Self {
            aspect_ratio_threshold: 3.5,
            target_slice_ratio: 2.0,
            overlap_height_ratio: 0.2,
            min_last_slice_ratio: 0.7,
        }
    }
}

impl VerticalSlicer {
    pub(crate) fn is_tall(self, width: u32, height: u32) -> bool {
        width > 0 && height > 0 && height as f32 / width as f32 > self.aspect_ratio_threshold
    }

    /// Returns vertical slices only when the image is tall enough and splitting
    /// would produce more than one useful slice after short-tail trimming.
    pub(crate) fn slices(self, width: u32, height: u32) -> Option<Vec<VerticalSlice>> {
        if !self.is_tall(width, height) {
            return None;
        }

        let slice_height = (width as f32 * self.target_slice_ratio)
            .round()
            .max(MIN_SLICE_HEIGHT as f32) as u32;
        if slice_height >= height {
            return None;
        }

        let effective_slice_height = (slice_height as f32 * (1.0 - self.overlap_height_ratio))
            .round()
            .max(1.0) as u32;
        let mut num_slices = height.div_ceil(effective_slice_height) as usize;
        if num_slices > 1 {
            let last_slice_start = (num_slices as u32 - 1) * effective_slice_height;
            let last_slice_height = height.saturating_sub(last_slice_start);
            if last_slice_height as f32 / slice_height as f32 <= self.min_last_slice_ratio {
                num_slices -= 1;
            }
        }

        debug_assert!(num_slices >= 1);

        let mut slices = Vec::with_capacity(num_slices);
        for slice_number in 0..num_slices {
            let start_y = slice_number as u32 * effective_slice_height;
            if start_y >= height {
                break;
            }
            let end_y = if slice_number + 1 == num_slices {
                height
            } else {
                (start_y + slice_height).min(height)
            };
            let crop_height = end_y.saturating_sub(start_y);
            if crop_height > 0 {
                slices.push(VerticalSlice {
                    y: start_y,
                    height: crop_height,
                });
            }
        }

        (slices.len() > 1).then_some(slices)
    }
}

#[cfg(test)]
mod tests {
    use super::{VerticalSlice, VerticalSlicer};

    #[test]
    fn slices_only_very_tall_images() {
        let slicer = VerticalSlicer::default();

        assert!(slicer.slices(1000, 2000).is_none());
        assert!(slicer.slices(1000, 5000).is_some());
    }

    #[test]
    fn slices_overlap_and_cover_full_height() {
        let slicer = VerticalSlicer::default();
        let slices = slicer.slices(100, 1000).expect("tall image");

        assert_eq!(slices[0], VerticalSlice { y: 0, height: 256 });
        assert_eq!(
            slices.last().unwrap().y + slices.last().unwrap().height,
            1000
        );
        for pair in slices.windows(2) {
            assert!(pair[0].y + pair[0].height > pair[1].y);
        }
    }
}
