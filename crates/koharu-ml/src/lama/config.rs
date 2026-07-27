//! Configuration for the Big-LaMa generator and IOPaint inference orchestration.
//!
//! Original implementations:
//! https://github.com/advimman/lama/blob/786f5936b27fb3dacd2b1ad799e4de968ea697e7/configs/training/big-lama.yaml
//! https://github.com/Sanster/IOPaint/blob/61a759fb3f332bacdce8b2813f4837495c9b86e0/iopaint/schema.py#L206-L214

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HDStrategy {
    Original,
    Resize,
    #[default]
    Crop,
}

#[derive(Debug, Clone)]
pub struct InpaintRequest {
    pub hd_strategy: HDStrategy,
    pub hd_strategy_crop_trigger_size: u32,
    pub hd_strategy_crop_margin: u32,
    pub hd_strategy_resize_limit: u32,
    pub sd_keep_unmasked_area: bool,
}

impl Default for InpaintRequest {
    fn default() -> Self {
        Self {
            hd_strategy: HDStrategy::Crop,
            hd_strategy_crop_trigger_size: 800,
            hd_strategy_crop_margin: 128,
            hd_strategy_resize_limit: 1280,
            sd_keep_unmasked_area: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FFCConfig {
    pub ratio_gin: f64,
    pub ratio_gout: f64,
}

#[derive(Debug, Clone)]
pub(super) struct FFCResNetGeneratorConfig {
    pub input_nc: i64,
    pub output_nc: i64,
    pub ngf: i64,
    pub n_downsampling: usize,
    pub n_blocks: usize,
    pub max_features: i64,
    pub init_conv_kwargs: FFCConfig,
    pub downsample_conv_kwargs: FFCConfig,
    pub resnet_conv_kwargs: FFCConfig,
}

impl Default for FFCResNetGeneratorConfig {
    fn default() -> Self {
        Self {
            input_nc: 4,
            output_nc: 3,
            ngf: 64,
            n_downsampling: 3,
            n_blocks: 18,
            max_features: 1024,
            init_conv_kwargs: FFCConfig {
                ratio_gin: 0.0,
                ratio_gout: 0.0,
            },
            downsample_conv_kwargs: FFCConfig {
                ratio_gin: 0.0,
                ratio_gout: 0.0,
            },
            resnet_conv_kwargs: FFCConfig {
                ratio_gin: 0.75,
                ratio_gout: 0.75,
            },
        }
    }
}
