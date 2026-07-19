//! Architecture settings for the reported-best TRBA checkpoint.
//!
//! Upstream invocation:
//! https://github.com/ku21fan/COO-Comic-Onomatopoeia/blob/d8028f015b8ce99a4dd798427342f97087529357/TRBA/README.md#L48-L51

#[derive(Debug, Clone, Copy)]
pub(super) struct Config {
    pub(super) image_height: i64,
    pub(super) image_width: i64,
    pub(super) num_fiducial: i64,
    pub(super) input_channels: i64,
    pub(super) output_channels: i64,
    pub(super) hidden_size: i64,
    pub(super) num_classes: i64,
    pub(super) batch_max_length: i64,
    pub(super) two_dimensional: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            image_height: 100,
            image_width: 100,
            num_fiducial: 20,
            input_channels: 3,
            output_channels: 512,
            hidden_size: 256,
            num_classes: 187,
            batch_max_length: 25,
            two_dimensional: true,
        }
    }
}
