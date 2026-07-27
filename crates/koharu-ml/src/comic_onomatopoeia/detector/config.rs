//! MTSv3 settings from COO's reported-best test configuration.
//!
//! https://github.com/ku21fan/COO-Comic-Onomatopoeia/blob/d8028f015b8ce99a4dd798427342f97087529357/MTSv3/configs/best_test.yaml

#[derive(Debug, Clone, Copy)]
pub(super) struct Config {
    pub(super) min_size: u32,
    pub(super) max_size: u32,
    pub(super) size_divisibility: u32,
    pub(super) pixel_mean: [f32; 3],
    pub(super) binary_threshold: f32,
    pub(super) box_threshold: f32,
    pub(super) minimum_size: f32,
    pub(super) polygon_expand_ratio: f32,
    pub(super) box_expand_ratio: f32,
    pub(super) top_n: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            min_size: 1440,
            max_size: 4000,
            size_divisibility: 32,
            pixel_mean: [102.9801, 115.9465, 122.7717],
            binary_threshold: 0.1,
            box_threshold: 0.1,
            minimum_size: 5.0,
            polygon_expand_ratio: 3.0,
            box_expand_ratio: 1.5,
            top_n: 1000,
        }
    }
}
